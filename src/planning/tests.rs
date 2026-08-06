use std::fs;

use sha2::{Digest, Sha256};

use super::*;
use crate::analysis;
use crate::error::DatapackError;
use crate::formats::csv::{self, columnar};
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
    assert_eq!(analysis.facts.coverage.sampled_records, 10_000);
    assert_eq!(
        analysis.facts.coverage.bytes_read,
        analysis.facts.source_size_bytes
    );
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
    for column in &analysis.facts.columns {
        assert_eq!(
            column.cardinality,
            analysis::CardinalityEstimate::AtLeast(8_193)
        );
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
    assert_eq!(analysis.facts.source_size_bytes, 544_597);
    assert_eq!(analysis.facts.coverage.bytes_read, 544_597);
    assert_eq!(analysis.facts.coverage.sampled_records, 10_000);
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

    assert_eq!(analysis.facts.source_size_bytes, 1_369_846);
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

    assert_eq!(analysis.facts.source_size_bytes, 1_547_543);
    assert_eq!(analysis.facts.coverage.bytes_read, 1_547_543);
    assert_eq!(analysis.facts.coverage.sampled_records, 10_000);
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

    assert_eq!(analysis.facts.source_size_bytes, 2_996_518);
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
            analysis.facts.source_size_bytes,
            analysis.facts.coverage.bytes_read,
            analysis.facts.coverage.sampled_records
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
            analysis.facts.source_size_bytes,
            analysis.facts.coverage.bytes_read,
            analysis.facts.coverage.sampled_records
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

#[test]
fn legacy_headerless_analysis_routes_remain_explicitly_divergent() {
    let bytes = b"1,Ada,active\r\n2,Grace,active\r\n";
    let (_temp, path) = write_case("headerless.csv", bytes);

    let planner = analyze_path(&path, 64).unwrap();
    assert_eq!(planner.facts.coverage.sampled_records, 1);
    assert_eq!(
        planner
            .columns
            .iter()
            .map(|column| column.column_name.as_str())
            .collect::<Vec<_>>(),
        ["1", "Ada", "active"]
    );

    let public = analysis::analyze_bytes(std::path::Path::new("headerless.csv"), bytes);
    let public_csv = public.csv.expect("public CSV analysis");
    assert!(!public_csv.has_headers);
    assert_eq!(public_csv.total_rows, 2);
    assert_eq!(
        public_csv
            .columns
            .iter()
            .map(|column| column.column_name.as_str())
            .collect::<Vec<_>>(),
        ["column_1", "column_2", "column_3"]
    );

    assert_eq!(
        columnar::CsvSafetyScanner::scan(bytes, b','),
        columnar::CsvSafety::Simple
    );
    let encoded = columnar::encode(bytes).unwrap().unwrap();
    assert_eq!(columnar::decode(&encoded).unwrap(), bytes);
}

#[test]
fn legacy_multiline_record_is_rejected_by_planner_but_accepted_by_codec() {
    let mut bytes = b"id,note\r\n1,\"".to_vec();
    bytes.extend(std::iter::repeat_n(b'x', 64 * 1024));
    bytes.extend_from_slice(b"\ncontinued\"\r\n2,plain\r\n");
    let (_temp, path) = write_case("multiline.csv", &bytes);

    let error = analyze_path(&path, 64).unwrap_err();
    assert!(
        matches!(error, DatapackError::InvalidCsv(ref reason) if reason == "unterminated quoted field")
    );

    let public = analysis::analyze_bytes(std::path::Path::new("multiline.csv"), &bytes);
    let public_csv = public.csv.expect("public CSV analysis");
    assert!(public_csv.has_headers);
    assert_eq!(public_csv.total_rows, 3);
    assert_eq!(public_csv.columns.len(), 2);

    assert_eq!(
        columnar::CsvSafetyScanner::scan(&bytes, b','),
        columnar::CsvSafety::RequiresRfc4180
    );
    let encoded = columnar::encode(&bytes)
        .unwrap()
        .expect("codec accepts one logical multiline record");
    assert_eq!(columnar::decode(&encoded).unwrap(), bytes);
}

#[test]
fn legacy_inconsistent_width_fallback_boundary_is_frozen() {
    let bytes = b"a,b\r\n1,2\r\n3\r\n";
    let (_temp, path) = write_case("inconsistent.csv", bytes);

    assert!(matches!(
        analyze_path(&path, 64),
        Err(DatapackError::InvalidCsv(_))
    ));

    let public = analysis::analyze_bytes(std::path::Path::new("inconsistent.csv"), bytes);
    let public_csv = public.csv.expect("public CSV analysis");
    assert!(public_csv.has_headers);
    assert_eq!(public_csv.total_rows, 2);
    assert_eq!(public_csv.columns.len(), 2);

    assert_eq!(
        columnar::CsvSafetyScanner::scan(bytes, b','),
        columnar::CsvSafety::Unsupported("csv column count is not stable".to_string())
    );
    assert!(columnar::encode(bytes)
        .unwrap_err()
        .to_string()
        .contains("column count is not stable"));
    let (candidate, reason) =
        storage::encode_columnar_dictionary_archive_detailed(&path, bytes).unwrap();
    assert!(candidate.is_none());
    assert!(reason
        .expect("columnar fallback diagnostic")
        .contains("column count is not stable"));

    let raw = storage::encode_raw_zstd_archive(&path, bytes).unwrap();
    let archive = storage::decode_archive(&raw).unwrap();
    assert_eq!(storage::restore_archive(&archive).unwrap(), bytes);
}

#[test]
fn oversized_header_is_strictly_bounded_by_sample_limit() {
    let header_value = "x".repeat(32 * 1024);
    let input = format!("left,{header_value}\n1,value\n");
    let (_temp, path) = write_case("large-header.csv", input.as_bytes());

    let analysis = SampleAnalyzer::new(SampleConfig {
        max_bytes: 64,
        max_rows: 10,
    })
    .analyze_path(&path)
    .unwrap();
    assert_eq!(analysis.facts.coverage.sampled_records, 0);
    assert!(analysis.columns.is_empty());
    assert_eq!(analysis.facts.observed_column_count, None);
    assert_eq!(analysis.facts.coverage.bytes_read, 64);
    assert_eq!(analysis.facts.source_size_bytes, input.len() as u64);
    assert_eq!(analysis.facts.coverage.bytes_analyzed, 0);
    assert_eq!(
        analysis.facts.coverage.stop_reason,
        analysis::AnalysisStopReason::ByteLimit
    );
    assert_eq!(analysis.facts.coverage.final_newline, None);
    assert_eq!(analysis.facts.limitations.len(), 1);
    assert_eq!(
        analysis.facts.limitations[0].code(),
        "INCOMPLETE_HEADER_SAMPLE"
    );
    assert!(analysis.requires_raw_fallback());
    assert_eq!(analysis.plan.archive_mode, ArchiveMode::RawZstd);
}

#[test]
fn unterminated_record_is_strictly_bounded_by_sample_limit() {
    let value = "x".repeat(32 * 1024);
    let input = format!("a,b\n1,{value}");
    let (_temp, path) = write_case("unterminated-record.csv", input.as_bytes());

    let analysis = SampleAnalyzer::new(SampleConfig {
        max_bytes: 64,
        max_rows: 10,
    })
    .analyze_path(&path)
    .unwrap();
    assert_eq!(analysis.facts.coverage.sampled_records, 0);
    assert_eq!(analysis.facts.coverage.bytes_read, 64);
    assert_eq!(analysis.facts.coverage.bytes_analyzed, 4);
    assert_eq!(
        analysis.facts.coverage.stop_reason,
        analysis::AnalysisStopReason::ByteLimit
    );
    assert!(analysis.facts.limitations.is_empty());
    assert!(!analysis.requires_raw_fallback());

    assert_eq!(
        columnar::CsvSafetyScanner::scan(input.as_bytes(), b','),
        columnar::CsvSafety::Simple
    );
    let encoded = columnar::encode(input.as_bytes()).unwrap().unwrap();
    assert_eq!(columnar::decode(&encoded).unwrap(), input.as_bytes());
}

#[test]
fn header_record_column_and_memory_limits_are_typed_safe_fallbacks() {
    let cases = [
        (
            "header-limit.csv",
            b"left,right-hand-header\n1,2\n".as_slice(),
            AnalysisLimits {
                max_header_bytes: 8,
                max_record_bytes: 64,
                max_columns: 8,
                max_global_cardinality_entries: 32,
                max_analysis_memory_bytes: 4096,
            },
            analysis::AnalysisStopReason::HeaderByteLimit,
            "HEADER_BYTE_LIMIT_REACHED",
        ),
        (
            "record-limit-hard.csv",
            b"a,b\n1,a-record-that-is-too-long\n".as_slice(),
            AnalysisLimits {
                max_header_bytes: 64,
                max_record_bytes: 8,
                max_columns: 8,
                max_global_cardinality_entries: 32,
                max_analysis_memory_bytes: 4096,
            },
            analysis::AnalysisStopReason::RecordByteLimit,
            "RECORD_BYTE_LIMIT_REACHED",
        ),
        (
            "column-limit.csv",
            b"a,b,c\n1,2,3\n".as_slice(),
            AnalysisLimits {
                max_header_bytes: 64,
                max_record_bytes: 64,
                max_columns: 2,
                max_global_cardinality_entries: 32,
                max_analysis_memory_bytes: 4096,
            },
            analysis::AnalysisStopReason::ColumnLimit,
            "COLUMN_LIMIT_REACHED",
        ),
        (
            "memory-limit.csv",
            b"a,b\n1,2\n".as_slice(),
            AnalysisLimits {
                max_header_bytes: 64,
                max_record_bytes: 64,
                max_columns: 8,
                max_global_cardinality_entries: 32,
                max_analysis_memory_bytes: 1,
            },
            analysis::AnalysisStopReason::MemoryLimit,
            "ANALYSIS_MEMORY_LIMIT_REACHED",
        ),
    ];

    for (name, input, limits, expected_stop, expected_code) in cases {
        let (_temp, path) = write_case(name, input);
        let analysis = SampleAnalyzer::new(SampleConfig {
            max_bytes: 1024,
            max_rows: 10,
        })
        .with_limits(limits)
        .analyze_path(&path)
        .unwrap();

        assert_eq!(analysis.facts.coverage.stop_reason, expected_stop);
        assert_eq!(analysis.facts.limitations.len(), 1);
        assert_eq!(analysis.facts.limitations[0].code(), expected_code);
        assert!(analysis.requires_raw_fallback());
        assert_eq!(analysis.plan.archive_mode, ArchiveMode::RawZstd);
    }
}

#[test]
fn exact_record_limit_accepts_an_unterminated_record_at_eof() {
    let input = b"a,b\n1,12345";
    let (_temp, path) = write_case("exact-record-limit.csv", input);
    let analysis = SampleAnalyzer::new(SampleConfig {
        max_bytes: 1024,
        max_rows: 10,
    })
    .with_limits(AnalysisLimits {
        max_header_bytes: 64,
        max_record_bytes: 7,
        max_columns: 8,
        max_global_cardinality_entries: 32,
        max_analysis_memory_bytes: 4096,
    })
    .analyze_path(&path)
    .unwrap();

    assert_eq!(analysis.facts.coverage.bytes_read, input.len() as u64);
    assert_eq!(analysis.facts.coverage.sampled_records, 1);
    assert_eq!(
        analysis.facts.coverage.stop_reason,
        analysis::AnalysisStopReason::Complete
    );
    assert!(analysis.facts.limitations.is_empty());
}

#[test]
fn record_limit_wins_an_exact_tie_with_the_remaining_sample_budget() {
    let input = b"a,b\n1,1234567";
    let (_temp, path) = write_case("record-sample-tie.csv", input);
    let analysis = SampleAnalyzer::new(SampleConfig {
        max_bytes: 12,
        max_rows: 10,
    })
    .with_limits(AnalysisLimits {
        max_header_bytes: 64,
        max_record_bytes: 8,
        max_columns: 8,
        max_global_cardinality_entries: 32,
        max_analysis_memory_bytes: 4096,
    })
    .analyze_path(&path)
    .unwrap();

    assert_eq!(analysis.facts.coverage.bytes_read, 12);
    assert_eq!(
        analysis.facts.coverage.stop_reason,
        analysis::AnalysisStopReason::RecordByteLimit
    );
    assert_eq!(
        analysis.facts.limitations[0].code(),
        "RECORD_BYTE_LIMIT_REACHED"
    );
}

#[test]
fn newline_at_the_exact_header_record_and_sample_caps_is_accepted() {
    let input = b"a,b\n1,123\n";
    let (_temp, path) = write_case("exact-newline-caps.csv", input);
    let analysis = SampleAnalyzer::new(SampleConfig {
        max_bytes: 10,
        max_rows: 10,
    })
    .with_limits(AnalysisLimits {
        max_header_bytes: 4,
        max_record_bytes: 6,
        max_columns: 8,
        max_global_cardinality_entries: 32,
        max_analysis_memory_bytes: 4096,
    })
    .analyze_path(&path)
    .unwrap();

    assert_eq!(analysis.facts.coverage.bytes_read, input.len() as u64);
    assert_eq!(analysis.facts.coverage.bytes_analyzed, input.len() as u64);
    assert_eq!(analysis.facts.coverage.sampled_records, 1);
    assert_eq!(
        analysis.facts.coverage.stop_reason,
        analysis::AnalysisStopReason::Complete
    );
    assert!(analysis.facts.limitations.is_empty());
}

#[test]
fn shared_cardinality_budget_censors_without_unbounded_growth() {
    let input = b"a,b\nx,y\nx,y\n";
    let (_temp, path) = write_case("cardinality-budget.csv", input);
    let analysis = SampleAnalyzer::new(SampleConfig {
        max_bytes: 1024,
        max_rows: 10,
    })
    .with_limits(AnalysisLimits {
        max_header_bytes: 64,
        max_record_bytes: 64,
        max_columns: 8,
        max_global_cardinality_entries: 1,
        max_analysis_memory_bytes: 4096,
    })
    .analyze_path(&path)
    .unwrap();

    assert_eq!(
        analysis.facts.coverage.stop_reason,
        analysis::AnalysisStopReason::Complete
    );
    assert_eq!(analysis.facts.limitations.len(), 1);
    assert_eq!(
        analysis.facts.limitations[0].code(),
        "CARDINALITY_MEMORY_LIMIT_REACHED"
    );
    assert!(analysis.facts.columns.iter().any(|column| matches!(
        column.cardinality,
        analysis::CardinalityEstimate::AtLeast(_)
    )));
    assert!(analysis.requires_raw_fallback());
}

#[test]
fn planner_size_aggregation_saturates_instead_of_panicking() {
    let columns = vec![
        ColumnProfile {
            column_index: 0,
            column_name: "left".to_string(),
            unique_count: 1,
            repetition_rate: 0.0,
            avg_value_len_bytes: 1.0,
            estimated_dict_size_kb: 0,
            estimated_encoded_size: u64::MAX,
            estimated_raw_size: u64::MAX,
            recommended_strategy: ColumnStrategy::Raw,
            reason: "No strong dictionary signal".to_string(),
            exceeded_cardinality: false,
        },
        ColumnProfile {
            column_index: 1,
            column_name: "right".to_string(),
            unique_count: 1,
            repetition_rate: 0.0,
            avg_value_len_bytes: 1.0,
            estimated_dict_size_kb: 0,
            estimated_encoded_size: u64::MAX,
            estimated_raw_size: u64::MAX,
            recommended_strategy: ColumnStrategy::Raw,
            reason: "No strong dictionary signal".to_string(),
            exceeded_cardinality: false,
        },
    ];

    let plan = PlannerPolicyV1::plan(&columns, 0);
    assert_eq!(plan.archive_mode, ArchiveMode::RawZstd);
    assert_eq!(plan.estimated_savings_percent, 0.0);
}

#[test]
fn extremely_wide_small_input_remains_index_separated() {
    const WIDTH: usize = 1_024;
    let header = (0..WIDTH)
        .map(|index| format!("c{index}"))
        .collect::<Vec<_>>()
        .join(",");
    let row = std::iter::repeat_n("v", WIDTH)
        .collect::<Vec<_>>()
        .join(",");
    let input = format!("{header}\n{row}\n");
    let (_temp, path) = write_case("wide.csv", input.as_bytes());

    let analysis = analyze_path(&path, 1).unwrap();
    assert_eq!(analysis.facts.coverage.sampled_records, 1);
    assert_eq!(analysis.facts.coverage.bytes_read, input.len() as u64);
    assert_eq!(analysis.columns.len(), WIDTH);
    assert_eq!(analysis.plan.columns.len(), WIDTH);
    assert_eq!(analysis.columns[0].column_name, "c0");
    assert_eq!(analysis.columns[WIDTH - 1].column_name, "c1023");
    assert!(analysis.columns.iter().all(|column| {
        column.unique_count == 1 && column.recommended_strategy == ColumnStrategy::Plain
    }));
    assert_eq!(analysis.plan.archive_mode, ArchiveMode::RawZstd);
}

#[test]
fn ambiguous_delimiter_routes_remain_explicitly_divergent() {
    let bytes = b"a,b|c\n1,2|3\n";
    let (_temp, path) = write_case("ambiguous.csv", bytes);

    let planner = analyze_path(&path, 64).unwrap();
    assert_eq!(
        planner
            .columns
            .iter()
            .map(|column| column.column_name.as_str())
            .collect::<Vec<_>>(),
        ["a", "b|c"]
    );

    let text = std::str::from_utf8(bytes).unwrap();
    assert_eq!(csv::detect_delimiter(text), '|');
    let public = analysis::analyze_bytes(std::path::Path::new("ambiguous.csv"), bytes);
    let public_csv = public.csv.expect("public CSV analysis");
    assert_eq!(public_csv.delimiter, '|');
    assert_eq!(
        public_csv
            .columns
            .iter()
            .map(|column| column.column_name.as_str())
            .collect::<Vec<_>>(),
        ["a,b", "c"]
    );

    assert_eq!(
        columnar::CsvSafetyScanner::scan(bytes, b'|'),
        columnar::CsvSafety::Simple
    );
    let encoded = columnar::encode(bytes).unwrap().unwrap();
    assert_eq!(columnar::decode(&encoded).unwrap(), bytes);
}

#[test]
fn dataset_facts_capture_physical_metrics_and_name_diagnostics() {
    let bytes = ",,name,name\n,,\"\",x\nxx,é,z,\"long\"\n".as_bytes();
    let (_temp, path) = write_case("passive-facts.csv", bytes);

    let analysis = analyze_path(&path, 64).unwrap();
    let facts = &analysis.facts;

    assert_eq!(facts.input_name, "passive-facts.csv");
    assert_eq!(facts.source_size_bytes, bytes.len() as u64);
    assert_eq!(facts.coverage.bytes_read, bytes.len() as u64);
    assert_eq!(facts.coverage.bytes_analyzed, bytes.len() as u64);
    assert_eq!(facts.coverage.sampled_records, 2);
    assert_eq!(
        facts.coverage.stop_reason,
        analysis::AnalysisStopReason::Complete
    );
    assert_eq!(facts.coverage.final_newline, Some(true));
    assert_eq!(facts.columns.len(), 4);
    assert!(facts.columns[0].name_status.is_empty);
    assert_eq!(facts.columns[0].name_status.duplicate_of, None);
    assert!(facts.columns[1].name_status.is_empty);
    assert_eq!(facts.columns[1].name_status.duplicate_of, Some(0));
    assert!(!facts.columns[2].name_status.is_empty);
    assert_eq!(facts.columns[2].name_status.duplicate_of, None);
    assert_eq!(facts.columns[3].name_status.duplicate_of, Some(2));

    assert_eq!(facts.columns[0].empty_values, 1);
    assert_eq!(facts.columns[0].min_value_len_bytes, Some(0));
    assert_eq!(facts.columns[0].max_value_len_bytes, Some(2));
    assert_eq!(facts.columns[1].empty_values, 1);
    assert_eq!(facts.columns[1].max_value_len_bytes, Some(2));
    // The legacy-compatible facts are physical: quoted empty is two bytes.
    assert_eq!(facts.columns[2].empty_values, 0);
    assert_eq!(facts.columns[2].min_value_len_bytes, Some(1));
    assert_eq!(facts.columns[2].max_value_len_bytes, Some(2));
    assert_eq!(facts.columns[3].min_value_len_bytes, Some(1));
    assert_eq!(facts.columns[3].max_value_len_bytes, Some(6));
}

#[test]
fn coverage_distinguishes_bounded_fragment_from_analyzed_bytes() {
    let input = format!("a,b\n1,x\n2,{}\n", "y".repeat(1024));
    let (_temp, path) = write_case("bounded-fragment.csv", input.as_bytes());

    let analysis = SampleAnalyzer::new(SampleConfig {
        max_bytes: 16,
        max_rows: 10,
    })
    .analyze_path(&path)
    .unwrap();

    assert_eq!(analysis.facts.coverage.sampled_records, 1);
    assert_eq!(analysis.facts.coverage.bytes_read, 16);
    assert_eq!(analysis.facts.source_size_bytes, input.len() as u64);
    assert_eq!(analysis.facts.coverage.bytes_analyzed, 8);
    assert_eq!(
        analysis.facts.coverage.stop_reason,
        analysis::AnalysisStopReason::ByteLimit
    );
    assert_eq!(analysis.facts.coverage.final_newline, None);
    assert!(analysis
        .facts
        .columns
        .iter()
        .all(|column| column.observed_values == 1));
}

#[test]
fn coverage_reports_record_limit_without_reading_the_next_record() {
    let bytes = b"a,b\n1,x\n2,y\n";
    let (_temp, path) = write_case("record-limit.csv", bytes);

    let analysis = SampleAnalyzer::new(SampleConfig {
        max_bytes: 1024,
        max_rows: 1,
    })
    .analyze_path(&path)
    .unwrap();

    assert_eq!(analysis.facts.coverage.sampled_records, 1);
    assert_eq!(
        analysis.facts.coverage.source_size_bytes,
        bytes.len() as u64
    );
    assert_eq!(analysis.facts.coverage.bytes_read, 8);
    assert_eq!(analysis.facts.coverage.bytes_analyzed, 8);
    assert_eq!(analysis.facts.coverage.max_bytes, 1024);
    assert_eq!(analysis.facts.coverage.max_records, 1);
    assert_eq!(
        analysis.facts.coverage.stop_reason,
        analysis::AnalysisStopReason::RecordLimit
    );
    assert_eq!(analysis.facts.coverage.final_newline, None);
}

#[test]
fn complete_coverage_reports_missing_final_newline() {
    let bytes = b"a,b\n1,x";
    let (_temp, path) = write_case("no-final-newline.csv", bytes);

    let analysis = analyze_path(&path, 64).unwrap();

    assert_eq!(analysis.facts.coverage.bytes_read, bytes.len() as u64);
    assert_eq!(analysis.facts.coverage.bytes_analyzed, bytes.len() as u64);
    assert_eq!(
        analysis.facts.coverage.stop_reason,
        analysis::AnalysisStopReason::Complete
    );
    assert_eq!(analysis.facts.coverage.final_newline, Some(false));
}
