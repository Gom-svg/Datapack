use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use datapack::metadata::PayloadKind;
use datapack::storage;
use serde_json::Value;

const MIB: usize = 1024 * 1024;

#[test]
fn oversized_header_is_reported_and_compressed_via_byte_exact_raw_fallback() {
    let directory = tempfile::tempdir().expect("create bounded-header directory");
    let input_path = directory.path().join("large-header.csv");
    let archive_path = directory.path().join("large-header.dpack");
    let mut input = Vec::with_capacity(MIB + 32);
    input.extend_from_slice(b"left,");
    input.extend(std::iter::repeat_n(b'x', MIB));
    input.extend_from_slice(b"\n1,value\n");
    fs::write(&input_path, &input).expect("write oversized header");

    let analyze = Command::new(env!("CARGO_BIN_EXE_datapack"))
        .arg("analyze")
        .arg(&input_path)
        .args(["--json", "--sample-mb", "1"])
        .output()
        .expect("run sample/header tie analysis");
    assert_success(&analyze);
    let report: Value = serde_json::from_slice(&analyze.stdout).expect("parse bounded-header JSON");
    assert!(report["dataset"]["column_count"].is_null());
    assert!(report["dataset"]["columns"]
        .as_array()
        .expect("columns array")
        .is_empty());
    assert_eq!(report["sampling"]["bytes_read"], MIB as u64);
    assert_eq!(report["sampling"]["bytes_analyzed"], 0);
    assert_eq!(report["sampling"]["completeness"], "partial");
    assert_eq!(report["sampling"]["limit_reached"], "header_byte_limit");
    assert_eq!(report["planner"]["selection_scope"], "safe_fallback");
    assert_eq!(report["planner"]["selected_archive_mode"], "raw_zstd");
    assert!(has_diagnostic(&report, "HEADER_BYTE_LIMIT_REACHED"));

    let compress = run_compress(&input_path, &archive_path, &["--verify-best"]);
    assert_success(&compress);
    let stderr = String::from_utf8(compress.stderr).expect("compression stderr is UTF-8");
    assert!(stderr.contains("HEADER_BYTE_LIMIT_REACHED"));
    assert!(stderr.contains("streaming RawZstd fallback"));
    assert!(!stderr.contains("verify-best: selected"));
    assert_raw_archive_restores(&archive_path, &input);
}

#[test]
fn huge_unterminated_record_stops_at_the_data_record_limit() {
    let directory = tempfile::tempdir().expect("create bounded-record directory");
    let input_path = directory.path().join("large-record.csv");
    let archive_path = directory.path().join("large-record.dpack");
    let mut input = Vec::with_capacity(8 * MIB + 32);
    input.extend_from_slice(b"left,right\n1,");
    input.extend(std::iter::repeat_n(b'x', 8 * MIB + 1));
    fs::write(&input_path, &input).expect("write oversized record");

    let analyze = run_analyze_json(&input_path);
    assert_success(&analyze);
    let report: Value = serde_json::from_slice(&analyze.stdout).expect("parse bounded-record JSON");
    assert_eq!(report["dataset"]["column_count"], 2);
    assert_eq!(report["sampling"]["bytes_read"], (8 * MIB + 11) as u64);
    assert_eq!(report["sampling"]["bytes_analyzed"], 11);
    assert_eq!(report["sampling"]["records_analyzed"], 0);
    assert_eq!(report["sampling"]["limit_reached"], "record_byte_limit");
    assert_eq!(report["planner"]["selection_scope"], "safe_fallback");
    assert!(has_diagnostic(&report, "RECORD_BYTE_LIMIT_REACHED"));

    let compress = run_compress(&input_path, &archive_path, &["--verify-best"]);
    assert_success(&compress);
    let stderr = String::from_utf8(compress.stderr).expect("compression stderr is UTF-8");
    assert!(stderr.contains("RECORD_BYTE_LIMIT_REACHED"));
    assert!(!stderr.contains("verify-best: selected"));
    assert_raw_archive_restores(&archive_path, &input);
}

#[test]
fn extreme_column_count_is_truthful_without_building_column_profiles() {
    let directory = tempfile::tempdir().expect("create bounded-column directory");
    let input_path = directory.path().join("wide.csv");
    let archive_path = directory.path().join("wide.dpack");
    let header = std::iter::repeat_n("c", 4_097)
        .collect::<Vec<_>>()
        .join(",");
    fs::write(&input_path, format!("{header}\n")).expect("write wide header");

    let analyze = run_analyze_json(&input_path);
    assert_success(&analyze);
    let report: Value = serde_json::from_slice(&analyze.stdout).expect("parse wide JSON");
    assert_eq!(report["dataset"]["column_count"], 4_097);
    assert!(report["dataset"]["columns"]
        .as_array()
        .expect("columns array")
        .is_empty());
    assert_eq!(report["sampling"]["scope"], "full");
    assert_eq!(report["sampling"]["completeness"], "partial");
    assert_eq!(report["sampling"]["limited"], true);
    assert_eq!(report["sampling"]["final_newline"], true);
    assert_eq!(report["sampling"]["limit_reached"], "column_limit");
    assert!(has_diagnostic(&report, "COLUMN_LIMIT_REACHED"));

    let compress = run_compress(&input_path, &archive_path, &["--verify-best"]);
    assert_success(&compress);
    assert_raw_archive_restores(
        &archive_path,
        fs::read(&input_path).expect("read wide input").as_slice(),
    );
}

