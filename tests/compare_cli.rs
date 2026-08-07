use std::cmp::Ordering;
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use serde_json::Value;

fn run_compare(input: &Path, arguments: &[&str]) -> Output {
    compare_command(input, arguments)
        .output()
        .expect("run datapack compare")
}

fn run_compare_in(input: &Path, arguments: &[&str], temp_root: &Path) -> Output {
    compare_command(input, arguments)
        .env("DATAPACK_TEMP_DIR", temp_root)
        .output()
        .expect("run datapack compare with isolated temporary root")
}

fn compare_command(input: &Path, arguments: &[&str]) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_datapack"));
    command.arg("compare").arg(input).args(arguments);
    command
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "comparison unexpectedly failed: {}",
        combined_output(output)
    );
    assert!(
        output.stderr.is_empty(),
        "successful comparison wrote stderr: {}",
        combined_output(output)
    );
}

fn assert_command_error(output: &Output, expected: &str) {
    assert_eq!(
        output.status.code(),
        Some(1),
        "semantic/operational comparison error must exit 1: {}",
        combined_output(output)
    );
    assert!(output.stdout.is_empty(), "failed command emitted a report");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(expected),
        "expected stderr to contain {expected:?}, got: {stderr}"
    );
}

fn parse_report(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "stdout was not ComparisonReportV1 JSON: {error}; output: {}",
            combined_output(output)
        )
    })
}

fn combined_output(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn write_repetitive_csv(path: &Path, rows: usize) -> Vec<u8> {
    let mut bytes = Vec::from(&b"group,status,code,note\n"[..]);
    for index in 0..rows {
        let row = if index % 2 == 0 {
            b"A,active,001,repeated\n".as_slice()
        } else {
            b"B,inactive,002,repeated\n".as_slice()
        };
        bytes.extend_from_slice(row);
    }
    fs::write(path, &bytes).expect("write repetitive CSV");
    bytes
}

fn write_repetitive_psv(path: &Path, rows: usize) -> Vec<u8> {
    let mut bytes = Vec::from(&b"group|note|status\n"[..]);
    for _ in 0..rows {
        bytes.extend_from_slice(b"A|\"comma,comma,comma,comma,comma\"|active\n");
    }
    fs::write(path, &bytes).expect("write repetitive PSV");
    bytes
}

fn limitation_codes(report: &Value) -> BTreeSet<&str> {
    report["limitations"]
        .as_array()
        .expect("limitations array")
        .iter()
        .map(|limitation| limitation["code"].as_str().expect("limitation code string"))
        .collect()
}

fn number(value: &Value, context: &str) -> f64 {
    let value = value
        .as_f64()
        .unwrap_or_else(|| panic!("{context} must be a JSON number, got {value}"));
    assert!(value.is_finite(), "{context} must be finite");
    assert!(value >= 0.0, "{context} must be non-negative");
    value
}

fn expected_winner(datapack: f64, zstd: f64) -> &'static str {
    match datapack.partial_cmp(&zstd).expect("finite measurements") {
        Ordering::Less => "datapack",
        Ordering::Equal => "tie",
        Ordering::Greater => "standalone_zstd",
    }
}

fn assert_report_winners_are_self_consistent(report: &Value) {
    let datapack_size = report["datapack"]["artifact_size_bytes"]
        .as_u64()
        .expect("DataPack artifact size");
    let zstd_size = report["standalone_zstd"]["artifact_size_bytes"]
        .as_u64()
        .expect("zstd artifact size");
    let expected_storage = match datapack_size.cmp(&zstd_size) {
        Ordering::Less => "datapack",
        Ordering::Equal => "tie",
        Ordering::Greater => "standalone_zstd",
    };
    assert_eq!(report["winners"]["best_storage_ratio"], expected_storage);

    let datapack_compression = number(
        &report["datapack"]["compression"]["median_ms"],
        "DataPack compression median",
    );
    let zstd_compression = number(
        &report["standalone_zstd"]["compression"]["median_ms"],
        "zstd compression median",
    );
    assert_eq!(
        report["winners"]["fastest_compression"],
        expected_winner(datapack_compression, zstd_compression)
    );

    let datapack_decompression = number(
        &report["datapack"]["decompression"]["median_ms"],
        "DataPack decompression median",
    );
    let zstd_decompression = number(
        &report["standalone_zstd"]["decompression"]["median_ms"],
        "zstd decompression median",
    );
    assert_eq!(
        report["winners"]["fastest_decompression"],
        expected_winner(datapack_decompression, zstd_decompression)
    );
}

