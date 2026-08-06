use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;

#[test]
fn compact_report_v1_is_deterministic_truthful_and_private() {
    let directory = tempfile::tempdir().expect("create analysis JSON directory");
    let secret_parent = directory.path().join("PRIVATE_PARENT_7F29");
    fs::create_dir(&secret_parent).expect("create private parent marker directory");
    let input_path = secret_parent.join("PRIVATE_FILENAME_6C18.csv");
    let input =
        b"PRIVATE_HEADER_A1,PRIVATE_HEADER_B2\nPRIVATE_ROW_VALUE_C3,7\nPRIVATE_ROW_VALUE_D4,7\n";
    fs::write(&input_path, input).expect("write private analysis corpus");

    let first = run_analyze(&input_path, &["--json"]);
    assert_success(&first);
    assert!(first.stderr.is_empty());
    assert_eq!(
        first.stdout.iter().filter(|byte| **byte == b'\n').count(),
        1
    );
    assert_eq!(first.stdout.last(), Some(&b'\n'));

    let second = run_analyze(&input_path, &["--json"]);
    assert_success(&second);
    assert_eq!(first.stdout, second.stdout);

    let with_legacy_plan_flag = run_analyze(&input_path, &["--json", "--plan"]);
    assert_success(&with_legacy_plan_flag);
    assert_eq!(first.stdout, with_legacy_plan_flag.stdout);

    let report: Value = serde_json::from_slice(&first.stdout).expect("parse compact JSON report");
    assert_eq!(
        object_keys(&report),
        keys(&[
            "dataset",
            "diagnostics",
            "planner",
            "report_type",
            "sampling",
            "schema_version"
        ])
    );
    assert_eq!(report["schema_version"], 1);
    assert_eq!(report["report_type"], "analysis");

    let dataset = &report["dataset"];
    assert_eq!(
        object_keys(dataset),
        keys(&["column_count", "columns", "parser", "source_size_bytes"])
    );
    assert_eq!(dataset["source_size_bytes"], input.len() as u64);
    assert_eq!(dataset["column_count"], 2);
    assert_eq!(dataset["parser"]["format"], "csv");
    assert_eq!(dataset["parser"]["delimiter"], ",");
    assert_eq!(dataset["parser"]["record_model"], "physical_line");
    assert_eq!(dataset["parser"]["header_mode"], "first_record");

    let columns = dataset["columns"].as_array().expect("columns array");
    assert_eq!(columns.len(), 2);
    assert_eq!(
        object_keys(&columns[0]),
        keys(&[
            "cardinality",
            "empty_values",
            "index",
            "name_status",
            "numeric_values",
            "observed_values",
            "planner",
            "repetition_rate",
            "value_length_bytes",
        ])
    );
    assert_eq!(
        object_keys(&columns[0]["name_status"]),
        keys(&["duplicate_of", "is_empty"])
    );
    assert_eq!(
        object_keys(&columns[0]["value_length_bytes"]),
        keys(&["maximum", "mean", "minimum", "total"])
    );
    assert_eq!(
        object_keys(&columns[0]["cardinality"]),
        keys(&["kind", "value"])
    );
    assert_eq!(
        object_keys(&columns[0]["repetition_rate"]),
        keys(&["kind", "value"])
    );
    assert_eq!(
        object_keys(&columns[0]["planner"]),
        keys(&[
            "estimated_dictionary_size_kib",
            "estimated_encoded_size_bytes",
            "estimated_raw_size_bytes",
            "reason",
            "selected_strategy",
        ])
    );
    assert_eq!(
        object_keys(&columns[0]["planner"]["reason"]),
        keys(&["code", "message"])
    );
    assert_eq!(columns[0]["index"], 0);
    assert_eq!(columns[0]["observed_values"], 2);
    assert_eq!(columns[0]["cardinality"]["kind"], "exact");
    assert_eq!(columns[0]["cardinality"]["value"], 2);
    assert_eq!(columns[1]["cardinality"]["value"], 1);
    assert_eq!(columns[1]["planner"]["selected_strategy"], "dictionary");

    let sampling = &report["sampling"];
    assert_eq!(
        object_keys(sampling),
        keys(&[
            "bytes_analyzed",
            "bytes_read",
            "completeness",
            "configured_max_bytes",
            "configured_max_records",
            "final_newline",
            "limit_reached",
            "limited",
            "records_analyzed",
            "scope",
            "source_size_bytes",
        ])
    );
    assert_eq!(sampling["scope"], "full");
    assert_eq!(sampling["completeness"], "complete");
    assert_eq!(sampling["limited"], false);
    assert!(sampling["limit_reached"].is_null());
    assert_eq!(sampling["bytes_read"], input.len() as u64);
    assert_eq!(sampling["bytes_analyzed"], input.len() as u64);
    assert_eq!(sampling["records_analyzed"], 2);
    assert_eq!(sampling["final_newline"], true);

    let planner = &report["planner"];
    assert_eq!(
        object_keys(planner),
        keys(&[
            "candidate_archive_modes",
            "candidate_column_strategies",
            "estimated_dictionary_memory_mib",
            "estimated_savings_percent",
            "policy",
            "reason",
            "selected_archive_mode",
            "selection_scope",
        ])
    );
    assert_eq!(planner["policy"]["name"], "PlannerPolicyV1");
    assert_eq!(planner["policy"]["version"], 1);
    assert_eq!(planner["selection_scope"], "planner_recommendation");
    assert_eq!(
        planner["candidate_archive_modes"],
        serde_json::json!(["csv_columnar_dictionary", "raw_zstd"])
    );
    assert_eq!(
        planner["candidate_column_strategies"],
        serde_json::json!(["dictionary", "plain", "delta_candidate", "raw"])
    );
    assert!(report["diagnostics"]
        .as_array()
        .expect("diagnostics array")
        .is_empty());

    let raw_output = String::from_utf8(first.stdout).expect("JSON output is UTF-8");
    for secret in [
        "PRIVATE_PARENT_7F29",
        "PRIVATE_FILENAME_6C18",
        "PRIVATE_HEADER_A1",
        "PRIVATE_HEADER_B2",
        "PRIVATE_ROW_VALUE_C3",
        "PRIVATE_ROW_VALUE_D4",
    ] {
        assert!(!raw_output.contains(secret), "report disclosed {secret}");
    }
    assert!(!raw_output.contains("planning_time"));
}

