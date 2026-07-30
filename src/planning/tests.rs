use std::fs;

use sha2::{Digest, Sha256};

use super::*;
use crate::error::DatapackError;
use crate::formats::csv::columnar;
use crate::generation::{self, Profile};
use crate::metadata::{FileType, PayloadKind};
use crate::storage;

const FLOAT_TOLERANCE: f32 = 0.01;

fn generate_sample(
    profile: Profile,
    rows: u64,
    seed: u64,
) -> (tempfile::TempDir, std::path::PathBuf) {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("sample.csv");
    generation::generate_to_path(profile, &path, rows, Some(seed)).unwrap();
    (temp, path)
}

fn write_case(name: &str, bytes: &[u8]) -> (tempfile::TempDir, std::path::PathBuf) {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join(name);
    fs::write(&path, bytes).unwrap();
    (temp, path)
}

fn assert_close(actual: f32, expected: f32) {
    assert!(
        (actual - expected).abs() <= FLOAT_TOLERANCE,
        "expected {expected} +/- {FLOAT_TOLERANCE}, got {actual}"
    );
}

fn assert_raw_censored_profile(analysis: &SampleAnalysis, expected_columns: usize) {
    assert_eq!(analysis.sampled_rows, 10_000);
    assert_eq!(analysis.sampled_bytes, analysis.total_file_size);
    assert_eq!(analysis.columns.len(), expected_columns);
    assert_eq!(analysis.plan.archive_mode, ArchiveMode::RawZstd);
    assert_close(analysis.plan.estimated_savings_percent, 0.0);
    assert_close(analysis.plan.estimated_memory_mb, 0.0);
    assert_eq!(
        analysis.plan.reason,
        "Insufficient repetition across majority of columns for dictionary gains."
    );
    for column in &analysis.columns {
        assert!(column.exceeded_cardinality, "{}", column.column_name);
        assert_eq!(column.unique_count, 10_000);
        assert_close(column.repetition_rate, 0.0);
        assert_eq!(column.recommended_strategy, ColumnStrategy::Raw);
        assert_eq!(column.reason, "Exceeded cardinality threshold");
    }
}

fn candidate_archive_sizes(path: &std::path::Path, bytes: &[u8]) -> (usize, usize) {
    let raw_bytes = storage::encode_raw_zstd_archive(path, bytes).unwrap();
    let raw_archive = storage::decode_archive(&raw_bytes).unwrap();
    assert_eq!(raw_archive.metadata.original_file_type, FileType::Csv);
    assert_eq!(raw_archive.metadata.payload_kind, PayloadKind::RawZstd);
    assert_eq!(storage::restore_archive(&raw_archive).unwrap(), bytes);

    let columnar_bytes = storage::encode_columnar_dictionary_archive(path, bytes)
        .unwrap()
        .expect("legacy corpus must remain a valid columnar candidate");
    let columnar_archive = storage::decode_archive(&columnar_bytes).unwrap();
    assert_eq!(columnar_archive.metadata.original_file_type, FileType::Csv);
    assert_eq!(
        columnar_archive.metadata.payload_kind,
        PayloadKind::CsvColumnarDictionary
    );
    assert_eq!(storage::restore_archive(&columnar_archive).unwrap(), bytes);

    (raw_bytes.len(), columnar_bytes.len())
}

#[test]
fn planner_policy_v1_repetitive_profile_is_frozen() {
    let (_temp, path) = generate_sample(Profile::Repetitive, 10_000, 42);
    let analysis = analyze_path(&path, 64).unwrap();

    assert_eq!(
        analysis.plan.archive_mode,
        ArchiveMode::CsvColumnarDictionary
    );
    assert_eq!(analysis.total_file_size, 544_597);
    assert_eq!(analysis.sampled_bytes, 544_597);
    assert_eq!(analysis.sampled_rows, 10_000);
    assert_eq!(
        analysis.plan.reason,
        "High repetition detected in 10/10 columns."
    );
    assert_close(analysis.plan.estimated_savings_percent, 81.227_26);
    assert_close(analysis.plan.estimated_memory_mb, 0.009_765_625);

    let expected = [
        ("region", 4),
        ("department", 6),
        ("status", 3),
        ("tier", 3),
        ("product_line", 5),
        ("channel", 4),
        ("priority", 3),
        ("category", 5),
        ("sub_category", 4),
        ("flag", 2),
    ];
    assert_eq!(analysis.columns.len(), expected.len());
    for (column, (name, unique)) in analysis.columns.iter().zip(expected) {
        assert_eq!(column.column_name, name);
        assert_eq!(column.unique_count, unique);
        assert_close(column.repetition_rate, 1.0 - unique as f32 / 10_000.0);
        assert_eq!(column.recommended_strategy, ColumnStrategy::Dictionary);
        assert_eq!(column.reason, "Very low cardinality with high repetition");
        assert!(!column.exceeded_cardinality);
    }

    let bytes = fs::read(&path).unwrap();
    assert_eq!(candidate_archive_sizes(&path, &bytes), (76_493, 28_925));
}

