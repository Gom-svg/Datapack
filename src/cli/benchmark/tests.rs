use super::super::BenchmarkOptions;
use super::model::{benchmark_partial_reasons, benchmark_scope, validation_status};
use super::report::json_escape;
use super::temp::BenchmarkTempFiles;

fn benchmark_options() -> BenchmarkOptions {
    BenchmarkOptions {
        json: false,
        keep_temp: false,
        quick: true,
        runs: 1,
        profile: false,
        chunked: false,
        chunk_size_mb: None,
        threads: None,
        max_in_flight_chunks: None,
        backend: None,
        adaptive_level: false,
        no_zstd_baseline: false,
        no_roundtrip: false,
        no_hash: false,
        estimate_only: false,
        max_input_mb: None,
    }
}

#[test]
fn benchmark_scope_uses_canonical_values() {
    let options = benchmark_options();
    assert_eq!(benchmark_scope(&options, false), "full");
    assert_eq!(benchmark_scope(&options, true), "sampled");

    let mut partial = options;
    partial.no_zstd_baseline = true;
    assert_eq!(benchmark_scope(&partial, false), "partial");
    assert_eq!(benchmark_scope(&partial, true), "sampled");
}

#[test]
fn benchmark_partial_reasons_are_unique() {
    let mut options = benchmark_options();
    options.no_zstd_baseline = true;
    options.no_roundtrip = true;
    options.no_hash = true;
    let reasons = benchmark_partial_reasons(&options, true);
    let mut unique = reasons.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(reasons.len(), unique.len());
}

#[test]
fn benchmark_validation_status_is_canonical_and_mismatch_is_error() {
    assert_eq!(
        validation_status(true, false, Some("same"), Some("same")).unwrap(),
        "validated"
    );
    assert_eq!(
        validation_status(true, true, Some("same"), Some("same")).unwrap(),
        "partially_validated"
    );
    assert_eq!(
        validation_status(false, false, None, None).unwrap(),
        "not_validated"
    );
    let error = validation_status(true, false, Some("restored"), Some("original"))
        .expect_err("a SHA256 mismatch must fail the benchmark");
    assert!(error.to_string().contains("SHA256 validation failed"));
}

#[test]
fn json_escape_covers_control_characters() {
    assert_eq!(
        json_escape("quote=\" slash=\\ line=\n tab=\t \u{0001}"),
        "quote=\\\" slash=\\\\ line=\\n tab=\\t \\u0001"
    );
}

#[test]
fn benchmark_temp_guard_removes_files_on_drop() {
    let directory = tempfile::tempdir().unwrap();
    let first = directory.path().join("first.tmp");
    let second = directory.path().join("second.tmp");
    std::fs::write(&first, b"first").unwrap();
    std::fs::write(&second, b"second").unwrap();

    {
        let _guard = BenchmarkTempFiles::new(false, vec![first.clone(), second.clone()]);
    }

    assert!(!first.exists());
    assert!(!second.exists());
}

#[test]
fn benchmark_temp_guard_honors_keep_temp() {
    let directory = tempfile::tempdir().unwrap();
    let artifact = directory.path().join("kept.tmp");
    std::fs::write(&artifact, b"kept").unwrap();

    {
        let _guard = BenchmarkTempFiles::new(true, vec![artifact.clone()]);
    }

    assert!(artifact.exists());
}