#[test]
fn comparison_json_v1_has_strict_private_factual_shape() {
    let directory = tempfile::tempdir().expect("comparison JSON directory");
    let input = directory
        .path()
        .join("PRIVATE_COMPARISON_FILENAME_8C42.csv");
    let private_value = "PRIVATE_COMPARISON_VALUE_9D53";
    let mut bytes = Vec::from(&b"group,status,note\n"[..]);
    for index in 0..256 {
        let row = format!("{},active,{private_value}\n", index % 2);
        bytes.extend_from_slice(row.as_bytes());
    }
    fs::write(&input, &bytes).expect("write private comparison input");

    let output = run_compare(&input, &["--runs", "1", "--json"]);
    assert_success(&output);
    assert_eq!(
        output.stdout.iter().filter(|byte| **byte == b'\n').count(),
        1
    );
    assert_eq!(output.stdout.last(), Some(&b'\n'));
    let report = parse_report(&output);

    assert_eq!(
        object_keys(&report),
        keys(&[
            "datapack",
            "limitations",
            "methodology",
            "mode",
            "report_type",
            "schema_version",
            "scope",
            "standalone_zstd",
            "winners",
        ])
    );
    assert_eq!(report["schema_version"], 1);
    assert_eq!(report["report_type"], "comparison");
    assert_eq!(report["mode"], "quick");
    assert_eq!(
        object_keys(&report["scope"]),
        keys(&[
            "compared_size_bytes",
            "kind",
            "prefix_limited",
            "source_size_bytes",
        ])
    );
    assert_eq!(
        object_keys(&report["methodology"]),
        keys(&[
            "aggregation",
            "artifact_stability",
            "planning_included",
            "planning_time_ms",
            "runs",
            "timing_boundary",
            "validation",
            "zstd_level",
        ])
    );
    assert_eq!(report["methodology"]["runs"], 1);
    assert_eq!(report["methodology"]["aggregation"], "median");
    assert_eq!(report["methodology"]["timing_boundary"], "file_to_file");
    assert_eq!(report["methodology"]["planning_included"], false);
    assert_eq!(report["methodology"]["zstd_level"], 3);
    assert_eq!(
        report["methodology"]["artifact_stability"],
        "sha256_per_run"
    );
    assert_eq!(
        report["methodology"]["validation"],
        "sha256_roundtrip_per_run"
    );
    number(&report["methodology"]["planning_time_ms"], "planning time");

    assert_competitor_shape(&report["datapack"]);
    assert_competitor_shape(&report["standalone_zstd"]);
    assert_eq!(report["datapack"]["artifact_format"], "dpack_v1");
    assert!(matches!(
        report["datapack"]["selected_mode"].as_str(),
        Some("raw_zstd" | "csv_columnar_dictionary")
    ));
    assert_eq!(report["standalone_zstd"]["artifact_format"], "zstd");
    assert_eq!(report["standalone_zstd"]["selected_mode"], Value::Null);
    assert_eq!(
        object_keys(&report["winners"]),
        keys(&[
            "best_storage_ratio",
            "fastest_compression",
            "fastest_decompression",
        ])
    );
    assert_report_winners_are_self_consistent(&report);

    let compared = report["scope"]["compared_size_bytes"]
        .as_u64()
        .expect("compared size");
    for competitor in ["datapack", "standalone_zstd"] {
        let artifact = report[competitor]["artifact_size_bytes"]
            .as_u64()
            .expect("artifact size");
        let ratio = number(
            &report[competitor]["compression_ratio"],
            "compression ratio",
        );
        let expected = compared as f64 / artifact as f64;
        assert!((ratio - expected).abs() < 1e-12);
    }

    for limitation in report["limitations"].as_array().expect("limitations array") {
        assert_eq!(object_keys(limitation), keys(&["code", "message"]));
    }

    let rendered = combined_output(&output);
    for secret in ["PRIVATE_COMPARISON_FILENAME_8C42", private_value] {
        assert!(!rendered.contains(secret), "report disclosed {secret}");
    }
    let lowercase = rendered.to_ascii_lowercase();
    for forbidden in [
        "overall_winner",
        "overall_score",
        "confidence",
        "recommendation",
    ] {
        assert!(
            !lowercase.contains(forbidden),
            "report included {forbidden}"
        );
    }

    let pretty = run_compare(&input, &["--runs", "1", "--json", "--pretty"]);
    assert_success(&pretty);
    assert!(
        pretty.stdout.windows(4).any(|window| window == b"\n  \""),
        "pretty JSON did not contain indented fields"
    );
    assert_eq!(object_keys(&parse_report(&pretty)), object_keys(&report));
}

