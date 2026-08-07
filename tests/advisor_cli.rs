use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use serde_json::Value;

const MIB: usize = 1024 * 1024;

#[test]
fn advisor_json_v1_is_deterministic_private_and_explainable() {
    let directory = tempfile::tempdir().expect("create Advisor JSON directory");
    let private_parent = directory.path().join("PRIVATE_ADVISOR_PARENT_2A71");
    fs::create_dir(&private_parent).expect("create private Advisor parent");
    let input = private_parent.join("PRIVATE_ADVISOR_FILENAME_8B43.csv");
    let mut bytes = Vec::from(&b"PRIVATE_ADVISOR_HEADER_C19,PRIVATE_ADVISOR_HEADER_D28\n"[..]);
    for _ in 0..64 {
        bytes.extend_from_slice(b"PRIVATE_ADVISOR_VALUE_E37,PRIVATE_ADVISOR_VALUE_F46\n");
    }
    fs::write(&input, &bytes).expect("write private Advisor corpus");

    let first = run_advisor(&input, &["--sample-mb", "1", "--json"]);
    assert_success(&first);
    assert!(first.stderr.is_empty());
    assert_eq!(
        first.stdout.iter().filter(|byte| **byte == b'\n').count(),
        1
    );
    assert_eq!(first.stdout.last(), Some(&b'\n'));

    let second = run_advisor(&input, &["--sample-mb", "1", "--json"]);
    assert_success(&second);
    assert_eq!(first.stdout, second.stdout);

    let report = parse_report(&first);
    assert_report_root(&report);
    assert_eq!(report["policy"]["name"], "AdvisorPolicyV1");
    assert_eq!(report["policy"]["version"], 1);

    let analysis = &report["analysis"];
    assert_analysis_shape(analysis);
    assert_eq!(analysis["status"], "available");
    assert_eq!(analysis["scope"], "full");
    assert_eq!(analysis["completeness"], "complete");
    assert_eq!(analysis["source_size_bytes"], bytes.len() as u64);
    assert_eq!(analysis["bytes_analyzed"], bytes.len() as u64);
    assert_eq!(analysis["records_analyzed"], 64);
    assert!(analysis["high_cardinality_columns"]
        .as_array()
        .expect("high-cardinality array")
        .is_empty());
    assert_planner(
        &analysis["planner"],
        "csv_columnar_dictionary",
        "HIGH_REPETITION_DETECTED",
        "planner_recommendation",
    );
    assert_evidence(
        &analysis["evidence"],
        "planner_policy",
        "HIGH_REPETITION_DETECTED",
        None,
    );

    assert_eq!(
        recommendation_codes(&report),
        vec!["STRUCTURED_COMPRESSION_RECOMMENDED"]
    );
    let structured = recommendation(&report, "STRUCTURED_COMPRESSION_RECOMMENDED");
    assert_recommendation_shape(structured);
    assert_eq!(structured["category"], "compression_strategy");
    assert_evidence(
        &structured["evidence"],
        "planner_policy",
        "HIGH_REPETITION_DETECTED",
        None,
    );

    let combined = combined_output(&first);
    for private in [
        "PRIVATE_ADVISOR_PARENT_2A71",
        "PRIVATE_ADVISOR_FILENAME_8B43",
        "PRIVATE_ADVISOR_HEADER_C19",
        "PRIVATE_ADVISOR_HEADER_D28",
        "PRIVATE_ADVISOR_VALUE_E37",
        "PRIVATE_ADVISOR_VALUE_F46",
    ] {
        assert!(!combined.contains(private), "Advisor disclosed {private}");
    }
    assert_no_forbidden_vocabulary(&report, &combined);

    let pretty = run_advisor(&input, &["--sample-mb", "1", "--json", "--pretty"]);
    assert_success(&pretty);
    assert!(pretty.stderr.is_empty());
    assert!(
        pretty.stdout.windows(4).any(|window| window == b"\n  \""),
        "pretty Advisor JSON was not indented"
    );
    assert_eq!(parse_report(&pretty), report);

    let text = run_advisor(&input, &["--sample-mb", "1"]);
    assert_success(&text);
    assert!(text.stderr.is_empty());
    let text_rendered = combined_output(&text);
    for private in [
        "PRIVATE_ADVISOR_PARENT_2A71",
        "PRIVATE_ADVISOR_FILENAME_8B43",
        "PRIVATE_ADVISOR_HEADER_C19",
        "PRIVATE_ADVISOR_HEADER_D28",
        "PRIVATE_ADVISOR_VALUE_E37",
        "PRIVATE_ADVISOR_VALUE_F46",
    ] {
        assert!(
            !text_rendered.contains(private),
            "Advisor text disclosed {private}"
        );
    }
    assert_no_forbidden_vocabulary(&report, &text_rendered);

    let entries = fs::read_dir(&private_parent)
        .expect("read Advisor input directory")
        .collect::<Result<Vec<_>, _>>()
        .expect("collect Advisor input directory entries");
    assert_eq!(entries.len(), 1, "Advisor created an unexpected artifact");
    assert_eq!(entries[0].path(), input);
}