#[test]
fn planner_policy_v1_random_profile_is_frozen() {
    let (_temp, path) = generate_sample(Profile::Random, 10_000, 99);
    let analysis = analyze_path(&path, 64).unwrap();

    assert_eq!(analysis.total_file_size, 1_369_846);
    assert_raw_censored_profile(&analysis, 8);

    let bytes = fs::read(&path).unwrap();
    assert_eq!(
        candidate_archive_sizes(&path, &bytes),
        (1_040_834, 1_002_499)
    );
}

#[test]
fn random_high_cardinality_repetition_is_conservative() {
    let (_temp, path) = generate_sample(Profile::Random, 10_000, 99);
    let analysis = analyze_path(&path, 64).unwrap();

    assert!(analysis
        .columns
        .iter()
        .filter(|column| column.exceeded_cardinality)
        .all(|column| column.repetition_rate <= 0.01));
}

#[test]
fn plan_03_high_unique_column_is_raw() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("unique.csv");
    let mut csv = String::from("unique_value,tag\r\n");
    for index in 0..70_000u64 {
        csv.push_str(&format!("value_{index:08},x\r\n"));
    }
    fs::write(&path, csv).unwrap();

    let analysis = analyze_path(&path, 64).unwrap();

    assert_eq!(
        analysis.columns[0].recommended_strategy,
        ColumnStrategy::Raw
    );
}

#[test]
fn plan_04_dictionary_value_limit_switches_to_plain() {
    let (_temp, path) = generate_sample(Profile::Repetitive, 10_000, 42);
    let analysis = analyze_path(&path, 64).unwrap();
    let mut plan = analysis.plan.clone();

    let warnings = apply_dictionary_limits(&mut plan, &analysis.columns, 1, 64);

    assert!(!warnings.is_empty());
    assert!(plan
        .columns
        .iter()
        .any(|column| column.strategy == ColumnStrategy::Plain));
}

#[test]
fn plan_05_dictionary_memory_limit_switches_to_plain() {
    let (_temp, path) = generate_sample(Profile::Repetitive, 10_000, 42);
    let analysis = analyze_path(&path, 64).unwrap();
    let mut plan = analysis.plan.clone();

    let warnings = apply_dictionary_limits(&mut plan, &analysis.columns, 65_535, 0);

    assert!(!warnings.is_empty());
    assert!(plan
        .columns
        .iter()
        .any(|column| column.strategy == ColumnStrategy::Plain));
}

#[test]
fn large_repeated_notes_dictionary_limit_switches_to_plain() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("notes.csv");
    let mut csv = String::from("id,note\r\n");
    for index in 0..2_000u64 {
        csv.push_str(&format!(
            "{index},\"repeated fictitious note with a comma, stable quoted layout and batch {}\"\r\n",
            index % 4
        ));
    }
    fs::write(&path, csv).unwrap();

    let analysis = analyze_path(&path, 64).unwrap();
    let mut plan = analysis.plan.clone();
    let warnings = apply_dictionary_limits(&mut plan, &analysis.columns, 65_535, 0);

    assert!(warnings.iter().any(|column| column == "note"));
    assert!(plan
        .columns
        .iter()
        .any(|column| column.column_name == "note" && column.strategy == ColumnStrategy::Plain));
}

#[test]
fn plan_06_fast_mode_style_single_plan_round_trip() {
    let (_temp, path) = generate_sample(Profile::Repetitive, 50_000, 42);
    let bytes = fs::read(&path).unwrap();
    let analysis = analyze_path(&path, 64).unwrap();
    let archive_bytes = match analysis.plan.archive_mode {
        ArchiveMode::RawZstd => storage::encode_raw_zstd_archive(&path, &bytes).unwrap(),
        ArchiveMode::CsvColumnarDictionary => {
            storage::encode_columnar_dictionary_archive(&path, &bytes)
                .unwrap()
                .unwrap_or_else(|| storage::encode_raw_zstd_archive(&path, &bytes).unwrap())
        }
    };
    let archive = storage::decode_archive(&archive_bytes).unwrap();
    let restored = storage::restore_archive(&archive).unwrap();

    assert_eq!(restored, bytes);
}

