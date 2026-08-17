#[path = "../benches/support/mod.rs"]
mod performance_support;

use std::path::Path;
use std::time::Duration;

use datapack::application::{self, CompareMode, CompareRequest};
use datapack::generation::{self, Profile};
use performance_support::{classify_path, median_duration, run_suite, scenarios, Preset};

#[test]
fn deterministic_generators_repeat_for_recorded_seeds() {
    let temporary = tempfile::tempdir().unwrap();
    for scenario in scenarios(Preset::Smoke) {
        let first = temporary
            .path()
            .join(format!("{}-first.csv", scenario.name));
        let second = temporary
            .path()
            .join(format!("{}-second.csv", scenario.name));
        let profile = match scenario.name {
            "repetitive_structured_v1" => Profile::Repetitive,
            "realistic_structured_v1" | "realistic_v2_multichunk" => Profile::Realistic,
            "high_cardinality_v1" => Profile::HighCardinality,
            "random_low_redundancy_v1" => Profile::Random,
            unknown => panic!("unexpected scenario {unknown}"),
        };
        generation::generate_to_path(profile, &first, scenario.rows, Some(scenario.seed)).unwrap();
        generation::generate_to_path(profile, &second, scenario.rows, Some(scenario.seed)).unwrap();
        assert_eq!(
            std::fs::read(&first).unwrap(),
            std::fs::read(&second).unwrap()
        );
    }
}

#[test]
fn smoke_suite_enforces_correctness_without_timing_thresholds() {
    let temporary = tempfile::tempdir().unwrap();
    let report = run_suite(Preset::Smoke, 2, temporary.path(), None).unwrap();

    assert_eq!(report.schema_version, 1);
    assert_eq!(report.report_type, "performance_regression_observation");
    assert_eq!(report.scenarios.len(), 5);
    assert!(!report.evidence_classification.timing_failure_thresholds);
    assert_eq!(
        report.evidence_classification.wall_clock_performance,
        "observational_only"
    );
    for scenario in report.scenarios {
        assert!(scenario.correctness.byte_exact_match);
        assert!(scenario.correctness.source_sha256_match);
        assert!(scenario.correctness.validation.valid);
        assert_eq!(scenario.correctness.validation.against_original, "matched");
        assert_eq!(scenario.archive_size_stable_across_runs, Some(true));
        assert_eq!(scenario.archive_sha256_stable_across_runs, Some(true));
        assert_eq!(scenario.compression.elapsed_ms_samples.len(), 2);
        assert_eq!(scenario.decompression.elapsed_ms_samples.len(), 2);
    }
}

#[test]
fn v2_scenario_requires_multiple_chunks_and_complete_integrity_checks() {
    let temporary = tempfile::tempdir().unwrap();
    let report = run_suite(Preset::Smoke, 1, temporary.path(), None).unwrap();
    let scenario = report
        .scenarios
        .iter()
        .find(|scenario| scenario.scenario == "realistic_v2_multichunk")
        .unwrap();

    assert_eq!(scenario.archive_version, 2);
    assert_eq!(scenario.selected_mode, "chunked_raw_zstd");
    assert!(scenario.chunk_count.unwrap() > 1);
    assert_eq!(scenario.correctness.validation.chunk_table, "passed");
    assert_eq!(scenario.correctness.validation.per_chunk_sha256, "passed");
    assert_eq!(scenario.correctness.validation.global_sha256, "passed");
    assert_eq!(scenario.correctness.validation.trailing_data, "passed");
}

#[test]
fn full_compare_keeps_factual_scope_and_per_run_identity_contract() {
    let temporary = tempfile::tempdir().unwrap();
    let input = temporary.path().join("compare.csv");
    generation::generate_to_path(Profile::Realistic, &input, 500, Some(2_026)).unwrap();
    let mut request = CompareRequest::new(&input);
    request.mode = CompareMode::Full;
    request.runs = 2;
    let report = application::compare(request).unwrap();

    assert_eq!(report.report_type, "comparison");
    assert_eq!(report.mode, "full");
    assert_eq!(report.scope.kind, "full");
    assert!(!report.scope.prefix_limited);
    assert_eq!(report.methodology.artifact_stability, "sha256_per_run");
    assert_eq!(report.methodology.validation, "sha256_roundtrip_per_run");
    assert_eq!(report.datapack.validation.status, "validated");
    assert!(report.datapack.validation.sha256_match);
    assert!(report.standalone_zstd.validation.sha256_match);
}

#[test]
fn path_classification_separates_wsl_windows_backed_and_linux_native_paths() {
    assert_eq!(
        classify_path(Path::new("/mnt/c/data/input.csv"), true, "linux"),
        "wsl_windows_backed_filesystem"
    );
    assert_eq!(
        classify_path(Path::new("/tmp/input.csv"), true, "linux"),
        "wsl_linux_native_filesystem"
    );
    assert_eq!(
        classify_path(Path::new("/var/tmp/input.csv"), false, "linux"),
        "native_linux_filesystem"
    );
    assert_eq!(
        classify_path(Path::new("C:\\data\\input.csv"), false, "windows"),
        "native_windows_filesystem"
    );
}

#[test]
fn median_is_deterministic_and_does_not_encode_a_speed_threshold() {
    assert_eq!(
        Preset::parse("representative").unwrap(),
        Preset::Representative
    );
    assert_eq!(scenarios(Preset::Representative).len(), 5);
    assert_eq!(
        median_duration(&[
            Duration::from_millis(9),
            Duration::from_millis(1),
            Duration::from_millis(5),
        ]),
        Duration::from_millis(5)
    );
    assert_eq!(
        median_duration(&[
            Duration::from_millis(10),
            Duration::from_millis(2),
            Duration::from_millis(6),
            Duration::from_millis(4),
        ]),
        Duration::from_millis(5)
    );
}