#[test]
fn quick_scope_is_always_partial_and_truthfully_reports_prefix_limiting() {
    let directory = tempfile::tempdir().expect("Quick scope directory");
    let small = directory.path().join("small.csv");
    let small_bytes = write_repetitive_csv(&small, 64);
    let output = run_compare(&small, &["--runs", "1", "--json"]);
    assert_success(&output);
    let report = parse_report(&output);
    assert_eq!(report["scope"]["kind"], "partial");
    assert_eq!(report["scope"]["source_size_bytes"], small_bytes.len());
    assert_eq!(report["scope"]["compared_size_bytes"], small_bytes.len());
    assert_eq!(report["scope"]["prefix_limited"], false);
    let codes = limitation_codes(&report);
    assert!(codes.contains("QUICK_MODE_PARTIAL"));
    assert!(!codes.contains("INPUT_PREFIX_LIMITED"));
    assert_validated_contenders(&report, "partially_validated", small_bytes.len() as u64);

    let exact = directory.path().join("exact-boundary.bin");
    fs::write(&exact, vec![b'x'; 1_048_576]).expect("write exact-boundary input");
    let output = run_compare(&exact, &["--runs", "1", "--max-input-mb", "1", "--json"]);
    assert_success(&output);
    let report = parse_report(&output);
    assert_eq!(report["scope"]["source_size_bytes"], 1_048_576);
    assert_eq!(report["scope"]["compared_size_bytes"], 1_048_576);
    assert_eq!(report["scope"]["prefix_limited"], false);
    assert!(limitation_codes(&report).contains("QUICK_MODE_PARTIAL"));
    assert!(!limitation_codes(&report).contains("INPUT_PREFIX_LIMITED"));

    let large = directory.path().join("large-unstructured.bin");
    let large_size = 1_100_000usize;
    let bytes = (0..large_size)
        .map(|index| [0xff, 0x00, 0x7f, (index % 251) as u8][index % 4])
        .collect::<Vec<_>>();
    fs::write(&large, bytes).expect("write bounded Quick input");
    let output = run_compare(&large, &["--runs", "1", "--max-input-mb", "1", "--json"]);
    assert_success(&output);
    let report = parse_report(&output);
    assert_eq!(report["scope"]["kind"], "partial");
    assert_eq!(report["scope"]["source_size_bytes"], large_size);
    assert_eq!(report["scope"]["compared_size_bytes"], 1_048_576);
    assert_eq!(report["scope"]["prefix_limited"], true);
    let codes = limitation_codes(&report);
    assert!(codes.contains("QUICK_MODE_PARTIAL"));
    assert!(codes.contains("INPUT_PREFIX_LIMITED"));
    assert_validated_contenders(&report, "partially_validated", 1_048_576);
}

#[test]
fn full_mode_validates_complete_roundtrips_for_both_contenders() {
    let directory = tempfile::tempdir().expect("Full comparison directory");
    let input = directory.path().join("full.csv");
    let bytes = write_repetitive_csv(&input, 1_024);

    let output = run_compare(&input, &["--mode", "full", "--runs", "1", "--json"]);
    assert_success(&output);
    let report = parse_report(&output);
    assert_eq!(report["mode"], "full");
    assert_eq!(report["scope"]["kind"], "full");
    assert_eq!(report["scope"]["source_size_bytes"], bytes.len());
    assert_eq!(report["scope"]["compared_size_bytes"], bytes.len());
    assert_eq!(report["scope"]["prefix_limited"], false);
    assert!(report["limitations"]
        .as_array()
        .expect("Full limitations")
        .iter()
        .all(|value| !matches!(
            value["code"].as_str(),
            Some("QUICK_MODE_PARTIAL" | "INPUT_PREFIX_LIMITED")
        )));
    assert_validated_contenders(&report, "validated", bytes.len() as u64);
    assert_report_winners_are_self_consistent(&report);
}

