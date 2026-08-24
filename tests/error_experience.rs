use std::fs;
use std::path::Path;
use std::process::{Command, Output};

fn datapack(arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_datapack"))
        .args(arguments)
        .output()
        .expect("run datapack CLI")
}

fn path_text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[test]
fn runtime_failures_have_stable_identity_context_and_clean_streams() {
    let directory = tempfile::tempdir().expect("CLI error-experience directory");
    let missing = directory.path().join("missing.csv");
    let archive = directory.path().join("missing.dpack");
    let missing_text = path_text(&missing);
    let archive_text = path_text(&archive);
    let output = datapack(&["compress", &missing_text, &archive_text]);

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).expect("runtime error is UTF-8");
    assert!(stderr.starts_with("error[invalid_format]:"));
    assert!(stderr.contains("input path"));
    assert!(stderr.contains(&missing_text));
    assert!(!stderr.contains("panicked at"));
    assert!(!stderr.contains("stack backtrace"));

    let source = directory.path().join("source.csv");
    fs::write(&source, b"id,value\n1,a\n2,b\n").expect("write source");
    fs::write(&archive, b"existing destination").expect("write protected destination");
    let source_text = path_text(&source);
    let output = datapack(&["compress", &source_text, &archive_text]);

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).expect("overwrite error is UTF-8");
    assert!(stderr.starts_with("error[invalid_format]:"));
    assert!(stderr.contains(&archive_text));
    assert!(stderr.contains("already exists"));
    assert!(stderr.contains("--force"));
    assert_eq!(
        fs::read(&archive).expect("read protected destination"),
        b"existing destination"
    );
}

#[test]
fn parse_failures_remain_exit_two_and_do_not_run_the_operation() {
    let directory = tempfile::tempdir().expect("CLI parse-error directory");
    let missing = directory.path().join("missing.csv");
    let missing_text = path_text(&missing);
    let output = datapack(&["compare", &missing_text, "--mode", "unsupported"]);

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).expect("Clap error is UTF-8");
    assert!(stderr.contains("invalid value"));
    assert!(stderr.contains("--mode"));
    assert!(!stderr.contains("not found"));
    assert!(!stderr.contains("panicked at"));
}

#[test]
fn successful_machine_report_uses_stdout_without_error_noise() {
    let directory = tempfile::tempdir().expect("CLI success directory");
    let source = directory.path().join("source.csv");
    fs::write(&source, b"id,value\n1,a\n2,a\n").expect("write source");
    let source_text = path_text(&source);
    let output = datapack(&["analyze", &source_text, "--json"]);

    assert_eq!(output.status.code(), Some(0));
    assert!(output.stderr.is_empty());
    let report: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout is the JSON report");
    assert_eq!(report["report_type"], "analysis");
}