#[test]
fn plan_07_verify_best_style_high_cardinality_keeps_smaller_without_temp() {
    let (_temp, path) = generate_sample(Profile::HighCardinality, 5_000, 7);
    let bytes = fs::read(&path).unwrap();
    let raw = storage::encode_raw_zstd_archive(&path, &bytes).unwrap();
    let columnar = storage::encode_columnar_dictionary_archive(&path, &bytes).unwrap();
    let selected = columnar
        .filter(|candidate| candidate.len() < raw.len())
        .unwrap_or(raw);
    let archive = storage::decode_archive(&selected).unwrap();
    let restored = storage::restore_archive(&archive).unwrap();

    assert_eq!(restored, bytes);
}

#[test]
fn planner_policy_v1_realistic_profile_is_frozen() {
    let (_temp, path) = generate_sample(Profile::Realistic, 10_000, 2026);
    let analysis = analyze_path(&path, 64).unwrap();

    assert_eq!(analysis.total_file_size, 1_547_543);
    assert_eq!(analysis.sampled_bytes, 1_547_543);
    assert_eq!(analysis.sampled_rows, 10_000);
    assert_eq!(
        analysis.plan.archive_mode,
        ArchiveMode::CsvColumnarDictionary
    );
    assert_eq!(
        analysis.plan.reason,
        "High repetition detected in 10/12 columns."
    );
    assert_close(analysis.plan.estimated_savings_percent, 46.090_23);
    assert_close(analysis.plan.estimated_memory_mb, 0.408_203_13);

    let names = [
        "record_id",
        "client_name",
        "region",
        "department",
        "account_type",
        "transaction_date",
        "amount",
        "currency",
        "status",
        "payment_method",
        "notes",
        "checksum",
    ];
    let unique = [10_000, 80, 4, 6, 4, 1_051, 7_001, 3, 5, 4, 4_364, 10_000];
    assert_eq!(analysis.columns.len(), names.len());
    for (index, column) in analysis.columns.iter().enumerate() {
        assert_eq!(column.column_name, names[index]);
        assert_eq!(column.unique_count, unique[index]);
        if matches!(index, 0 | 11) {
            assert!(column.exceeded_cardinality);
            assert_eq!(column.recommended_strategy, ColumnStrategy::Raw);
            assert_eq!(column.reason, "Exceeded cardinality threshold");
        } else {
            assert!(!column.exceeded_cardinality);
            assert_eq!(column.recommended_strategy, ColumnStrategy::Dictionary);
        }
    }
    for index in [1, 2, 3, 4, 7, 8, 9] {
        assert_eq!(
            analysis.columns[index].reason,
            "Very low cardinality with high repetition"
        );
    }
    for index in [5, 6, 10] {
        assert_eq!(
            analysis.columns[index].reason,
            "Repeated values likely benefit from dictionary IDs"
        );
    }

    let bytes = fs::read(&path).unwrap();
    assert_eq!(candidate_archive_sizes(&path, &bytes), (298_855, 206_276));
}

#[test]
fn planner_policy_v1_high_cardinality_profile_is_frozen() {
    let (_temp, path) = generate_sample(Profile::HighCardinality, 10_000, 7);
    let analysis = analyze_path(&path, 64).unwrap();

    assert_eq!(analysis.total_file_size, 2_996_518);
    assert_raw_censored_profile(&analysis, 10);

    let bytes = fs::read(&path).unwrap();
    assert_eq!(
        candidate_archive_sizes(&path, &bytes),
        (1_507_722, 1_269_920)
    );
}