#[test]
fn cardinality_pressure_is_full_coverage_but_partial_facts() {
    let directory = tempfile::tempdir().expect("create cardinality pressure directory");
    let input_path = directory.path().join("cardinality-pressure.csv");
    let mut input = (0..33)
        .map(|index| format!("c{index}"))
        .collect::<Vec<_>>()
        .join(",");
    input.push('\n');
    for row in 0..8_000 {
        let value = format!("v{row}");
        input.push_str(
            &std::iter::repeat_n(value.as_str(), 33)
                .collect::<Vec<_>>()
                .join(","),
        );
        input.push('\n');
    }
    fs::write(&input_path, input).expect("write cardinality pressure corpus");

    let analyze = run_analyze_json(&input_path);
    assert_success(&analyze);
    let report: Value =
        serde_json::from_slice(&analyze.stdout).expect("parse cardinality pressure JSON");
    assert_eq!(report["sampling"]["scope"], "full");
    assert_eq!(report["sampling"]["completeness"], "partial");
    assert_eq!(report["sampling"]["limited"], true);
    assert_eq!(report["sampling"]["final_newline"], true);
    assert_eq!(
        report["sampling"]["limit_reached"],
        "cardinality_memory_limit"
    );
    assert_eq!(report["planner"]["selection_scope"], "safe_fallback");
    assert_eq!(report["planner"]["selected_archive_mode"], "raw_zstd");
    assert_eq!(
        report["planner"]["reason"]["code"],
        "ANALYSIS_LIMITED_RAW_ZSTD_FALLBACK"
    );
    assert!(has_diagnostic(&report, "CARDINALITY_MEMORY_LIMIT_REACHED"));
}

#[test]
fn malformed_csv_compression_uses_raw_fallback_without_candidate_comparison() {
    let directory = tempfile::tempdir().expect("create malformed fallback directory");
    let input_path = directory.path().join("inconsistent.csv");
    let archive_path = directory.path().join("inconsistent.dpack");
    let input = b"left,right\n1,2\n3\n";
    fs::write(&input_path, input).expect("write malformed CSV");

    let compress = run_compress(&input_path, &archive_path, &["--verify-best"]);
    assert_success(&compress);
    let stderr = String::from_utf8(compress.stderr).expect("compression stderr is UTF-8");
    assert!(stderr.contains("structured analysis unavailable"));
    assert!(stderr.contains("streaming RawZstd fallback"));
    assert!(!stderr.contains("verify-best: selected"));
    assert_raw_archive_restores(&archive_path, input);
}

#[test]
fn estimate_only_max_input_bounds_the_planning_scope() {
    let directory = tempfile::tempdir().expect("create benchmark scope directory");
    let input_path = directory.path().join("benchmark.csv");
    let mut input = String::from("a,b\n");
    let row = format!("{},y\n", "x".repeat(200));
    while input.len() <= 2 * MIB {
        input.push_str(&row);
    }
    fs::write(&input_path, input).expect("write benchmark corpus");

    let output = Command::new(env!("CARGO_BIN_EXE_datapack"))
        .arg("benchmark")
        .arg(&input_path)
        .args(["--estimate-only", "--max-input-mb", "1", "--json"])
        .output()
        .expect("run scoped estimate-only benchmark");
    assert_success(&output);
    let report: Value = serde_json::from_slice(&output.stdout).expect("parse benchmark JSON");
    assert_eq!(
        report["source_size_bytes"],
        fs::metadata(&input_path).unwrap().len()
    );
    assert_eq!(report["measured_input_size_bytes"], MIB as u64);
    assert_eq!(report["planning_sample_bytes"], MIB as u64);
    assert_eq!(report["input_sampled"], true);
    assert_eq!(report["benchmark_scope"], "estimate_only");
}

fn run_analyze_json(input: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_datapack"))
        .arg("analyze")
        .arg(input)
        .arg("--json")
        .output()
        .expect("run JSON analysis")
}

fn run_compress(input: &Path, output: &Path, extra: &[&str]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_datapack"));
    command.arg("compress").arg(input).arg(output).args(extra);
    command.output().expect("run compression")
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "command failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn has_diagnostic(report: &Value, code: &str) -> bool {
    report["diagnostics"]
        .as_array()
        .expect("diagnostics array")
        .iter()
        .any(|diagnostic| diagnostic["code"] == code)
}

fn assert_raw_archive_restores(archive_path: &Path, expected: &[u8]) {
    let archive_bytes = fs::read(archive_path).expect("read fallback archive");
    let archive = storage::decode_archive(&archive_bytes).expect("decode fallback archive");
    assert_eq!(archive.metadata.payload_kind, PayloadKind::RawZstd);
    assert_eq!(
        storage::restore_archive(&archive).expect("restore fallback archive"),
        expected
    );
}