#[test]
fn empty_input_has_no_storage_ratio_winner_and_no_undefined_rate() {
    let directory = tempfile::tempdir().expect("empty comparison directory");
    let input = directory.path().join("empty.bin");
    fs::write(&input, []).expect("write empty input");

    let output = run_compare(&input, &["--mode", "full", "--runs", "1", "--json"]);
    assert_success(&output);
    let report = parse_report(&output);
    assert_eq!(report["scope"]["compared_size_bytes"], 0);
    assert_eq!(report["datapack"]["compression_ratio"], 0.0);
    assert_eq!(report["standalone_zstd"]["compression_ratio"], 0.0);
    assert_eq!(report["winners"]["best_storage_ratio"], "tie");
    for competitor in ["datapack", "standalone_zstd"] {
        for operation in ["compression", "decompression"] {
            assert_eq!(
                report[competitor][operation]["throughput_mib_per_second"],
                Value::Null
            );
        }
    }
    assert_validated_contenders(&report, "validated", 0);
}

#[test]
fn two_runs_report_every_sample_and_conventional_medians() {
    let directory = tempfile::tempdir().expect("two-run comparison directory");
    let input = directory.path().join("unstructured.bin");
    let bytes = (0..128 * 1024)
        .map(|index| [0xff, 0x00, 0x81, (index % 251) as u8][index % 4])
        .collect::<Vec<_>>();
    fs::write(&input, bytes).expect("write unstructured comparison input");

    let output = run_compare(&input, &["--runs", "2", "--json"]);
    assert_success(&output);
    let report = parse_report(&output);
    assert_eq!(report["methodology"]["runs"], 2);
    assert_eq!(report["methodology"]["aggregation"], "median");
    assert_eq!(report["datapack"]["selected_mode"], "raw_zstd");
    assert!(limitation_codes(&report).contains("STRUCTURED_ANALYSIS_UNAVAILABLE"));

    for competitor in ["datapack", "standalone_zstd"] {
        for operation in ["compression", "decompression"] {
            let timing = &report[competitor][operation];
            let samples = timing["samples_ms"]
                .as_array()
                .expect("timing sample array");
            assert_eq!(samples.len(), 2);
            let first = number(&samples[0], "first timing sample");
            let second = number(&samples[1], "second timing sample");
            let expected = (first + second) / 2.0;
            let median = number(&timing["median_ms"], "timing median");
            assert!(
                (median - expected).abs() <= 0.000_001,
                "{competitor} {operation} median {median} was not the conventional median of {first} and {second}"
            );
        }
    }
    assert_report_winners_are_self_consistent(&report);
}

#[test]
fn repetitive_pipe_input_uses_the_existing_structured_datapack_path() {
    let directory = tempfile::tempdir().expect("structured comparison directory");
    let input = directory.path().join("repetitive.psv");
    let bytes = write_repetitive_psv(&input, 512);

    let output = run_compare(&input, &["--runs", "1", "--json"]);
    assert_success(&output);
    let report = parse_report(&output);
    assert_eq!(
        report["datapack"]["selected_mode"],
        "csv_columnar_dictionary"
    );
    assert_validated_contenders(&report, "partially_validated", bytes.len() as u64);
    assert!(!limitation_codes(&report).contains("STRUCTURED_ENCODER_FALLBACK"));
}

