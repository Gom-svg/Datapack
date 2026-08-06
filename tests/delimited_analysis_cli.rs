use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use datapack::metadata::PayloadKind;
use datapack::storage;
use serde_json::Value;

const MIB: usize = 1024 * 1024;

#[derive(Clone, Copy)]
struct DialectCase {
    name: &'static str,
    delimiter: char,
    format: &'static str,
    record_model: &'static str,
}

const DIALECTS: [DialectCase; 4] = [
    DialectCase {
        name: "csv",
        delimiter: ',',
        format: "csv",
        record_model: "physical_line",
    },
    DialectCase {
        name: "tsv",
        delimiter: '\t',
        format: "tsv",
        record_model: "logical_record",
    },
    DialectCase {
        name: "psv",
        delimiter: '|',
        format: "psv",
        record_model: "logical_record",
    },
    DialectCase {
        name: "semicolon",
        delimiter: ';',
        format: "semicolon_delimited",
        record_model: "logical_record",
    },
];

#[test]
fn json_v1_reports_all_supported_dialects_consistently_and_privately() {
    let mut comma_columns = None;

    for dialect in DIALECTS {
        let input = format!(
            "PRIVATE_HEADER_A1{}PRIVATE_HEADER_B2\nPRIVATE_VALUE_C3{}7\nPRIVATE_VALUE_D4{}7\n",
            dialect.delimiter, dialect.delimiter, dialect.delimiter
        );
        let file_name = format!("PRIVATE_FILENAME_6C18.{}", dialect.name);
        let output = run_analyze(&file_name, input.as_bytes(), &["--json"]);
        assert_success(&output);
        assert!(output.stderr.is_empty(), "{} emitted stderr", dialect.name);

        let report = parse_report(&output);
        assert_eq!(report["schema_version"], 1, "{} schema", dialect.name);
        assert_eq!(report["report_type"], "analysis", "{} type", dialect.name);
        assert_eq!(
            report["dataset"]["source_size_bytes"],
            input.len() as u64,
            "{} source size",
            dialect.name
        );
        assert_eq!(
            report["dataset"]["column_count"], 2,
            "{} width",
            dialect.name
        );

        let parser = &report["dataset"]["parser"];
        assert_eq!(parser["format"], dialect.format, "{} format", dialect.name);
        assert_eq!(
            parser["delimiter"],
            dialect.delimiter.to_string(),
            "{} delimiter",
            dialect.name
        );
        assert_eq!(
            parser["record_model"], dialect.record_model,
            "{} record model",
            dialect.name
        );
        assert_eq!(parser["header_mode"], "first_record");

        let columns = report["dataset"]["columns"]
            .as_array()
            .expect("columns array");
        assert_eq!(columns.len(), 2, "{} column reports", dialect.name);
        assert_eq!(columns[0]["observed_values"], 2);
        assert_eq!(columns[0]["cardinality"]["kind"], "exact");
        assert_eq!(columns[0]["cardinality"]["value"], 2);
        assert_eq!(columns[1]["cardinality"]["value"], 1);

        let sampling = &report["sampling"];
        assert_eq!(sampling["scope"], "full");
        assert_eq!(sampling["completeness"], "complete");
        assert_eq!(sampling["limited"], false);
        assert!(sampling["limit_reached"].is_null());
        assert_eq!(sampling["records_analyzed"], 2);
        assert_eq!(sampling["bytes_read"], input.len() as u64);
        assert_eq!(sampling["bytes_analyzed"], input.len() as u64);
        assert_eq!(sampling["final_newline"], true);
        assert!(report["diagnostics"]
            .as_array()
            .expect("diagnostics array")
            .is_empty());

        if dialect.delimiter == ',' {
            assert_eq!(
                report["planner"]["selection_scope"],
                "planner_recommendation"
            );
            comma_columns = Some(report["dataset"]["columns"].clone());
        } else {
            assert_eq!(report["planner"]["selection_scope"], "format_fallback");
            assert_eq!(report["planner"]["selected_archive_mode"], "raw_zstd");
            assert_eq!(
                report["planner"]["reason"]["code"],
                "STRUCTURED_COMPRESSION_NOT_ENABLED_FOR_DIALECT"
            );
            assert_eq!(
                &report["dataset"]["columns"],
                comma_columns.as_ref().expect("comma facts recorded"),
                "{} facts diverged from comma",
                dialect.name
            );
        }

        let raw_output = String::from_utf8_lossy(&output.stdout);
        for secret in [
            "PRIVATE_FILENAME_6C18",
            "PRIVATE_HEADER_A1",
            "PRIVATE_HEADER_B2",
            "PRIVATE_VALUE_C3",
            "PRIVATE_VALUE_D4",
        ] {
            assert!(
                !raw_output.contains(secret),
                "{} JSON disclosed {secret}",
                dialect.name
            );
        }
        if dialect.delimiter == '\t' {
            assert!(raw_output.contains("\"delimiter\":\"\\t\""));
        }
    }
}