#[test]
fn pretty_report_has_the_same_json_value_as_compact() {
    let directory = tempfile::tempdir().expect("create pretty JSON directory");
    let input_path = directory.path().join("input.csv");
    fs::write(&input_path, b"left,right\nA,B\nA,C\n").expect("write pretty JSON corpus");

    let compact = run_analyze(&input_path, &["--json"]);
    let pretty = run_analyze(&input_path, &["--json", "--pretty"]);
    assert_success(&compact);
    assert_success(&pretty);
    assert!(compact.stderr.is_empty());
    assert!(pretty.stderr.is_empty());
    assert!(pretty.stdout.iter().filter(|byte| **byte == b'\n').count() > 2);
    assert!(String::from_utf8_lossy(&pretty.stdout).contains("\n  \"schema_version\""));

    let compact_value: Value = serde_json::from_slice(&compact.stdout).expect("parse compact JSON");
    let pretty_value: Value = serde_json::from_slice(&pretty.stdout).expect("parse pretty JSON");
    assert_eq!(compact_value, pretty_value);
}

#[test]
fn record_limited_sampling_is_partial_and_diagnostic() {
    let directory = tempfile::tempdir().expect("create sampled JSON directory");
    let input_path = directory.path().join("sampled.csv");
    let mut input = String::from("left,right\n");
    for _ in 0..10_001 {
        input.push_str("A,B\n");
    }
    fs::write(&input_path, input).expect("write record-limited corpus");

    let output = run_analyze(&input_path, &["--json", "--sample-mb", "1"]);
    assert_success(&output);
    let report: Value = serde_json::from_slice(&output.stdout).expect("parse sampled JSON");
    let sampling = &report["sampling"];
    assert_eq!(sampling["scope"], "sampled");
    assert_eq!(sampling["completeness"], "partial");
    assert_eq!(sampling["limited"], true);
    assert_eq!(sampling["limit_reached"], "record_limit");
    assert_eq!(sampling["records_analyzed"], 10_000);
    assert!(sampling["final_newline"].is_null());
    assert_eq!(
        report["diagnostics"][0]["code"],
        "SAMPLE_RECORD_LIMIT_REACHED"
    );
    assert!(report["diagnostics"][0]["column_index"].is_null());
}