#[test]
fn text_output_reports_factual_metrics_and_independent_winners() {
    let directory = tempfile::tempdir().expect("comparison text directory");
    let input = directory.path().join("text.csv");
    write_repetitive_csv(&input, 128);

    let output = run_compare(&input, &["--runs", "1"]);
    assert_success(&output);
    let text = String::from_utf8(output.stdout).expect("comparison text is UTF-8");
    for expected in [
        "DataPack comparison",
        "Mode: quick",
        "Scope: partial",
        "Runs: 1 (median, file-to-file)",
        "DataPack v1",
        "Standalone zstd (level 3)",
        "archive bytes:",
        "compression ratio:",
        "compression median:",
        "decompression median:",
        "validation: partially_validated (SHA-256 match)",
        "Measured winners",
        "best storage ratio:",
        "fastest compression:",
        "fastest decompression:",
        "QUICK_MODE_PARTIAL:",
    ] {
        assert!(
            text.contains(expected),
            "text report omitted {expected:?}: {text}"
        );
    }
    let lowercase = text.to_ascii_lowercase();
    assert!(!lowercase.contains("overall winner"));
    assert!(!lowercase.contains("score"));
    assert!(!lowercase.contains("confidence"));
}

#[test]
fn compare_rejects_invalid_cli_and_mode_combinations() {
    let directory = tempfile::tempdir().expect("comparison error directory");
    let input = directory.path().join("input.csv");
    write_repetitive_csv(&input, 16);

    let pretty = run_compare(&input, &["--pretty"]);
    assert_eq!(pretty.status.code(), Some(2));
    assert!(pretty.stdout.is_empty());
    assert!(String::from_utf8_lossy(&pretty.stderr).contains("--json"));

    for runs in ["0", "26"] {
        let output = run_compare(&input, &["--runs", runs, "--json"]);
        assert_command_error(&output, "--runs must be between 1 and 25");
    }

    let output = run_compare(
        &input,
        &[
            "--mode",
            "full",
            "--max-input-mb",
            "1",
            "--runs",
            "1",
            "--json",
        ],
    );
    assert_command_error(&output, "--max-input-mb is only valid with --mode quick");
}

#[test]
fn comparison_workspace_is_cleaned_on_success_and_safe_operational_errors() {
    let input_directory = tempfile::tempdir().expect("comparison input directory");
    let workspace_root = tempfile::tempdir().expect("comparison workspace root");
    let input = input_directory.path().join("cleanup.csv");
    let input_before = write_repetitive_csv(&input, 128);

    let output = run_compare_in(
        &input,
        &["--mode", "full", "--runs", "1", "--json"],
        workspace_root.path(),
    );
    assert_success(&output);
    assert!(
        fs::read_dir(workspace_root.path())
            .expect("read workspace root after success")
            .next()
            .is_none(),
        "comparison left temporary artifacts after success"
    );
    assert_eq!(
        fs::read(&input).expect("read input after comparison"),
        input_before
    );

    let missing = input_directory.path().join("missing-input.bin");
    let output = run_compare_in(&missing, &["--runs", "1", "--json"], workspace_root.path());
    assert_command_error(&output, "I/O error");
    assert!(
        fs::read_dir(workspace_root.path())
            .expect("read workspace root after missing input")
            .next()
            .is_none(),
        "comparison left temporary artifacts after an operational error"
    );
}

fn assert_competitor_shape(competitor: &Value) {
    assert_eq!(
        object_keys(competitor),
        keys(&[
            "artifact_format",
            "artifact_size_bytes",
            "compression",
            "compression_ratio",
            "decompression",
            "selected_mode",
            "validation",
        ])
    );
    for operation in ["compression", "decompression"] {
        assert_eq!(
            object_keys(&competitor[operation]),
            keys(&["median_ms", "samples_ms", "throughput_mib_per_second"])
        );
        assert!(competitor[operation]["samples_ms"].is_array());
        number(&competitor[operation]["median_ms"], "operation median");
        number(
            &competitor[operation]["throughput_mib_per_second"],
            "operation throughput",
        );
    }
    assert_eq!(
        object_keys(&competitor["validation"]),
        keys(&["restored_size_bytes", "sha256_match", "status"])
    );
}

fn assert_validated_contenders(report: &Value, status: &str, expected_size: u64) {
    for competitor in ["datapack", "standalone_zstd"] {
        assert_eq!(report[competitor]["validation"]["status"], status);
        assert_eq!(
            report[competitor]["validation"]["restored_size_bytes"],
            expected_size
        );
        assert_eq!(report[competitor]["validation"]["sha256_match"], true);
    }
}

fn object_keys(value: &Value) -> BTreeSet<String> {
    value
        .as_object()
        .expect("value is a JSON object")
        .keys()
        .cloned()
        .collect()
}

fn keys(values: &[&str]) -> BTreeSet<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}