#[test]
fn bounded_multi_record_detection_resolves_header_only_decoys() {
    let cases: [(&str, &[u8], &str, &str); 4] = [
        (
            "comma-with-pipe-decoy.csv",
            b"id,note|tag\n1,foo\n2,bar\n",
            "csv",
            ",",
        ),
        (
            "tab-with-comma-decoy.tsv",
            b"id\tnote,tag\n1\tfoo\n2\tbar\n",
            "tsv",
            "\t",
        ),
        (
            "pipe-with-semicolon-decoy.psv",
            b"id|note;tag\n1|foo\n2|bar\n",
            "psv",
            "|",
        ),
        (
            "semicolon-with-pipe-decoy.data",
            b"id;note|tag\n1;foo\n2;bar\n",
            "semicolon_delimited",
            ";",
        ),
    ];

    for (name, input, format, delimiter) in cases {
        let output = run_analyze(name, input, &["--json"]);
        assert_success(&output);
        let report = parse_report(&output);
        assert_eq!(report["dataset"]["parser"]["format"], format, "{name}");
        assert_eq!(
            report["dataset"]["parser"]["delimiter"], delimiter,
            "{name}"
        );
        assert_eq!(report["sampling"]["records_analyzed"], 2, "{name}");
    }
}

#[test]
fn logical_analysis_handles_quotes_multiline_lf_and_escaped_quotes() {
    let input = b"id|note\n1|\"left|right\nup|down\"\n2|\"a \"\"quoted\"\" value\"\n";
    let output = run_analyze("logical.psv", input, &["--json"]);
    assert_success(&output);
    let report = parse_report(&output);

    assert_eq!(report["dataset"]["parser"]["format"], "psv");
    assert_eq!(report["dataset"]["parser"]["delimiter"], "|");
    assert_eq!(
        report["dataset"]["parser"]["record_model"],
        "logical_record"
    );
    assert_eq!(report["dataset"]["column_count"], 2);
    assert_eq!(report["sampling"]["records_analyzed"], 2);
    assert_eq!(report["sampling"]["bytes_read"], input.len() as u64);
    assert_eq!(report["sampling"]["bytes_analyzed"], input.len() as u64);
    assert_eq!(report["sampling"]["final_newline"], true);
    assert_eq!(report["dataset"]["columns"][1]["observed_values"], 2);
    assert_eq!(report["dataset"]["columns"][1]["cardinality"]["value"], 2);

    let raw_output = String::from_utf8_lossy(&output.stdout);
    assert!(!raw_output.contains("left|right"));
    assert!(!raw_output.contains("quoted value"));
}