#[test]
fn byte_limited_sampling_strictly_bounds_read_bytes() {
    let directory = tempfile::tempdir().expect("create byte-limited JSON directory");
    let input_path = directory.path().join("byte-limited.csv");
    let mut input = String::from("left,right\nA,B\nC,");
    input.extend(std::iter::repeat_n('x', 1024 * 1024));
    input.push('\n');
    fs::write(&input_path, input).expect("write byte-limited corpus");

    let output = run_analyze(&input_path, &["--json", "--sample-mb", "1"]);
    assert_success(&output);
    let report: Value = serde_json::from_slice(&output.stdout).expect("parse byte-limited JSON");
    let sampling = &report["sampling"];
    assert_eq!(sampling["scope"], "sampled");
    assert_eq!(sampling["completeness"], "partial");
    assert_eq!(sampling["limited"], true);
    assert_eq!(sampling["limit_reached"], "byte_limit");
    let bytes_read = sampling["bytes_read"].as_u64().expect("bytes_read is u64");
    let bytes_analyzed = sampling["bytes_analyzed"]
        .as_u64()
        .expect("bytes_analyzed is u64");
    let configured_max_bytes = sampling["configured_max_bytes"]
        .as_u64()
        .expect("configured_max_bytes is u64");
    assert_eq!(bytes_read, configured_max_bytes);
    assert!(bytes_analyzed < bytes_read);
    assert_eq!(sampling["records_analyzed"], 1);
    assert!(sampling["final_newline"].is_null());
    assert_eq!(
        report["diagnostics"][0]["code"],
        "SAMPLE_BYTE_LIMIT_REACHED"
    );
}

#[test]
fn cardinality_censorship_is_a_truthful_lower_bound() {
    let directory = tempfile::tempdir().expect("create cardinality JSON directory");
    let input_path = directory.path().join("cardinality.csv");
    let mut input = String::from("identifier,constant\n");
    for index in 0..8_193 {
        input.push_str(&format!("value_{index:04},fixed\n"));
    }
    fs::write(&input_path, input).expect("write cardinality corpus");

    let output = run_analyze(&input_path, &["--json", "--sample-mb", "1"]);
    assert_success(&output);
    let report: Value = serde_json::from_slice(&output.stdout).expect("parse cardinality JSON");
    let first = &report["dataset"]["columns"][0];
    assert_eq!(first["cardinality"]["kind"], "at_least");
    assert_eq!(first["cardinality"]["value"], 8_193);
    assert_eq!(first["repetition_rate"]["kind"], "unknown");
    assert!(first["repetition_rate"]["value"].is_null());
    assert_eq!(
        first["planner"]["reason"]["code"],
        "CARDINALITY_THRESHOLD_EXCEEDED"
    );

    let diagnostics = report["diagnostics"].as_array().expect("diagnostics array");
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic["code"] == "CARDINALITY_LIMIT_REACHED" && diagnostic["column_index"] == 0
    }));
}

#[test]
fn empty_and_duplicate_names_use_indexes_without_disclosure() {
    let directory = tempfile::tempdir().expect("create name diagnostic directory");
    let input_path = directory.path().join("names.csv");
    fs::write(
        &input_path,
        b",PRIVATE_DUPLICATE_HEADER_E5,PRIVATE_DUPLICATE_HEADER_E5\n",
    )
    .expect("write name diagnostic corpus");

    let output = run_analyze(&input_path, &["--json"]);
    assert_success(&output);
    let report: Value = serde_json::from_slice(&output.stdout).expect("parse name JSON");
    let columns = report["dataset"]["columns"]
        .as_array()
        .expect("columns array");
    assert_eq!(columns[0]["name_status"]["is_empty"], true);
    assert!(columns[0]["name_status"]["duplicate_of"].is_null());
    assert_eq!(columns[2]["name_status"]["duplicate_of"], 1);
    assert!(columns[0]["value_length_bytes"]["minimum"].is_null());
    assert!(columns[0]["value_length_bytes"]["maximum"].is_null());
    assert!(columns[0]["value_length_bytes"]["mean"].is_null());
    assert_eq!(columns[0]["repetition_rate"]["kind"], "unknown");

    let diagnostics = report["diagnostics"].as_array().expect("diagnostics array");
    assert_eq!(diagnostics.len(), 2);
    assert_eq!(diagnostics[0]["code"], "EMPTY_COLUMN_NAME");
    assert_eq!(diagnostics[0]["column_index"], 0);
    assert_eq!(diagnostics[1]["code"], "DUPLICATE_COLUMN_NAME");
    assert_eq!(diagnostics[1]["column_index"], 2);
    assert_eq!(
        object_keys(&diagnostics[0]),
        keys(&["code", "column_index", "message", "severity"])
    );
    assert!(!String::from_utf8_lossy(&output.stdout).contains("PRIVATE_DUPLICATE_HEADER_E5"));
}

