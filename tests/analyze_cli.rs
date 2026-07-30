use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use tempfile::TempDir;

const PLANNING_TIME_PREFIX: &str = "Planning time:       ";
const PLANNING_TIME_SUFFIX: &str = " ms";

// Corpus-to-requirement mapping:
// - simple_header_lf: simple CSV, explicit header, LF, default renderer.
// - repetitive_crlf_plan: CRLF and the --plan renderer.
// - no_header_legacy: first data row is consumed as the legacy header.
// - quoted_delimiter: RFC-style quoted commas on one physical record.
// - empty_utf8: empty fields and multibyte UTF-8 values.
// - high_cardinality_small: a small sample whose primary column is all-unique.
// - one_column, empty_file, inconsistent_width: legacy error/exit behavior.

#[test]
fn analyze_simple_header_lf_matches_golden() {
    assert_success(
        "simple_header_lf",
        "simple_header_lf.csv",
        b"id,name,status\n1,Ada,active\n2,Grace,active\n3,Linus,inactive\n",
        &[],
        false,
    );
}

#[test]
fn analyze_repetitive_crlf_plan_matches_golden() {
    assert_success(
        "repetitive_crlf_plan",
        "repetitive_crlf_plan.csv",
        b"code,message\r\nAA,repeated-message\r\nAA,repeated-message\r\nAA,repeated-message\r\nAA,repeated-message\r\n",
        &["--plan"],
        true,
    );
}

#[test]
fn analyze_no_header_legacy_behavior_matches_golden() {
    assert_success(
        "no_header_legacy",
        "no_header_legacy.csv",
        b"1,Ada\n2,Grace\n3,Linus\n",
        &[],
        false,
    );
}

#[test]
fn analyze_quoted_delimiter_matches_golden() {
    assert_success(
        "quoted_delimiter",
        "quoted_delimiter.csv",
        b"id,note\n1,\"hello,world\"\n2,\"hello,world\"\n3,\"other,value\"\n",
        &[],
        false,
    );
}

#[test]
fn analyze_empty_fields_and_utf8_match_golden() {
    assert_success(
        "empty_utf8",
        "empty_utf8.csv",
        "id,city,note\n1,東京,\n2,東京,\n3,,vacío\n4,東京,\n".as_bytes(),
        &[],
        false,
    );
}

#[test]
fn analyze_small_high_cardinality_sample_matches_golden() {
    let mut input = String::from("value,tag\n");
    for index in 0..16 {
        input.push_str(&format!("value_{index:02},A\n"));
    }
    assert_success(
        "high_cardinality_small",
        "high_cardinality_small.csv",
        input.as_bytes(),
        &[],
        false,
    );
}

#[test]
fn analyze_one_column_error_matches_golden() {
    assert_failure(
        "one_column",
        "one_column.csv",
        b"\"only,column\"\n\"value,with,commas\"\n",
        2,
    );
}

#[test]
fn analyze_empty_file_error_matches_golden() {
    assert_failure("empty_file", "empty_file.csv", b"", 2);
}

#[test]
fn analyze_inconsistent_width_error_matches_golden() {
    assert_failure(
        "inconsistent_width",
        "inconsistent_width.csv",
        b"a,b\n1,2\n3,4,5\n",
        2,
    );
}

fn assert_success(
    golden_name: &str,
    file_name: &str,
    input: &[u8],
    extra_args: &[&str],
    expects_planning_time: bool,
) {
    let (_directory, output) = run_analyze(file_name, input, extra_args);
    assert_eq!(
        output.status.code(),
        Some(0),
        "analyze unexpectedly failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).expect("analyze stdout must be UTF-8");
    let (stdout, normalized_count) = normalize_planning_time(&stdout);
    assert_eq!(
        normalized_count,
        usize::from(expects_planning_time),
        "unexpected number of normalized planning-time lines"
    );
    assert_eq!(stdout, read_golden(&format!("{golden_name}.stdout.golden")));
    assert_eq!(output.stderr, read_golden_bytes("empty.golden"));
}

fn assert_failure(golden_name: &str, file_name: &str, input: &[u8], expected_exit: i32) {
    let (_directory, output) = run_analyze(file_name, input, &[]);
    assert_eq!(output.status.code(), Some(expected_exit));
    assert_eq!(output.stdout, read_golden_bytes("empty.golden"));
    assert_eq!(
        output.stderr,
        read_golden_bytes(&format!("{golden_name}.stderr.golden"))
    );
}

fn run_analyze(file_name: &str, input: &[u8], extra_args: &[&str]) -> (TempDir, Output) {
    let directory = tempfile::tempdir().expect("temporary analyze corpus directory");
    let input_path = directory.path().join(file_name);
    fs::write(&input_path, input).expect("write deterministic analyze corpus case");

    let output = Command::new(env!("CARGO_BIN_EXE_datapack"))
        .arg("analyze")
        .arg(&input_path)
        .args(extra_args)
        .output()
        .expect("run datapack analyze");
    (directory, output)
}

fn normalize_planning_time(output: &str) -> (String, usize) {
    let mut normalized = String::with_capacity(output.len());
    let mut normalized_count = 0;

    for segment in output.split_inclusive('\n') {
        let (line, newline) = segment
            .strip_suffix('\n')
            .map_or((segment, ""), |line| (line, "\n"));
        if let Some(value) = line
            .strip_prefix(PLANNING_TIME_PREFIX)
            .and_then(|value| value.strip_suffix(PLANNING_TIME_SUFFIX))
        {
            value
                .parse::<u64>()
                .expect("planning time must remain an integer number of milliseconds");
            normalized.push_str(PLANNING_TIME_PREFIX);
            normalized.push_str("<normalized>");
            normalized.push_str(PLANNING_TIME_SUFFIX);
            normalized.push_str(newline);
            normalized_count += 1;
        } else {
            normalized.push_str(segment);
        }
    }

    (normalized, normalized_count)
}

fn read_golden(name: &str) -> String {
    fs::read_to_string(golden_path(name)).expect("read analyze text golden")
}

fn read_golden_bytes(name: &str) -> Vec<u8> {
    fs::read(golden_path(name)).expect("read analyze byte golden")
}

fn golden_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("golden")
        .join("analyze")
        .join(name)
}