#[test]
fn logical_analysis_handles_crlf_utf8_empty_fields_and_no_final_newline() {
    let input = "id\tcity\tnote\r\n1\t東京\t\r\n2\t\tvacío";
    let output = run_analyze("unicode.tsv", input.as_bytes(), &["--json"]);
    assert_success(&output);
    let report = parse_report(&output);

    assert_eq!(report["dataset"]["parser"]["format"], "tsv");
    assert_eq!(report["dataset"]["parser"]["delimiter"], "\t");
    assert_eq!(report["sampling"]["records_analyzed"], 2);
    assert_eq!(report["sampling"]["scope"], "full");
    assert_eq!(report["sampling"]["completeness"], "complete");
    assert_eq!(report["sampling"]["final_newline"], false);
    assert_eq!(report["dataset"]["columns"][1]["empty_values"], 1);
    assert_eq!(
        report["dataset"]["columns"][1]["value_length_bytes"]["maximum"],
        6
    );
    assert_eq!(
        report["dataset"]["columns"][1]["value_length_bytes"]["total"],
        6
    );
    assert_eq!(report["dataset"]["columns"][2]["empty_values"], 1);
    assert_eq!(
        report["dataset"]["columns"][2]["value_length_bytes"]["maximum"],
        6
    );

    let raw_output = String::from_utf8_lossy(&output.stdout);
    assert!(!raw_output.contains("東京"));
    assert!(!raw_output.contains("vacío"));
}

#[test]
fn alternate_analysis_preserves_first_record_header_policy_without_inference() {
    let output = run_analyze("headerless.psv", b"1|Ada\n2|Grace\n3|Linus\n", &["--json"]);
    assert_success(&output);
    let report = parse_report(&output);

    assert_eq!(report["dataset"]["parser"]["header_mode"], "first_record");
    assert_eq!(report["sampling"]["records_analyzed"], 2);
    assert_eq!(report["dataset"]["columns"][0]["observed_values"], 2);
    assert_eq!(report["dataset"]["columns"][0]["numeric_values"], 2);
    assert_eq!(report["dataset"]["columns"][0]["cardinality"]["value"], 2);
}

#[test]
fn alternate_text_identifies_the_dialect_and_escapes_control_headers() {
    let input = b"id\t\"line\tbreak\"\n1\tvalue\n";
    let output = run_analyze("control.tsv", input, &[]);
    assert_success(&output);
    assert!(output.stderr.is_empty());

    let stdout = String::from_utf8(output.stdout).expect("text analysis is UTF-8");
    assert!(stdout.contains("Detected dialect:  TSV (tab)\n"));
    assert!(stdout.contains("line\\tbreak"));
    assert!(!stdout.contains("line\tbreak"));
}

#[test]
fn ambiguity_and_unsupported_input_are_deterministic() {
    let cases: [(&str, &[u8], &str); 2] = [
        (
            "ambiguous.data",
            b"a,b|c\n1,2|3\n",
            "error: file is not valid CSV: ambiguous delimiter; candidates: comma (,), pipe (|)\n",
        ),
        (
            "unsupported.data",
            b"alpha\nbeta\n",
            "error: file is not valid CSV: no supported structured delimiter was detected; input is unsupported or unstructured\n",
        ),
    ];

    for (name, input, expected_stderr) in cases {
        let first = run_analyze(name, input, &["--json"]);
        let second = run_analyze(name, input, &["--json"]);
        assert_eq!(first.status.code(), Some(2), "{name} exit status");
        assert_eq!(second.status.code(), Some(2), "{name} repeated exit status");
        assert!(first.stdout.is_empty(), "{name} emitted JSON on failure");
        assert_eq!(first.stdout, second.stdout, "{name} stdout changed");
        assert_eq!(first.stderr, expected_stderr.as_bytes(), "{name} stderr");
        assert_eq!(first.stderr, second.stderr, "{name} stderr changed");
        assert!(!String::from_utf8_lossy(&first.stderr).contains("1,2|3"));
    }
}