#[test]
fn planner_modes_strategies_and_reason_codes_come_from_real_analysis() {
    let directory = tempfile::tempdir().expect("create planner JSON directory");

    let unique = analyze_bytes(
        directory.path(),
        "unique.csv",
        b"left,right\nalpha,one\nbeta,two\ngamma,three\n",
    );
    assert_eq!(unique["planner"]["selected_archive_mode"], "raw_zstd");
    assert_eq!(
        unique["planner"]["reason"]["code"],
        "INSUFFICIENT_REPETITION_MAJORITY"
    );
    for column in unique["dataset"]["columns"]
        .as_array()
        .expect("unique columns")
    {
        assert_eq!(column["planner"]["selected_strategy"], "plain");
        assert_eq!(
            column["planner"]["reason"]["code"],
            "NO_STRONG_DICTIONARY_SIGNAL"
        );
    }

    let small_repeat = analyze_bytes(
        directory.path(),
        "small-repeat.csv",
        b"left,right\nfixed,same\nfixed,same\n",
    );
    assert_eq!(small_repeat["planner"]["selected_archive_mode"], "raw_zstd");
    assert_eq!(
        small_repeat["planner"]["reason"]["code"],
        "PROJECTED_DICTIONARY_SAVINGS_BELOW_THRESHOLD"
    );
    for column in small_repeat["dataset"]["columns"]
        .as_array()
        .expect("small-repeat columns")
    {
        assert_eq!(column["planner"]["selected_strategy"], "dictionary");
        assert_eq!(
            column["planner"]["reason"]["code"],
            "REPETITION_SUPPORTS_DICTIONARY"
        );
    }

    let mut repeated_input = String::from("left,right\n");
    for _ in 0..32 {
        repeated_input.push_str("fixed,same\n");
    }
    let repeated = analyze_bytes(directory.path(), "repeated.csv", repeated_input.as_bytes());
    assert_eq!(
        repeated["planner"]["selected_archive_mode"],
        "csv_columnar_dictionary"
    );
    assert_eq!(
        repeated["planner"]["reason"]["code"],
        "HIGH_REPETITION_DETECTED"
    );
    for column in repeated["dataset"]["columns"]
        .as_array()
        .expect("repeated columns")
    {
        assert_eq!(column["planner"]["selected_strategy"], "dictionary");
        assert_eq!(
            column["planner"]["reason"]["code"],
            "VERY_LOW_CARDINALITY_HIGH_REPETITION"
        );
    }

    let numeric = analyze_bytes(
        directory.path(),
        "numeric.csv",
        b"amount,label\n1,alpha\n2,beta\n3,gamma\n",
    );
    assert_eq!(
        numeric["dataset"]["columns"][0]["planner"]["selected_strategy"],
        "delta_candidate"
    );
    assert_eq!(
        numeric["dataset"]["columns"][0]["planner"]["reason"]["code"],
        "NUMERIC_OR_DATE_HEURISTIC_MATCHED"
    );
}

#[test]
fn pretty_without_json_is_rejected_before_input_access() {
    let directory = tempfile::tempdir().expect("create pretty rejection directory");
    let missing_input = directory.path().join("does-not-exist.csv");
    let output = run_analyze(&missing_input, &["--pretty"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).expect("Clap error is UTF-8");
    assert!(stderr.contains("--json"));
    assert!(stderr.contains("required arguments"));
    assert!(!stderr.contains("file not found"));
}

#[test]
fn json_analysis_errors_leave_stdout_empty_and_preserve_legacy_error() {
    let directory = tempfile::tempdir().expect("create JSON error directory");
    let input_path = directory.path().join("inconsistent.csv");
    fs::write(&input_path, b"a,b\n1,2\n3,4,5\n").expect("write inconsistent corpus");

    let output = run_analyze(&input_path, &["--json", "--pretty"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert_eq!(
        output.stderr,
        fs::read(golden_path("inconsistent_width.stderr.golden"))
            .expect("read legacy inconsistent-width golden")
    );
}

fn run_analyze(input: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_datapack"))
        .arg("analyze")
        .arg(input)
        .args(args)
        .output()
        .expect("run datapack analyze")
}

fn analyze_bytes(directory: &Path, name: &str, input: &[u8]) -> Value {
    let input_path = directory.join(name);
    fs::write(&input_path, input).expect("write planner JSON corpus");
    let output = run_analyze(&input_path, &["--json"]);
    assert_success(&output);
    serde_json::from_slice(&output.stdout).expect("parse planner JSON")
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "analyze failed with status {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn object_keys(value: &Value) -> BTreeSet<String> {
    value
        .as_object()
        .expect("JSON object")
        .keys()
        .cloned()
        .collect()
}

fn keys(values: &[&str]) -> BTreeSet<String> {
    values.iter().map(|value| (*value).to_string()).collect()
}

fn golden_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("golden")
        .join("analyze")
        .join(name)
}
