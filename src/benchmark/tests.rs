use std::path::PathBuf;

use crate::storage::chunked::ChunkedBackend;

use super::{
    benchmark_partial_reasons, benchmark_scope, execute, execute_with_events, validation_status,
    BenchmarkEvent, BenchmarkEventState, BenchmarkExecution, BenchmarkProgressReporter,
    BenchmarkRequest, BenchmarkTempFiles,
};

fn request() -> BenchmarkRequest {
    BenchmarkRequest {
        input: PathBuf::from("benchmark.csv"),
        keep_temp: false,
        quick: false,
        runs: 3,
        profile: false,
        chunked: false,
        chunk_size_mb: None,
        threads: None,
        max_in_flight_chunks: None,
        backend: None::<ChunkedBackend>,
        adaptive_level: false,
        no_zstd_baseline: false,
        no_roundtrip: false,
        no_hash: false,
        estimate_only: false,
        max_input_mb: None,
    }
}

#[test]
fn partial_reasons_preserve_an_embedded_semicolon_as_one_reason() {
    let mut request = request();
    request.no_roundtrip = true;

    assert_eq!(
        benchmark_partial_reasons(&request, false),
        vec!["round-trip decompression skipped; output identity not validated"]
    );
}

#[test]
fn benchmark_scope_uses_canonical_values() {
    let request = request();
    assert_eq!(benchmark_scope(&request, false), "full");
    assert_eq!(benchmark_scope(&request, true), "sampled");

    let mut partial = request;
    partial.no_zstd_baseline = true;
    assert_eq!(benchmark_scope(&partial, false), "partial");
    assert_eq!(benchmark_scope(&partial, true), "sampled");
}

#[test]
fn benchmark_partial_reasons_are_unique() {
    let mut request = request();
    request.no_zstd_baseline = true;
    request.no_roundtrip = true;
    request.no_hash = true;
    let reasons = benchmark_partial_reasons(&request, true);
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

#[test]
fn terminal_free_engine_returns_estimate_without_cli_options() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("input.csv");
    std::fs::write(&input, b"id,value\n1,alpha\n2,beta\n").unwrap();
    let mut request = request();
    request.input = input;
    request.estimate_only = true;

    let execution = execute(request).unwrap();
    let BenchmarkExecution::EstimateOnly {
        analysis,
        partial_reasons,
        ..
    } = execution
    else {
        panic!("estimate-only request must not run compression");
    };
    assert_eq!(analysis.facts.source_size_bytes, 24);
    assert_eq!(
        partial_reasons,
        vec!["estimate-only: compression, decompression, and hashing skipped"]
    );
}

#[test]
fn event_observer_is_separate_from_benchmark_result() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("input.csv");
    std::fs::write(&input, b"id,value\n1,alpha\n").unwrap();
    let mut request = request();
    request.input = input;
    request.estimate_only = true;
    let mut events = Vec::new();

    let execution = execute_with_events(request, &mut |event| events.push(event)).unwrap();

    assert!(matches!(execution, BenchmarkExecution::EstimateOnly { .. }));
    assert!(events.iter().any(|event| matches!(
        event,
        BenchmarkEvent::Phase {
            name: "planning",
            state: BenchmarkEventState::Completed,
            ..
        }
    )));
}

#[test]
fn reporter_distinguishes_advanced_from_completed_snapshots() {
    let mut events = Vec::new();
    {
        let mut observer = |event| events.push(event);
        let mut reporter =
            BenchmarkProgressReporter::new("benchmark test", Some(10), &mut observer);
        reporter.report(BenchmarkEventState::Advanced);
        reporter.finish();
    }

    assert!(matches!(
        events.as_slice(),
        [
            BenchmarkEvent::Phase {
                state: BenchmarkEventState::Advanced,
                ..
            },
            BenchmarkEvent::Phase {
                state: BenchmarkEventState::Completed,
                ..
            }
        ]
    ));
}