#[test]
fn inconsistent_width_is_rejected_for_every_alternate_delimiter() {
    for dialect in DIALECTS.into_iter().filter(|case| case.delimiter != ',') {
        let input = format!("a{0}b\n1{0}2\n3{0}4{0}5\n", dialect.delimiter);
        let output = run_analyze(dialect.name, input.as_bytes(), &["--json"]);
        assert_eq!(
            output.status.code(),
            Some(2),
            "{} exit status",
            dialect.name
        );
        assert!(output.stdout.is_empty(), "{} emitted JSON", dialect.name);
        assert_eq!(
            output.stderr, b"error: file is not valid CSV: row has 3 columns, expected 2\n",
            "{} width error",
            dialect.name
        );
    }
}

#[test]
fn alternate_sampling_and_column_limits_are_reported_truthfully() {
    let mut sampled = Vec::from(b"left|right\n1|".as_slice());
    sampled.extend(std::iter::repeat_n(b'x', MIB));
    sampled.push(b'\n');
    let sampled_output = run_analyze(
        "sample-limited.psv",
        &sampled,
        &["--json", "--sample-mb", "1"],
    );
    assert_success(&sampled_output);
    let sampled_report = parse_report(&sampled_output);
    assert_eq!(sampled_report["dataset"]["parser"]["delimiter"], "|");
    assert_eq!(sampled_report["dataset"]["column_count"], 2);
    assert_eq!(sampled_report["sampling"]["bytes_read"], MIB as u64);
    assert_eq!(sampled_report["sampling"]["bytes_analyzed"], 11);
    assert_eq!(sampled_report["sampling"]["records_analyzed"], 0);
    assert_eq!(sampled_report["sampling"]["scope"], "sampled");
    assert_eq!(sampled_report["sampling"]["completeness"], "partial");
    assert_eq!(sampled_report["sampling"]["limited"], true);
    assert_eq!(sampled_report["sampling"]["limit_reached"], "byte_limit");
    assert!(sampled_report["sampling"]["final_newline"].is_null());
    assert_eq!(
        sampled_report["diagnostics"][0]["code"],
        "SAMPLE_BYTE_LIMIT_REACHED"
    );

    let wide_header = std::iter::repeat_n("c", 4_097)
        .collect::<Vec<_>>()
        .join("|");
    let wide = format!("{wide_header}\n");
    let wide_output = run_analyze("wide.psv", wide.as_bytes(), &["--json"]);
    assert_success(&wide_output);
    let wide_report = parse_report(&wide_output);
    assert_eq!(wide_report["dataset"]["parser"]["delimiter"], "|");
    assert!(wide_report["dataset"]["column_count"].is_null());
    assert!(wide_report["dataset"]["columns"]
        .as_array()
        .expect("wide columns array")
        .is_empty());
    assert_eq!(wide_report["sampling"]["scope"], "full");
    assert_eq!(wide_report["sampling"]["completeness"], "partial");
    assert_eq!(wide_report["sampling"]["limit_reached"], "column_limit");
    assert_eq!(wide_report["planner"]["selection_scope"], "safe_fallback");
    assert_eq!(wide_report["planner"]["selected_archive_mode"], "raw_zstd");
    assert!(wide_report["diagnostics"]
        .as_array()
        .expect("wide diagnostics")
        .iter()
        .any(|diagnostic| diagnostic["code"] == "COLUMN_LIMIT_REACHED"));
}

#[test]
fn alternate_header_detection_never_exceeds_or_disguises_its_bound() {
    let mut unquoted = Vec::from(b"left|".as_slice());
    unquoted.extend(std::iter::repeat_n(b'x', MIB));
    unquoted.extend_from_slice(b"\n1|2\n");
    let bounded = run_analyze("oversized-header.psv", &unquoted, &["--json"]);
    assert_success(&bounded);
    let report = parse_report(&bounded);
    assert_eq!(report["dataset"]["parser"]["format"], "psv");
    assert!(report["dataset"]["column_count"].is_null());
    assert_eq!(report["sampling"]["limit_reached"], "header_byte_limit");
    assert_eq!(
        report["diagnostics"][0]["message"],
        "The logical header record exceeded the header byte limit."
    );

    let mut quoted = Vec::from(b"left|\"open".as_slice());
    quoted.extend(std::iter::repeat_n(b'x', MIB));
    let limited = run_analyze("quoted-oversized-header.psv", &quoted, &["--json"]);
    assert_eq!(limited.status.code(), Some(2));
    assert!(limited.stdout.is_empty());
    assert_eq!(
        limited.stderr,
        b"error: file is not valid CSV: delimited dialect detection could not safely complete within configured limits\n"
    );
}