#[test]
fn complete_unique_analysis_recommends_raw_zstd_from_planner_facts() {
    let directory = tempfile::tempdir().expect("create raw Advisor directory");
    let input = directory.path().join("unique.csv");
    fs::write(&input, b"left,right\nalpha,one\nbeta,two\ngamma,three\n")
        .expect("write unique Advisor corpus");

    let output = run_advisor(&input, &["--json"]);
    assert_success(&output);
    let report = parse_report(&output);
    assert_report_root(&report);
    assert_eq!(report["analysis"]["status"], "available");
    assert_eq!(report["analysis"]["scope"], "full");
    assert_eq!(report["analysis"]["completeness"], "complete");
    assert_planner(
        &report["analysis"]["planner"],
        "raw_zstd",
        "INSUFFICIENT_REPETITION_MAJORITY",
        "planner_recommendation",
    );
    assert_eq!(recommendation_codes(&report), vec!["RAW_ZSTD_RECOMMENDED"]);
    let raw = recommendation(&report, "RAW_ZSTD_RECOMMENDED");
    assert_eq!(raw["category"], "compression_strategy");
    assert_evidence(
        &raw["evidence"],
        "planner_policy",
        "INSUFFICIENT_REPETITION_MAJORITY",
        None,
    );

    let text = run_advisor(&input, &[]);
    assert_success(&text);
    assert!(text.stderr.is_empty());
    let stdout = String::from_utf8(text.stdout).expect("Advisor text is UTF-8");
    assert!(stdout.contains("RAW_ZSTD_RECOMMENDED"));
    assert!(stdout.contains("INSUFFICIENT_REPETITION_MAJORITY"));
}

#[test]
fn partial_analysis_preserves_planner_advice_and_recommends_full_comparison() {
    let directory = tempfile::tempdir().expect("create limited Advisor directory");
    let input = directory.path().join("record-limited.csv");
    let mut bytes = Vec::from(&b"left,right\n"[..]);
    for _ in 0..10_001 {
        bytes.extend_from_slice(b"A,B\n");
    }
    fs::write(&input, &bytes).expect("write record-limited Advisor corpus");

    let output = run_advisor(&input, &["--sample-mb", "1", "--json"]);
    assert_success(&output);
    let report = parse_report(&output);
    let analysis = &report["analysis"];
    assert_eq!(analysis["status"], "available");
    assert_eq!(analysis["scope"], "sampled");
    assert_eq!(analysis["completeness"], "partial");
    assert_eq!(analysis["source_size_bytes"], bytes.len() as u64);
    assert_eq!(analysis["records_analyzed"], 10_000);
    assert!(analysis["bytes_analyzed"].as_u64().expect("bytes analyzed") < bytes.len() as u64);
    assert_planner(
        &analysis["planner"],
        "csv_columnar_dictionary",
        "HIGH_REPETITION_DETECTED",
        "planner_recommendation",
    );
    assert_evidence(
        &analysis["evidence"],
        "analysis_diagnostic",
        "SAMPLE_RECORD_LIMIT_REACHED",
        None,
    );

    assert_eq!(
        recommendation_codes(&report),
        vec![
            "STRUCTURED_COMPRESSION_RECOMMENDED",
            "ANALYSIS_LIMITED",
            "FULL_COMPARISON_RECOMMENDED",
        ]
    );
    assert_eq!(
        recommendation(&report, "STRUCTURED_COMPRESSION_RECOMMENDED")["category"],
        "compression_strategy"
    );
    assert_eq!(
        recommendation(&report, "ANALYSIS_LIMITED")["category"],
        "analysis_limitation"
    );
    assert_eq!(
        recommendation(&report, "FULL_COMPARISON_RECOMMENDED")["category"],
        "next_action"
    );
    assert_evidence(
        &recommendation(&report, "ANALYSIS_LIMITED")["evidence"],
        "analysis_diagnostic",
        "SAMPLE_RECORD_LIMIT_REACHED",
        None,
    );
    assert_evidence(
        &recommendation(&report, "FULL_COMPARISON_RECOMMENDED")["evidence"],
        "analysis_diagnostic",
        "SAMPLE_RECORD_LIMIT_REACHED",
        None,
    );
    assert!(!recommendation_codes(&report).contains(&"RAW_ZSTD_RECOMMENDED"));
}

