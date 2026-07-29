use std::fs;

use sha2::{Digest, Sha256};

use super::*;
use crate::error::DatapackError;
use crate::generation::{self, Profile};
use crate::storage;

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

#[test]
fn plan_01_repetitive_profile_selects_columnar() {
    let (_temp, path) = generate_sample(Profile::Repetitive, 10_000, 42);
    let analysis = analyze_path(&path, 64).unwrap();

    assert_eq!(
        analysis.plan.archive_mode,
        ArchiveMode::CsvColumnarDictionary
    );
}

#[test]
fn plan_02_random_profile_uses_safe_plan() {
    let (_temp, path) = generate_sample(Profile::Random, 10_000, 99);
    let analysis = analyze_path(&path, 64).unwrap();

    assert!(
        analysis.plan.archive_mode == ArchiveMode::RawZstd
            || analysis.plan.estimated_savings_percent < 15.0
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
fn plan_08_realistic_analyze_plan_is_valid_with_ten_columns() {
    let (_temp, path) = generate_sample(Profile::Realistic, 10_000, 2026);
    let analysis = analyze_path(&path, 64).unwrap();

    assert!(matches!(
        analysis.plan.archive_mode,
        ArchiveMode::CsvColumnarDictionary | ArchiveMode::RawZstd
    ));
    assert!(analysis.plan.columns.len() >= 10);
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