#[test]
fn alternate_delimiter_compression_stays_raw_and_byte_exact_in_phase_6() {
    let modes: [(&str, &[&str]); 3] = [
        ("default", &[]),
        ("best", &["--mode", "best"]),
        ("verify-best", &["--verify-best"]),
    ];

    for dialect in DIALECTS.into_iter().filter(|case| case.delimiter != ',') {
        let mut input = format!("left{0}middle{0}right,decoy\n", dialect.delimiter);
        for _ in 0..64 {
            input.push_str(&format!("fixed{0}same{0}value,tag\n", dialect.delimiter));
        }

        for (mode, arguments) in modes {
            let directory = tempfile::tempdir().expect("temporary compression directory");
            let input_path = directory.path().join(format!("input.{}", dialect.name));
            let archive_path = directory
                .path()
                .join(format!("{}-{mode}.dpack", dialect.name));
            fs::write(&input_path, input.as_bytes()).expect("write alternate compression input");

            let output = run_compress(&input_path, &archive_path, arguments);
            assert_success(&output);
            let archive_bytes = fs::read(&archive_path).expect("read alternate archive");
            let archive =
                storage::decode_archive(&archive_bytes).expect("decode alternate archive");
            assert_eq!(
                archive.metadata.payload_kind,
                PayloadKind::RawZstd,
                "{} {mode} enabled structured compression before Phase 7",
                dialect.name
            );
            assert_eq!(
                storage::restore_archive(&archive).expect("restore alternate archive"),
                input.as_bytes(),
                "{} {mode} restoration",
                dialect.name
            );
        }
    }
}

#[test]
fn alternate_delimiter_benchmark_cannot_execute_columnar_before_phase_7() {
    let directory = tempfile::tempdir().expect("temporary benchmark directory");
    let input_path = directory.path().join("ambiguous-to-legacy.tsv");
    let mut input = String::from("group\tstatus\tnote,decoy\n");
    for _ in 0..64 {
        input.push_str("A\tactive\tsame,tag\n");
    }
    fs::write(&input_path, input).expect("write benchmark eligibility corpus");

    let output = Command::new(env!("CARGO_BIN_EXE_datapack"))
        .arg("benchmark")
        .arg(&input_path)
        .args(["--quick", "--json"])
        .output()
        .expect("run alternate benchmark");
    assert_success(&output);
    let report: Value = serde_json::from_slice(&output.stdout).expect("parse benchmark JSON");
    assert_eq!(report["estimated_mode"], "RawZstd");
    assert_eq!(report["selected_mode"], "RawZstd");
    assert_eq!(report["roundtrip_sha256_match"], true);
}

fn run_analyze(name: &str, input: &[u8], arguments: &[&str]) -> Output {
    let directory = tempfile::tempdir().expect("temporary delimited analysis directory");
    let input_path = directory.path().join(name);
    fs::write(&input_path, input).expect("write delimited analysis input");
    Command::new(env!("CARGO_BIN_EXE_datapack"))
        .arg("analyze")
        .arg(input_path)
        .args(arguments)
        .output()
        .expect("run delimited analysis")
}

fn run_compress(input: &Path, output: &Path, arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_datapack"))
        .arg("compress")
        .arg(input)
        .arg(output)
        .args(arguments)
        .output()
        .expect("run alternate compression")
}

fn parse_report(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "parse analysis JSON: {error}\nstdout={}\nstderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "command failed with {:?}: stdout={} stderr={}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