#[test]
fn hard_header_limit_uses_safety_fallback_without_planner_attribution() {
    let directory = tempfile::tempdir().expect("create header-limit Advisor directory");
    let input = directory.path().join("oversized-header.csv");
    let mut bytes = Vec::with_capacity(MIB + 16);
    for _ in 0..(MIB / 2) {
        bytes.extend_from_slice(b"a,");
    }
    bytes.extend_from_slice(b"a\n1,2\n");
    fs::write(&input, &bytes).expect("write oversized Advisor header");

    let output = run_advisor(&input, &["--sample-mb", "1", "--json"]);
    assert_success(&output);
    let report = parse_report(&output);
    let analysis = &report["analysis"];
    assert_eq!(analysis["status"], "available");
    assert_eq!(analysis["scope"], "sampled");
    assert_eq!(analysis["completeness"], "partial");
    assert_eq!(analysis["source_size_bytes"], bytes.len() as u64);
    assert_eq!(analysis["bytes_analyzed"], 0);
    assert_eq!(analysis["records_analyzed"], 0);
    assert_planner(
        &analysis["planner"],
        "raw_zstd",
        "ANALYSIS_LIMITED_RAW_ZSTD_FALLBACK",
        "safe_fallback",
    );
    assert_evidence(
        &analysis["evidence"],
        "safety_policy",
        "ANALYSIS_LIMITED_RAW_ZSTD_FALLBACK",
        None,
    );
    assert_evidence(
        &analysis["evidence"],
        "analysis_diagnostic",
        "HEADER_BYTE_LIMIT_REACHED",
        None,
    );
    assert_eq!(
        recommendation_codes(&report),
        vec![
            "RAW_ZSTD_RECOMMENDED",
            "ANALYSIS_LIMITED",
            "FULL_COMPARISON_RECOMMENDED",
        ]
    );
    let raw = recommendation(&report, "RAW_ZSTD_RECOMMENDED");
    assert_evidence(
        &raw["evidence"],
        "safety_policy",
        "ANALYSIS_LIMITED_RAW_ZSTD_FALLBACK",
        None,
    );
    assert!(
        !raw["message"]
            .as_str()
            .expect("RawZstd recommendation message")
            .contains("PlannerPolicyV1"),
        "safe fallback must not be attributed to PlannerPolicyV1"
    );
    for code in ["ANALYSIS_LIMITED", "FULL_COMPARISON_RECOMMENDED"] {
        assert_evidence(
            &recommendation(&report, code)["evidence"],
            "analysis_diagnostic",
            "HEADER_BYTE_LIMIT_REACHED",
            None,
        );
    }

    let text = run_advisor(&input, &["--sample-mb", "1"]);
    assert_success(&text);
    let text = String::from_utf8(text.stdout).expect("hard-limit Advisor text is UTF-8");
    assert!(text.contains("Selection scope: safe_fallback"));
    assert!(text.contains("Archive selection: raw_zstd"));
    assert!(!text.contains("Planner selection:"));
}