#[test]
fn planner_policy_v1_line_local_rfc4180_case_is_frozen() {
    let bytes = b"id,note,status\r\n1,\"hello, world\",open\r\n2,\"hello, world\",open\r\n3,plain,closed\r\n";
    let (_temp, path) = write_case("rfc4180.csv", bytes);
    let analysis = analyze_path(&path, 64).unwrap();

    assert_eq!(
        (
            analysis.total_file_size,
            analysis.sampled_bytes,
            analysis.sampled_rows
        ),
        (78, 78, 3)
    );
    assert_eq!(
        analysis
            .columns
            .iter()
            .map(|column| column.column_name.as_str())
            .collect::<Vec<_>>(),
        ["id", "note", "status"]
    );
    assert_eq!(
        analysis
            .columns
            .iter()
            .map(|column| column.unique_count)
            .collect::<Vec<_>>(),
        [3, 2, 2]
    );
    assert_eq!(
        analysis
            .columns
            .iter()
            .map(|column| column.recommended_strategy)
            .collect::<Vec<_>>(),
        [
            ColumnStrategy::DeltaCandidate,
            ColumnStrategy::Dictionary,
            ColumnStrategy::Dictionary,
        ]
    );
    assert_eq!(analysis.columns[0].reason, "Numeric/date heuristic matched");
    assert_eq!(
        analysis.columns[1].reason,
        "Repeated values likely benefit from dictionary IDs"
    );
    assert_eq!(
        analysis.columns[2].reason,
        "Repeated values likely benefit from dictionary IDs"
    );
    assert_eq!(
        (
            analysis.columns[0].estimated_encoded_size,
            analysis.columns[0].estimated_raw_size
        ),
        (18, 6)
    );
    assert_eq!(
        (
            analysis.columns[1].estimated_encoded_size,
            analysis.columns[1].estimated_raw_size
        ),
        (33, 36)
    );
    assert_eq!(
        (
            analysis.columns[2].estimated_encoded_size,
            analysis.columns[2].estimated_raw_size
        ),
        (21, 17)
    );
    assert_eq!(analysis.plan.archive_mode, ArchiveMode::RawZstd);
    assert_eq!(
        analysis.plan.reason,
        "Projected dictionary savings < 5%; RawZstd is more predictable."
    );
    assert_close(analysis.plan.estimated_savings_percent, 0.0);
    assert_close(analysis.plan.estimated_memory_mb, 2.0 / 1024.0);

    assert_eq!(candidate_archive_sizes(&path, bytes), (117, 148));
}

#[test]
fn planner_policy_v1_unfavorable_csv_is_frozen() {
    let bytes = b"id,value\r\n1,alpha\r\n2,beta\r\n3,gamma\r\n4,delta\r\n";
    let (_temp, path) = write_case("unfavorable.csv", bytes);
    let analysis = analyze_path(&path, 64).unwrap();

    assert_eq!(
        (
            analysis.total_file_size,
            analysis.sampled_bytes,
            analysis.sampled_rows
        ),
        (45, 45, 4)
    );
    assert_eq!(
        analysis
            .columns
            .iter()
            .map(|column| column.recommended_strategy)
            .collect::<Vec<_>>(),
        [ColumnStrategy::DeltaCandidate, ColumnStrategy::Plain]
    );
    assert_eq!(analysis.columns[0].reason, "Numeric/date heuristic matched");
    assert_eq!(analysis.columns[1].reason, "No strong dictionary signal");
    assert_eq!(analysis.plan.archive_mode, ArchiveMode::RawZstd);
    assert_eq!(
        analysis.plan.reason,
        "Insufficient repetition across majority of columns for dictionary gains."
    );
    assert_close(analysis.plan.estimated_savings_percent, 0.0);

    let encoded = columnar::encode(bytes)
        .unwrap()
        .expect("structurally valid DCSV01 candidate");
    assert_eq!(columnar::decode(&encoded).unwrap(), bytes);
    assert_eq!(candidate_archive_sizes(&path, bytes), (98, 131));
}

#[test]
fn plan_09_binary_input_is_invalid_csv() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("binary.csv");
    fs::write(&path, [0, 159, 146, 150]).unwrap();

    assert!(matches!(
        analyze_path(&path, 64),
        Err(DatapackError::InvalidCsv(_))
    ));
}

#[test]
fn plan_10_roundtrip_sha256_matches_disk_hash() {
    let (_temp, path) = generate_sample(Profile::Repetitive, 10_000, 42);
    let bytes = fs::read(&path).unwrap();
    let archive_bytes = storage::encode_adaptive_archive(&path, &bytes).unwrap();
    let archive = storage::decode_archive(&archive_bytes).unwrap();
    let restored = storage::restore_archive(&archive).unwrap();

    assert_eq!(Sha256::digest(&bytes), Sha256::digest(&restored));
}