#[test]
fn high_cardinality_advice_preserves_lower_bound_semantics() {
    let directory = tempfile::tempdir().expect("create high-cardinality Advisor directory");
    let input = directory.path().join("high-cardinality.csv");
    let mut corpus = String::from("identifier_a,identifier_b,identifier_c,identifier_d\n");
    for index in 0..8_193 {
        corpus.push_str(&format!("a{index},b{index},c{index},d{index}\n"));
    }
    fs::write(&input, corpus.as_bytes()).expect("write high-cardinality Advisor corpus");

    let output = run_advisor(&input, &["--sample-mb", "1", "--json"]);
    assert_success(&output);
    let report = parse_report(&output);
    let analysis = &report["analysis"];
    assert_eq!(analysis["status"], "available");
    assert_eq!(analysis["scope"], "full");
    assert_eq!(analysis["completeness"], "complete");
    assert_planner(
        &analysis["planner"],
        "raw_zstd",
        "INSUFFICIENT_REPETITION_MAJORITY",
        "planner_recommendation",
    );

    let columns = analysis["high_cardinality_columns"]
        .as_array()
        .expect("high-cardinality columns");
    assert_eq!(columns.len(), 4);
    for (index, column) in columns.iter().enumerate() {
        assert_eq!(object_keys(column), keys(&["cardinality", "column_index"]));
        assert_eq!(column["column_index"], index);
        assert_eq!(
            object_keys(&column["cardinality"]),
            keys(&["kind", "value"])
        );
        assert_eq!(column["cardinality"]["kind"], "at_least");
        assert_eq!(column["cardinality"]["value"], 8_193);
    }
    for index in 0..4 {
        assert_evidence(
            &analysis["evidence"],
            "analysis_diagnostic",
            "CARDINALITY_LIMIT_REACHED",
            Some(index),
        );
        assert_evidence(
            &analysis["evidence"],
            "column_policy",
            "CARDINALITY_THRESHOLD_EXCEEDED",
            Some(index),
        );
    }
    assert_eq!(
        recommendation_codes(&report),
        vec!["RAW_ZSTD_RECOMMENDED", "HIGH_CARDINALITY_OBSERVED"]
    );
    let high = recommendation(&report, "HIGH_CARDINALITY_OBSERVED");
    assert_eq!(high["category"], "dataset_observation");
    for index in 0..4 {
        assert_evidence(
            &high["evidence"],
            "analysis_diagnostic",
            "CARDINALITY_LIMIT_REACHED",
            Some(index),
        );
        assert_evidence(
            &high["evidence"],
            "column_policy",
            "CARDINALITY_THRESHOLD_EXCEEDED",
            Some(index),
        );
    }
    assert!(!recommendation_codes(&report).contains(&"FULL_COMPARISON_RECOMMENDED"));
}

#[test]
fn advisor_reuses_shared_alternate_delimiter_analysis() {
    let directory = tempfile::tempdir().expect("create PSV Advisor directory");
    let input = directory.path().join("repetitive.psv");
    let mut corpus = Vec::from(&b"group|note|status\n"[..]);
    for _ in 0..32 {
        corpus.extend_from_slice(b"A|same|active\n");
    }
    fs::write(&input, &corpus).expect("write PSV Advisor corpus");

    let output = run_advisor(&input, &["--json"]);
    assert_success(&output);
    let report = parse_report(&output);
    assert_eq!(report["analysis"]["status"], "available");
    assert_eq!(report["analysis"]["scope"], "full");
    assert_eq!(report["analysis"]["completeness"], "complete");
    assert_planner(
        &report["analysis"]["planner"],
        "csv_columnar_dictionary",
        "HIGH_REPETITION_DETECTED",
        "planner_recommendation",
    );
    assert_eq!(
        recommendation_codes(&report),
        vec!["STRUCTURED_COMPRESSION_RECOMMENDED"]
    );
}

#[test]
fn unsupported_input_is_truthfully_unavailable_and_still_archivable() {
    let directory = tempfile::tempdir().expect("create unavailable Advisor directory");
    let input = directory
        .path()
        .join("PRIVATE_UNSTRUCTURED_FILENAME_5D61.data");
    let bytes = b"PRIVATE_UNSTRUCTURED_VALUE_6E72\nplain\n";
    fs::write(&input, bytes).expect("write unavailable Advisor corpus");

    let first = run_advisor(&input, &["--json"]);
    assert_success(&first);
    let second = run_advisor(&input, &["--json"]);
    assert_success(&second);
    assert_eq!(first.stdout, second.stdout);

    let report = parse_report(&first);
    assert_report_root(&report);
    let analysis = &report["analysis"];
    assert_analysis_shape(analysis);
    assert_eq!(analysis["status"], "unavailable");
    assert_eq!(analysis["scope"], "unavailable");
    assert_eq!(analysis["completeness"], "unavailable");
    assert_eq!(analysis["source_size_bytes"], bytes.len() as u64);
    assert!(analysis["bytes_analyzed"].is_null());
    assert!(analysis["records_analyzed"].is_null());
    assert!(analysis["planner"].is_null());
    assert_eq!(
        evidence_items(&analysis["evidence"]).len(),
        1,
        "unavailable analysis must expose one typed status fact"
    );
    assert_evidence(
        &analysis["evidence"],
        "analysis_status",
        "STRUCTURED_ANALYSIS_UNAVAILABLE",
        None,
    );
    assert!(analysis["high_cardinality_columns"]
        .as_array()
        .expect("unavailable high-cardinality array")
        .is_empty());
    assert_eq!(
        recommendation_codes(&report),
        vec!["RAW_ZSTD_RECOMMENDED", "FULL_COMPARISON_RECOMMENDED"]
    );
    assert_eq!(
        recommendation(&report, "RAW_ZSTD_RECOMMENDED")["category"],
        "compression_strategy"
    );
    assert_eq!(
        recommendation(&report, "FULL_COMPARISON_RECOMMENDED")["category"],
        "next_action"
    );
    for code in ["RAW_ZSTD_RECOMMENDED", "FULL_COMPARISON_RECOMMENDED"] {
        assert_evidence(
            &recommendation(&report, code)["evidence"],
            "analysis_status",
            "STRUCTURED_ANALYSIS_UNAVAILABLE",
            None,
        );
    }
    let rendered = combined_output(&first);
    assert!(!rendered.contains("PRIVATE_UNSTRUCTURED_FILENAME_5D61"));
    assert!(!rendered.contains("PRIVATE_UNSTRUCTURED_VALUE_6E72"));
}

#[test]
fn advisor_operational_and_limit_errors_leave_stdout_empty() {
    let directory = tempfile::tempdir().expect("create Advisor error directory");
    let missing = directory.path().join("missing.csv");
    let missing_output = run_advisor(&missing, &["--json"]);
    assert_command_error(&missing_output);

    let input = directory.path().join("valid.csv");
    fs::write(&input, b"left,right\nA,B\n").expect("write valid Advisor error corpus");
    let invalid_limit = run_advisor(&input, &["--sample-mb", "0", "--json"]);
    assert_command_error(&invalid_limit);
    assert!(String::from_utf8_lossy(&invalid_limit.stderr)
        .contains("--sample-mb must be between 1 and 2048"));
}

fn run_advisor(input: &Path, arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_datapack"))
        .arg("advisor")
        .arg(input)
        .args(arguments)
        .output()
        .expect("run datapack advisor")
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "Advisor unexpectedly failed: {}",
        combined_output(output)
    );
}

fn assert_command_error(output: &Output) {
    assert_eq!(
        output.status.code(),
        Some(1),
        "Advisor command error had unexpected status: {}",
        combined_output(output)
    );
    assert!(output.stdout.is_empty(), "Advisor error emitted stdout");
    assert!(!output.stderr.is_empty(), "Advisor error omitted stderr");
}

fn combined_output(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn parse_report(output: &Output) -> Value {
    let report = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "stdout was not AdvisorReportV1 JSON: {error}; output: {}",
            combined_output(output)
        )
    });
    assert_recommendation_evidence_is_reported(&report);
    report
}

fn assert_recommendation_evidence_is_reported(report: &Value) {
    let facts = evidence_items(&report["analysis"]["evidence"]);
    for recommendation in report["recommendations"]
        .as_array()
        .expect("recommendations array")
    {
        for evidence in evidence_items(&recommendation["evidence"]) {
            assert!(
                facts.contains(evidence),
                "recommendation evidence {evidence} was absent from analysis.evidence"
            );
        }
    }
}

fn assert_report_root(report: &Value) {
    assert_eq!(
        object_keys(report),
        keys(&[
            "analysis",
            "policy",
            "recommendations",
            "report_type",
            "schema_version",
        ])
    );
    assert_eq!(report["schema_version"], 1);
    assert_eq!(report["report_type"], "advisor");
    assert_eq!(object_keys(&report["policy"]), keys(&["name", "version"]));
}

fn assert_analysis_shape(analysis: &Value) {
    assert_eq!(
        object_keys(analysis),
        keys(&[
            "bytes_analyzed",
            "completeness",
            "evidence",
            "high_cardinality_columns",
            "planner",
            "records_analyzed",
            "scope",
            "source_size_bytes",
            "status",
        ])
    );
}

fn assert_planner(planner: &Value, selected_mode: &str, reason_code: &str, selection_scope: &str) {
    assert_eq!(
        object_keys(planner),
        keys(&[
            "policy",
            "reason_code",
            "selected_archive_mode",
            "selection_scope",
        ])
    );
    assert_eq!(object_keys(&planner["policy"]), keys(&["name", "version"]));
    assert_eq!(planner["policy"]["name"], "PlannerPolicyV1");
    assert_eq!(planner["policy"]["version"], 1);
    assert_eq!(planner["selected_archive_mode"], selected_mode);
    assert_eq!(planner["reason_code"], reason_code);
    assert_eq!(planner["selection_scope"], selection_scope);
}

fn assert_recommendation_shape(recommendation: &Value) {
    assert_eq!(
        object_keys(recommendation),
        keys(&["category", "code", "evidence", "message"])
    );
    assert!(recommendation["code"].as_str().is_some());
    assert!(recommendation["category"].as_str().is_some());
    assert!(recommendation["message"].as_str().is_some());
    assert!(!evidence_items(&recommendation["evidence"]).is_empty());
}

fn recommendation_codes(report: &Value) -> Vec<&str> {
    report["recommendations"]
        .as_array()
        .expect("recommendations array")
        .iter()
        .map(|recommendation| {
            assert_recommendation_shape(recommendation);
            recommendation["code"]
                .as_str()
                .expect("recommendation code string")
        })
        .collect()
}

fn recommendation<'a>(report: &'a Value, code: &str) -> &'a Value {
    report["recommendations"]
        .as_array()
        .expect("recommendations array")
        .iter()
        .find(|recommendation| recommendation["code"] == code)
        .unwrap_or_else(|| panic!("missing recommendation {code}"))
}

fn evidence_items(value: &Value) -> &[Value] {
    let items = value.as_array().expect("evidence array");
    for evidence in items {
        assert_eq!(
            object_keys(evidence),
            keys(&["code", "column_index", "source"])
        );
        let source = evidence["source"].as_str().expect("evidence source string");
        assert!(
            matches!(
                source,
                "analysis_status"
                    | "analysis_diagnostic"
                    | "planner_policy"
                    | "column_policy"
                    | "safety_policy"
                    | "compatibility_policy"
            ),
            "unknown evidence source {source}"
        );
        assert!(evidence["code"].as_str().is_some());
        if let Some(column_index) = evidence["column_index"].as_u64() {
            assert!(matches!(
                (source, evidence["code"].as_str()),
                ("analysis_diagnostic", Some("CARDINALITY_LIMIT_REACHED"))
                    | ("column_policy", Some("CARDINALITY_THRESHOLD_EXCEEDED"))
            ));
            usize::try_from(column_index).expect("column index fits usize");
        } else {
            assert!(evidence["column_index"].is_null());
        }
    }
    items
}

fn assert_evidence(value: &Value, source: &str, code: &str, column_index: Option<usize>) {
    assert!(
        evidence_items(value).iter().any(|evidence| {
            evidence["source"] == source
                && evidence["code"] == code
                && match column_index {
                    Some(index) => evidence["column_index"] == index,
                    None => evidence["column_index"].is_null(),
                }
        }),
        "missing evidence source={source} code={code} column_index={column_index:?}"
    );
}

fn assert_no_forbidden_vocabulary(report: &Value, rendered: &str) {
    fn visit(value: &Value) {
        match value {
            Value::Object(object) => {
                for (key, value) in object {
                    assert!(
                        !matches!(
                            key.to_ascii_lowercase().as_str(),
                            "ai" | "llm"
                                | "score"
                                | "scoring"
                                | "confidence"
                                | "confidence_percent"
                                | "overall_score"
                        ),
                        "forbidden Advisor JSON key: {key}"
                    );
                    visit(value);
                }
            }
            Value::Array(array) => array.iter().for_each(visit),
            _ => {}
        }
    }
    visit(report);

    let words = rendered
        .split(|character: char| !character.is_ascii_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_ascii_lowercase)
        .collect::<BTreeSet<_>>();
    for forbidden in ["ai", "llm", "score", "scoring", "confidence"] {
        assert!(
            !words.contains(forbidden),
            "Advisor output used forbidden term {forbidden}"
        );
    }
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
