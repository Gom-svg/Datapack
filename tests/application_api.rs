use std::fs;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::time::Instant;

use datapack::application::{
    self, AgainstStatusV1, AnalyzeRequest, ArchiveFormatV1, ArchiveModeV1, BenchmarkRequest,
    BenchmarkScopeV1, BenchmarkValidationStatusV1, CancellationToken, CheckStatusV1,
    CodecBackendV1, CompareMode, CompareRequest, CompressRequest, CompressionFormat,
    DecompressRequest, OperationControl, OperationError, OperationKind, ProgressEvent,
    ProgressPhase, ProgressState, V2CompressionOptions, ValidateRequest,
};
use datapack::error::{DatapackError, ErrorCategory};

fn write_repetitive_csv(path: &std::path::Path, rows: usize) -> Vec<u8> {
    let mut bytes = Vec::from(&b"group,status,note\n"[..]);
    for index in 0..rows {
        let row = if index % 2 == 0 {
            b"A,active,repeated\n".as_slice()
        } else {
            b"B,inactive,repeated\n".as_slice()
        };
        bytes.extend_from_slice(row);
    }
    fs::write(path, &bytes).expect("write application API input");
    bytes
}

fn v2_options(chunk_size_bytes: usize) -> V2CompressionOptions {
    let mut options = V2CompressionOptions::default();
    options.chunk_size_bytes = chunk_size_bytes;
    options.threads = 1;
    options.max_in_flight_chunks = 2;
    options
}

fn create_v2_archive(input: &Path, archive: &Path, chunk_size_bytes: usize) {
    let mut request = CompressRequest::new(input, archive);
    request.format = CompressionFormat::V2(v2_options(chunk_size_bytes));
    let result = application::compress(request).expect("create v2 archive through public API");
    assert_eq!(result.archive_version, 2);
    assert_eq!(result.selected_mode, ArchiveModeV1::ChunkedRawZstd);
    assert_eq!(result.backend, CodecBackendV1::ChunkedRawZstd);
}

fn assert_progress_lifecycle(events: &[ProgressEvent], operation: OperationKind) {
    assert!(!events.is_empty(), "{operation:?} emitted no progress");
    assert!(events.iter().all(|event| event.operation == operation));

    let mut phases = Vec::new();
    for (index, event) in events.iter().enumerate() {
        assert!(
            event
                .total_bytes
                .is_none_or(|total| event.completed_bytes <= total),
            "{operation:?} emitted bytes beyond its total: {event:?}"
        );
        assert!(
            event
                .total_items
                .is_none_or(|total| event.completed_items <= total),
            "{operation:?} emitted items beyond its total: {event:?}"
        );
        if !phases.contains(&event.phase) {
            phases.push(event.phase);
        }
        let preceding = &events[..index];
        let starts = preceding
            .iter()
            .filter(|candidate| {
                candidate.phase == event.phase && candidate.state == ProgressState::Started
            })
            .count();
        let completions = preceding
            .iter()
            .filter(|candidate| {
                candidate.phase == event.phase && candidate.state == ProgressState::Completed
            })
            .count();
        if event.state != ProgressState::Started {
            assert!(
                starts > completions,
                "{operation:?} emitted {event:?} without an active phase"
            );
        }
    }

    for phase in phases {
        let starts = events
            .iter()
            .filter(|event| event.phase == phase && event.state == ProgressState::Started)
            .count();
        let completions = events
            .iter()
            .filter(|event| event.phase == phase && event.state == ProgressState::Completed)
            .count();
        assert_eq!(
            starts, completions,
            "{operation:?} left {phase:?} lifecycle incomplete"
        );

        let mut last_bytes = 0;
        let mut last_items = 0;
        for event in events.iter().filter(|event| event.phase == phase) {
            if event.state == ProgressState::Started {
                last_bytes = 0;
                last_items = 0;
            } else {
                assert!(event.completed_bytes >= last_bytes);
                assert!(event.completed_items >= last_items);
                last_bytes = event.completed_bytes;
                last_items = event.completed_items;
            }
        }
    }

    assert!(events
        .last()
        .is_some_and(|event| event.is_terminal_success()));
}

fn partial_files(directory: &Path) -> Vec<PathBuf> {
    fs::read_dir(directory)
        .expect("read partial-output directory")
        .map(|entry| entry.expect("read partial-output entry").path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with(".partial"))
        })
        .collect()
}

#[test]
fn cancellation_token_is_monotonic_idempotent_and_shared() {
    let token = CancellationToken::new();
    let clone = token.clone();

    assert!(!token.is_cancelled());
    assert!(!clone.is_cancelled());
    clone.cancel();
    clone.cancel();

    assert!(token.is_cancelled());
    assert!(clone.is_cancelled());
}

#[test]
fn pre_cancelled_control_starts_no_output_work() {
    let directory = tempfile::tempdir().expect("pre-cancel directory");
    let input = directory.path().join("input.bin");
    let archive = directory.path().join("cancelled.dpack");
    fs::write(&input, b"pre-cancelled input").expect("write pre-cancel input");
    let token = CancellationToken::new();
    token.cancel();

    let result = application::compress_with_control(
        CompressRequest::new(&input, &archive),
        OperationControl::new().with_cancellation(token),
    );

    assert!(matches!(result, Err(OperationError::Cancelled)));
    assert!(!archive.exists());
    assert!(partial_files(directory.path()).is_empty());
}

#[test]
fn v1_compression_and_decompression_cancel_at_safe_stages() {
    let directory = tempfile::tempdir().expect("v1 cancellation directory");
    let input = directory.path().join("input.bin");
    let archive = directory.path().join("input.dpack");
    let restored = directory.path().join("restored.bin");
    let original = (0u8..=255).cycle().take(512 * 1024).collect::<Vec<_>>();
    fs::write(&input, &original).expect("write v1 cancellation input");

    let compression_token = CancellationToken::new();
    let observer_token = compression_token.clone();
    let mut compression_events = Vec::new();
    let mut compression_observer = |event: &ProgressEvent| {
        compression_events.push(*event);
        if event.phase == ProgressPhase::Compressing && event.state == ProgressState::Started {
            observer_token.cancel();
        }
    };
    let result = application::compress_with_control(
        CompressRequest::new(&input, &archive),
        OperationControl::new()
            .with_progress(&mut compression_observer)
            .with_cancellation(compression_token),
    );
    assert!(matches!(result, Err(OperationError::Cancelled)));
    assert!(!archive.exists());
    assert!(!compression_events
        .iter()
        .any(|event| event.is_terminal_success()));

    application::compress(CompressRequest::new(&input, &archive)).expect("create v1 archive");
    let decompression_token = CancellationToken::new();
    let observer_token = decompression_token.clone();
    let mut decompression_events = Vec::new();
    let mut decompression_observer = |event: &ProgressEvent| {
        decompression_events.push(*event);
        if event.phase == ProgressPhase::WritingOutput && event.state == ProgressState::Started {
            observer_token.cancel();
        }
    };
    let result = application::decompress_with_control(
        DecompressRequest::new(&archive, &restored),
        OperationControl::new()
            .with_progress(&mut decompression_observer)
            .with_cancellation(decompression_token),
    );
    assert!(matches!(result, Err(OperationError::Cancelled)));
    assert!(!restored.exists());
    assert!(!decompression_events
        .iter()
        .any(|event| event.is_terminal_success()));
    assert_eq!(fs::read(&input).expect("read unchanged source"), original);
}

#[test]
fn v2_compression_cancellation_preserves_existing_destination_and_bounds_cleanup() {
    let directory = tempfile::tempdir().expect("v2 compression cancellation directory");
    let input = directory.path().join("input.bin");
    let archive = directory.path().join("existing.dpack");
    let original = (0u8..=255)
        .cycle()
        .take(4 * 1024 * 1024)
        .collect::<Vec<_>>();
    fs::write(&input, &original).expect("write v2 cancellation input");
    fs::write(&archive, b"previous valid destination").expect("write previous destination");

    let token = CancellationToken::new();
    let observer_token = token.clone();
    let mut events = Vec::new();
    let mut observer = |event: &ProgressEvent| {
        events.push(*event);
        if event.phase == ProgressPhase::Compressing
            && event.state == ProgressState::Advanced
            && event
                .total_items
                .is_some_and(|total| event.completed_items == total)
        {
            observer_token.cancel();
        }
    };
    let mut request = CompressRequest::new(&input, &archive);
    request.format = CompressionFormat::V2(v2_options(64 * 1024));
    request.overwrite = true;
    let result = application::compress_with_control(
        request,
        OperationControl::new()
            .with_progress(&mut observer)
            .with_cancellation(token),
    );

    assert!(matches!(result, Err(OperationError::Cancelled)));
    assert_eq!(
        fs::read(&archive).expect("read preserved destination"),
        b"previous valid destination"
    );
    assert!(partial_files(directory.path()).is_empty());
    assert!(events.iter().any(|event| {
        event.phase == ProgressPhase::Compressing
            && event
                .total_items
                .is_some_and(|total| event.completed_items == total)
    }));
    assert!(!events.iter().any(|event| event.is_terminal_success()));
}

#[test]
fn v2_cancellation_honors_explicit_keep_partial_without_final_commit() {
    let directory = tempfile::tempdir().expect("v2 keep-partial cancellation directory");
    let input = directory.path().join("input.bin");
    let archive = directory.path().join("cancelled.dpack");
    fs::write(&input, vec![b'x'; 2 * 1024 * 1024]).expect("write keep-partial input");
    let token = CancellationToken::new();
    let observer_token = token.clone();
    let mut observer = move |event: &ProgressEvent| {
        if event.phase == ProgressPhase::Compressing && event.state == ProgressState::Advanced {
            observer_token.cancel();
        }
    };
    let mut request = CompressRequest::new(&input, &archive);
    request.format = CompressionFormat::V2(v2_options(64 * 1024));
    request.keep_partial = true;

    let result = application::compress_with_control(
        request,
        OperationControl::new()
            .with_progress(&mut observer)
            .with_cancellation(token),
    );

    assert!(matches!(result, Err(OperationError::Cancelled)));
    assert!(!archive.exists());
    let partials = partial_files(directory.path());
    assert_eq!(partials.len(), 1);
    assert!(fs::metadata(&partials[0]).expect("partial metadata").len() > 0);
}

#[test]
fn v2_decompression_cancels_after_a_committed_chunk_without_final_output() {
    let directory = tempfile::tempdir().expect("v2 decompression cancellation directory");
    let input = directory.path().join("input.bin");
    let archive = directory.path().join("input.dpack");
    let restored = directory.path().join("restored.bin");
    let original = (0u8..=255)
        .cycle()
        .take(2 * 1024 * 1024)
        .collect::<Vec<_>>();
    fs::write(&input, &original).expect("write decompression cancellation input");
    create_v2_archive(&input, &archive, 64 * 1024);

    let token = CancellationToken::new();
    let observer_token = token.clone();
    let mut events = Vec::new();
    let mut observer = |event: &ProgressEvent| {
        events.push(*event);
        if event.phase == ProgressPhase::Decompressing
            && event.state == ProgressState::Advanced
            && event.completed_items >= 1
        {
            observer_token.cancel();
        }
    };
    let result = application::decompress_with_control(
        DecompressRequest::new(&archive, &restored),
        OperationControl::new()
            .with_progress(&mut observer)
            .with_cancellation(token),
    );

    assert!(matches!(result, Err(OperationError::Cancelled)));
    assert!(!restored.exists());
    assert!(partial_files(directory.path()).is_empty());
    assert!(!events.iter().any(|event| event.is_terminal_success()));
    assert_eq!(
        fs::read(&input).expect("read source after cancel"),
        original
    );
}

#[test]
fn read_oriented_and_benchmark_operations_return_typed_cancellation() {
    let directory = tempfile::tempdir().expect("read cancellation directory");
    let input = directory.path().join("input.csv");
    let archive = directory.path().join("input.dpack");
    write_repetitive_csv(&input, 128);
    create_v2_archive(&input, &archive, 1_024);

    let run_cancelled =
        |operation: OperationKind,
         run: &mut dyn FnMut(OperationControl<'_>) -> Result<(), OperationError>| {
            let (target_phase, target_state) = match operation {
                OperationKind::Analyze => (ProgressPhase::Analyzing, ProgressState::Completed),
                OperationKind::Validate => (ProgressPhase::Validating, ProgressState::Completed),
                OperationKind::Compare => (ProgressPhase::Comparing, ProgressState::Completed),
                _ => (ProgressPhase::Benchmarking, ProgressState::Started),
            };
            let token = CancellationToken::new();
            let observer_token = token.clone();
            let mut events = Vec::new();
            let mut observer = |event: &ProgressEvent| {
                events.push(*event);
                if event.operation == operation
                    && event.phase == target_phase
                    && event.state == target_state
                {
                    observer_token.cancel();
                }
            };
            let result = run(OperationControl::new()
                .with_progress(&mut observer)
                .with_cancellation(token));
            assert!(matches!(result, Err(OperationError::Cancelled)));
            assert!(!events.iter().any(|event| event.is_terminal_success()));
        };

    run_cancelled(OperationKind::Analyze, &mut |control| {
        application::analyze_with_control(AnalyzeRequest::new(&input), control).map(|_| ())
    });
    run_cancelled(OperationKind::Validate, &mut |control| {
        application::validate_with_control(ValidateRequest::new(&archive), control).map(|_| ())
    });
    run_cancelled(OperationKind::Compare, &mut |control| {
        let mut request = CompareRequest::new(&input);
        request.runs = 1;
        application::compare_with_control(request, control).map(|_| ())
    });
    run_cancelled(OperationKind::Benchmark, &mut |control| {
        let mut request = BenchmarkRequest::new(&input);
        request.quick = true;
        request.runs = 1;
        application::benchmark_with_control(request, control).map(|_| ())
    });
}

#[test]
fn benchmark_cancellation_after_planning_prevents_remaining_measurements() {
    let directory = tempfile::tempdir().expect("benchmark cancellation directory");
    let input = directory.path().join("input.csv");
    write_repetitive_csv(&input, 512);
    let token = CancellationToken::new();
    let observer_token = token.clone();
    let mut events = Vec::new();
    let mut observer = |event: &ProgressEvent| {
        events.push(*event);
        if event.phase == ProgressPhase::Planning && event.state == ProgressState::Completed {
            observer_token.cancel();
        }
    };
    let mut request = BenchmarkRequest::new(&input);
    request.quick = true;
    request.runs = 1;

    let result = application::benchmark_with_control(
        request,
        OperationControl::new()
            .with_progress(&mut observer)
            .with_cancellation(token),
    );

    assert!(matches!(result, Err(OperationError::Cancelled)));
    assert!(events.iter().any(|event| {
        event.phase == ProgressPhase::Planning && event.state == ProgressState::Completed
    }));
    assert!(!events
        .iter()
        .any(|event| event.phase == ProgressPhase::Compressing));
    assert!(!events.iter().any(|event| event.is_terminal_success()));
}

#[test]
fn successful_commit_wins_over_late_cancellation_and_still_completes_progress() {
    let directory = tempfile::tempdir().expect("commit boundary directory");
    let input = directory.path().join("input.bin");
    let archive = directory.path().join("input.dpack");
    let original = (0u8..=255).cycle().take(512 * 1024).collect::<Vec<_>>();
    fs::write(&input, &original).expect("write commit boundary input");

    let token = CancellationToken::new();
    let observer_token = token.clone();
    let mut events = Vec::new();
    let mut observer = |event: &ProgressEvent| {
        events.push(*event);
        if event.phase == ProgressPhase::Finalizing && event.state == ProgressState::Started {
            observer_token.cancel();
        }
    };
    let mut request = CompressRequest::new(&input, &archive);
    request.format = CompressionFormat::V2(v2_options(64 * 1024));
    let report = application::compress_with_control(
        request,
        OperationControl::new()
            .with_progress(&mut observer)
            .with_cancellation(token.clone()),
    )
    .expect("commit must win before terminal progress");

    assert_eq!(report.archive_version, 2);
    assert!(archive.is_file());
    assert!(token.is_cancelled());
    assert!(events
        .last()
        .is_some_and(|event| event.is_terminal_success()));
}

#[test]
fn installed_control_that_never_cancels_is_observational_and_byte_exact() {
    let directory = tempfile::tempdir().expect("cancellation overhead directory");
    let input = directory.path().join("input.bin");
    let baseline = directory.path().join("baseline.dpack");
    let controlled = directory.path().join("controlled.dpack");
    let original = (0u8..=255)
        .cycle()
        .take(4 * 1024 * 1024)
        .collect::<Vec<_>>();
    fs::write(&input, &original).expect("write overhead input");

    let mut baseline_request = CompressRequest::new(&input, &baseline);
    baseline_request.format = CompressionFormat::V2(v2_options(256 * 1024));
    let started = Instant::now();
    application::compress(baseline_request).expect("baseline compression");
    let baseline_elapsed = started.elapsed();

    let mut controlled_request = CompressRequest::new(&input, &controlled);
    controlled_request.format = CompressionFormat::V2(v2_options(256 * 1024));
    let token = CancellationToken::new();
    let started = Instant::now();
    application::compress_with_control(
        controlled_request,
        OperationControl::new().with_cancellation(token.clone()),
    )
    .expect("never-cancelled compression");
    let controlled_elapsed = started.elapsed();

    assert!(!token.is_cancelled());
    assert_eq!(
        fs::read(&baseline).expect("read baseline archive"),
        fs::read(&controlled).expect("read controlled archive")
    );
    eprintln!(
        "OBSERVATIONAL cancellation overhead: disabled={baseline_elapsed:?} installed_not_cancelled={controlled_elapsed:?} chunks=16"
    );
}

#[test]
fn ordinary_failures_remain_distinct_from_typed_cancellation() {
    let directory = tempfile::tempdir().expect("ordinary failure directory");
    let missing = directory.path().join("missing.bin");
    let output = directory.path().join("output.dpack");
    let token = CancellationToken::new();

    let result = application::compress_with_control(
        CompressRequest::new(&missing, &output),
        OperationControl::new().with_cancellation(token),
    );

    let error = result.expect_err("missing input must be an ordinary failure");
    assert!(!error.is_cancelled());
    assert_eq!(error.category(), ErrorCategory::Format);
    assert_eq!(error.code(), "invalid_format");
    assert!(error.to_string().contains("input path"));
    assert!(error
        .to_string()
        .contains(missing.to_string_lossy().as_ref()));

    let cancelled = OperationError::Cancelled;
    assert!(cancelled.is_cancelled());
    assert_eq!(cancelled.category(), ErrorCategory::Cancellation);
    assert_eq!(cancelled.code(), "cancelled");

    let format = OperationError::Failed(DatapackError::InvalidFormat("context".to_string()));
    assert_eq!(format.category(), ErrorCategory::Format);
    assert_eq!(format.code(), "invalid_format");
}

#[test]
fn analyze_returns_a_versioned_privacy_safe_report() {
    let directory = tempfile::tempdir().expect("analysis API directory");
    let input = directory.path().join("private-analysis.csv");
    let private_header = "PRIVATE_HEADER_12A7";
    let private_value = "PRIVATE_VALUE_91F4";
    let bytes = format!("kind,{private_header}\nA,{private_value}\nA,{private_value}\n");
    fs::write(&input, bytes).expect("write private analysis input");

    let mut request = AnalyzeRequest::new(&input);
    request.sample_mb = 1;
    let report = application::analyze(request).expect("analyze through public API");

    assert_eq!(report.schema_version, 1);
    assert_eq!(report.report_type, "analysis");
    assert_eq!(
        report.dataset.source_size_bytes,
        fs::metadata(&input).unwrap().len()
    );
    assert_eq!(report.dataset.column_count, Some(2));
    assert_eq!(report.dataset.parser.delimiter, ",");
    assert_eq!(report.dataset.columns.len(), 2);

    let json = serde_json::to_string(&report).expect("serialize public analysis report");
    assert!(!json.contains(private_header));
    assert!(!json.contains(private_value));
    assert!(!json.contains(input.to_string_lossy().as_ref()));
}

#[test]
fn v1_structured_application_path_reports_selected_mode_and_is_byte_exact() {
    let directory = tempfile::tempdir().expect("v1 application API directory");
    let input = directory.path().join("input.csv");
    let archive = directory.path().join("input-v1.dpack");
    let restored = directory.path().join("restored.csv");
    let original = write_repetitive_csv(&input, 2_048);

    let compression =
        application::compress(CompressRequest::new(&input, &archive)).expect("compress v1");
    assert_eq!(compression.schema_version, 1);
    assert_eq!(compression.report_type, "compression");
    assert_eq!(compression.archive_version, 1);
    assert_eq!(
        compression.selected_mode,
        ArchiveModeV1::CsvColumnarDictionary
    );
    assert_eq!(compression.backend, CodecBackendV1::Zstd);
    assert_eq!(compression.input_size_bytes, original.len() as u64);
    assert_eq!(
        compression.archive_size_bytes,
        fs::metadata(&archive).unwrap().len()
    );

    let decompression = application::decompress(DecompressRequest::new(&archive, &restored))
        .expect("decompress v1");
    assert_eq!(decompression.schema_version, 1);
    assert_eq!(decompression.report_type, "decompression");
    assert_eq!(decompression.archive_version, 1);
    assert_eq!(
        decompression.selected_mode,
        ArchiveModeV1::CsvColumnarDictionary
    );
    assert_eq!(decompression.backend, CodecBackendV1::Zstd);
    assert_eq!(decompression.restored_size_bytes, original.len() as u64);
    assert_eq!(
        fs::read(restored).expect("read restored v1 bytes"),
        original
    );
}

#[test]
fn v1_raw_application_path_reports_streaming_backend_and_is_byte_exact() {
    let directory = tempfile::tempdir().expect("v1 raw application API directory");
    let input = directory.path().join("unstructured.bin");
    let archive = directory.path().join("raw-v1.dpack");
    let restored = directory.path().join("restored.bin");
    let original = (0u8..=255).cycle().take(8_193).collect::<Vec<_>>();
    fs::write(&input, &original).expect("write unstructured v1 input");

    let compression =
        application::compress(CompressRequest::new(&input, &archive)).expect("compress raw v1");
    assert_eq!(compression.archive_version, 1);
    assert_eq!(compression.selected_mode, ArchiveModeV1::RawZstd);
    assert_eq!(compression.backend, CodecBackendV1::RawZstdStreaming);

    let decompression = application::decompress(DecompressRequest::new(&archive, &restored))
        .expect("decompress raw v1");
    assert_eq!(decompression.selected_mode, ArchiveModeV1::RawZstd);
    assert_eq!(decompression.backend, CodecBackendV1::RawZstdStreaming);
    assert_eq!(fs::read(restored).expect("read raw v1 output"), original);
}

#[test]
fn v2_application_compression_and_decompression_are_byte_exact() {
    let directory = tempfile::tempdir().expect("v2 application API directory");
    let input = directory.path().join("input.bin");
    let archive = directory.path().join("input-v2.dpack");
    let restored = directory.path().join("restored.bin");
    let original = (0u8..=255).cycle().take(12_345).collect::<Vec<_>>();
    fs::write(&input, &original).expect("write v2 input");

    let mut request = CompressRequest::new(&input, &archive);
    request.format = CompressionFormat::V2(v2_options(1_024));
    let compression = application::compress(request).expect("compress v2");
    assert_eq!(compression.archive_version, 2);
    assert_eq!(compression.selected_mode, ArchiveModeV1::ChunkedRawZstd);
    assert_eq!(compression.backend, CodecBackendV1::ChunkedRawZstd);

    let decompression = application::decompress(DecompressRequest::new(&archive, &restored))
        .expect("decompress v2");
    assert_eq!(decompression.archive_version, 2);
    assert_eq!(decompression.verified, Some(true));
    assert_eq!(decompression.selected_mode, ArchiveModeV1::ChunkedRawZstd);
    assert_eq!(decompression.backend, CodecBackendV1::V2RawZstdFrame);
    assert_eq!(decompression.restored_size_bytes, original.len() as u64);
    assert_eq!(
        fs::read(restored).expect("read restored v2 bytes"),
        original
    );
}

#[test]
fn validation_service_reports_a_valid_archive_and_against_match() {
    let directory = tempfile::tempdir().expect("validation application API directory");
    let input = directory.path().join("input.csv");
    let archive = directory.path().join("input.dpack");
    write_repetitive_csv(&input, 32);
    application::compress(CompressRequest::new(&input, &archive)).expect("create valid archive");

    let mut request = ValidateRequest::new(&archive);
    request.against = Some(input);
    let report = application::validate(request).expect("validate through public API");

    assert!(report.valid);
    assert_eq!(report.schema_version, 1);
    assert_eq!(report.report_type, "validation");
    assert!(matches!(
        report.archive.format,
        Some(ArchiveFormatV1::DpackV1)
    ));
    assert_eq!(report.checks.header, CheckStatusV1::Passed);
    assert_eq!(report.checks.decompression, CheckStatusV1::Passed);
    assert_eq!(report.against.status, AgainstStatusV1::Matched);
    assert!(report.diagnostics.is_empty());
}

#[test]
fn v2_validation_and_resource_limits_are_effective_through_the_public_api() {
    let directory = tempfile::tempdir().expect("v2 validation application API directory");
    let input = directory.path().join("input.bin");
    let archive = directory.path().join("input-v2.dpack");
    let original = (0u8..=255).cycle().take(12_345).collect::<Vec<_>>();
    fs::write(&input, &original).expect("write v2 validation input");
    create_v2_archive(&input, &archive, 1_024);

    let mut request = ValidateRequest::new(&archive);
    request.against = Some(input.clone());
    let report = application::validate(request).expect("validate v2 through public API");
    assert!(report.valid);
    assert!(matches!(
        report.archive.format,
        Some(ArchiveFormatV1::DpackV2)
    ));
    assert!(report.archive.chunk_count.is_some_and(|count| count > 1));
    assert_eq!(report.checks.header, CheckStatusV1::Passed);
    assert_eq!(report.checks.metadata, CheckStatusV1::Passed);
    assert_eq!(report.checks.chunk_table, CheckStatusV1::Passed);
    assert_eq!(report.checks.payload_structure, CheckStatusV1::Passed);
    assert_eq!(report.checks.decompression, CheckStatusV1::Passed);
    assert_eq!(report.checks.restored_length, CheckStatusV1::Passed);
    assert_eq!(report.checks.per_chunk_sha256, CheckStatusV1::Passed);
    assert_eq!(report.checks.global_sha256, CheckStatusV1::Passed);
    assert_eq!(report.checks.trailing_data, CheckStatusV1::Passed);
    assert_eq!(report.against.status, AgainstStatusV1::Matched);

    let output_limit_path = directory.path().join("output-limit.bin");
    let mut output_limit = DecompressRequest::new(&archive, &output_limit_path);
    output_limit.max_output_bytes = Some(original.len() as u64 - 1);
    assert!(application::decompress(output_limit).is_err());
    assert!(!output_limit_path.exists());

    let chunk_limit_path = directory.path().join("chunk-limit.bin");
    let mut chunk_limit = DecompressRequest::new(&archive, &chunk_limit_path);
    chunk_limit.max_chunks = Some(1);
    assert!(application::decompress(chunk_limit).is_err());
    assert!(!chunk_limit_path.exists());

    let memory_limit_path = directory.path().join("memory-limit.bin");
    let mut memory_limit = DecompressRequest::new(&archive, &memory_limit_path);
    memory_limit.max_memory_bytes = Some(1);
    assert!(application::decompress(memory_limit).is_err());
    assert!(!memory_limit_path.exists());

    let mut validation_output_limit = ValidateRequest::new(&archive);
    validation_output_limit.max_output_bytes = Some(original.len() as u64 - 1);
    let report =
        application::validate(validation_output_limit).expect("report v2 validation output limit");
    assert!(!report.valid);
    assert!(report
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "DECLARED_OUTPUT_LIMIT_REACHED"));

    let mut validation_chunk_limit = ValidateRequest::new(&archive);
    validation_chunk_limit.max_chunks = Some(1);
    let report =
        application::validate(validation_chunk_limit).expect("report v2 validation chunk limit");
    assert!(!report.valid);
    assert!(report
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "CHUNK_COUNT_LIMIT_REACHED"));

    let mut validation_memory_limit = ValidateRequest::new(&archive);
    validation_memory_limit.max_memory_bytes = 1;
    let report =
        application::validate(validation_memory_limit).expect("report v2 validation memory limit");
    assert!(!report.valid);
    assert!(report
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "VALIDATION_MEMORY_LIMIT_REACHED"));
}

#[test]
fn compare_service_returns_factual_single_run_results() {
    let directory = tempfile::tempdir().expect("comparison application API directory");
    let input = directory.path().join("input.csv");
    let original = write_repetitive_csv(&input, 64);

    let mut request = CompareRequest::new(&input);
    request.mode = CompareMode::Quick;
    request.runs = 1;
    let report = application::compare(request).expect("compare through public API");

    assert_eq!(report.schema_version, 1);
    assert_eq!(report.report_type, "comparison");
    assert_eq!(report.mode, "quick");
    assert_eq!(report.methodology.runs, 1);
    assert_eq!(report.scope.source_size_bytes, original.len() as u64);
    assert_eq!(report.scope.compared_size_bytes, original.len() as u64);
    assert_eq!(report.datapack.compression.samples_ms.len(), 1);
    assert_eq!(report.standalone_zstd.compression.samples_ms.len(), 1);
    assert!(report.datapack.validation.sha256_match);
    assert!(report.standalone_zstd.validation.sha256_match);
}

#[test]
fn benchmark_service_supports_estimate_only_and_measured_runs() {
    let directory = tempfile::tempdir().expect("benchmark application API directory");
    let input = directory.path().join("input.csv");
    let original = write_repetitive_csv(&input, 64);

    let mut estimate_request = BenchmarkRequest::new(&input);
    estimate_request.estimate_only = true;
    estimate_request.runs = 1;
    let estimate = application::benchmark(estimate_request).expect("estimate-only benchmark");
    assert!(estimate.estimate_only);
    assert_eq!(estimate.schema_version, 1);
    assert_eq!(estimate.report_type, "benchmark");
    assert_eq!(estimate.source_size_bytes, original.len() as u64);
    assert_eq!(estimate.scope, BenchmarkScopeV1::EstimateOnly);
    assert_eq!(
        estimate.validation_status,
        BenchmarkValidationStatusV1::NotValidated
    );
    assert!(!estimate.zstd_baseline_performed);
    assert!(!estimate.roundtrip_performed);
    assert!(!estimate.hash_performed);
    assert_eq!(estimate.runs_used, 0);
    assert!(estimate.selected_mode.is_none());
    assert!(estimate.datapack_size_bytes.is_none());

    let mut measured_request = BenchmarkRequest::new(&input);
    measured_request.quick = true;
    measured_request.runs = 1;
    let measured = application::benchmark(measured_request).expect("measured benchmark");
    assert!(!measured.estimate_only);
    assert_eq!(measured.runs_used, 1);
    assert_eq!(measured.source_size_bytes, original.len() as u64);
    assert_eq!(measured.measured_input_size_bytes, original.len() as u64);
    assert_eq!(measured.scope, BenchmarkScopeV1::Full);
    assert_eq!(
        measured.validation_status,
        BenchmarkValidationStatusV1::Validated
    );
    assert!(measured.zstd_baseline_performed);
    assert!(measured.roundtrip_performed);
    assert!(measured.hash_performed);
    assert!(measured.selected_mode.is_some());
    assert!(measured.datapack_size_bytes.is_some());
    assert_eq!(measured.roundtrip_sha256_match, Some(true));
}

#[test]
fn benchmark_opt_outs_preserve_structured_partial_reasons() {
    let directory = tempfile::tempdir().expect("benchmark opt-out API directory");
    let input = directory.path().join("input.csv");
    write_repetitive_csv(&input, 64);

    let mut request = BenchmarkRequest::new(&input);
    request.quick = true;
    request.runs = 1;
    request.include_zstd_baseline = false;
    request.roundtrip = false;
    request.hash = false;
    let report = application::benchmark(request).expect("run benchmark with opt-outs");

    assert_eq!(report.scope, BenchmarkScopeV1::Partial);
    assert_eq!(
        report.validation_status,
        BenchmarkValidationStatusV1::NotValidated
    );
    assert!(!report.zstd_baseline_performed);
    assert!(!report.roundtrip_performed);
    assert!(!report.hash_performed);
    assert_eq!(report.zstd_size_bytes, None);
    assert_eq!(report.decompression_time_ms, None);
    assert_eq!(report.roundtrip_sha256_match, None);
    assert_eq!(report.partial_reasons.len(), 3);
    assert_eq!(
        report
            .partial_reasons
            .iter()
            .filter(|reason| {
                reason.as_str() == "round-trip decompression skipped; output identity not validated"
            })
            .count(),
        1
    );
    assert!(!report
        .partial_reasons
        .iter()
        .any(|reason| reason == "round-trip decompression skipped"));
    assert!(!report
        .partial_reasons
        .iter()
        .any(|reason| reason == "output identity not validated"));
}

#[test]
fn benchmark_keep_artifacts_returns_paths_and_profile_facts() {
    let directory = tempfile::tempdir().expect("benchmark artifacts API directory");
    let input = directory.path().join("raw-benchmark.csv");
    let mut original = Vec::from(&b"id,value\n"[..]);
    for index in 0..1_024 {
        original.extend_from_slice(
            format!("{index},unique-value-{index:08x}-{index:08x}\n").as_bytes(),
        );
    }
    fs::write(&input, &original).expect("write raw benchmark input");

    let mut request = BenchmarkRequest::new(&input);
    request.quick = true;
    request.runs = 1;
    request.keep_artifacts = true;
    let report = application::benchmark(request).expect("benchmark with retained artifacts");

    assert_eq!(report.scope, BenchmarkScopeV1::Full);
    assert_eq!(
        report.validation_status,
        BenchmarkValidationStatusV1::Validated
    );
    assert!(report.zstd_baseline_performed);
    assert!(report.roundtrip_performed);
    assert!(report.hash_performed);
    assert_eq!(report.selected_mode, Some(ArchiveModeV1::RawZstd));
    assert_eq!(report.roundtrip_sha256_match, Some(true));
    assert!(report.zstd_compression_time_ms.is_some());
    assert!(report.zstd_compression_mib_per_second.is_some());

    for (label, path) in [
        ("DataPack", report.artifacts.datapack.as_ref()),
        ("restored", report.artifacts.restored.as_ref()),
        ("zstd", report.artifacts.zstd.as_ref()),
    ] {
        let path = path.unwrap_or_else(|| panic!("missing retained {label} artifact"));
        assert!(path.is_file(), "retained {label} artifact does not exist");
    }
    assert_eq!(
        fs::read(
            report
                .artifacts
                .restored
                .as_ref()
                .expect("retained restored path")
        )
        .expect("read retained restored artifact"),
        original
    );
    assert!(report.artifacts.chunked.is_none());
    assert!(report.artifacts.chunked_restored.is_none());
    assert!(report.artifacts.chunked_sample.is_none());

    assert!(report.profile.planning_ms.is_some());
    assert!(report.profile.zstd_only_ms.is_some());
    assert!(report.profile.datapack_compress_ms.is_some());
    assert!(report.profile.datapack_decompress_ms.is_some());
    assert!(report.profile.hash_ms.is_some());
    assert!(report.profile.total_elapsed_ms.is_some());
    assert!(report.profile.compression_mib_per_second.is_some());
    assert!(report.profile.decompression_mib_per_second.is_some());
}

#[test]
fn typed_progress_is_ordered_and_contains_only_safe_facts() {
    let directory = tempfile::tempdir().expect("progress application API directory");
    let input = directory.path().join("PRIVATE_PROGRESS_PATH_61CE.csv");
    let private_value = "PRIVATE_PROGRESS_VALUE_407B";
    fs::write(
        &input,
        format!("kind,note\nA,{private_value}\nA,{private_value}\n"),
    )
    .expect("write progress input");

    let mut events = Vec::<ProgressEvent>::new();
    let mut observer = |event: &ProgressEvent| events.push(*event);
    application::analyze_with_progress(AnalyzeRequest::new(&input), &mut observer)
        .expect("analyze with typed progress");

    assert!(events.len() >= 4);
    assert!(events
        .iter()
        .all(|event| event.operation == OperationKind::Analyze));
    assert!(events.iter().any(|event| {
        event.phase == ProgressPhase::Analyzing && event.state == ProgressState::Completed
    }));
    assert_eq!(events.first().unwrap().state, ProgressState::Started);
    assert_eq!(events.last().unwrap().state, ProgressState::Completed);
    assert!(events.last().unwrap().is_terminal_success());
    assert_progress_lifecycle(&events, OperationKind::Analyze);
    let serialized = serde_json::to_value(events.first().unwrap()).expect("serialize progress");
    assert_eq!(serialized["operation"], "analyze");
    assert_eq!(serialized["phase"], "analyzing");
    assert_eq!(serialized["state"], "started");
    let transcript = format!("{events:?}");
    assert!(!transcript.contains(input.to_string_lossy().as_ref()));
    assert!(!transcript.contains(private_value));
}

#[test]
fn every_public_operation_emits_balanced_typed_progress() {
    let directory = tempfile::tempdir().expect("all-operation progress API directory");
    let input = directory.path().join("input.csv");
    let archive = directory.path().join("input-v2.dpack");
    let restored = directory.path().join("restored.csv");
    write_repetitive_csv(&input, 256);

    let mut compress_events = Vec::new();
    let mut compress_request = CompressRequest::new(&input, &archive);
    compress_request.format = CompressionFormat::V2(v2_options(1_024));
    {
        let mut observer = |event: &ProgressEvent| compress_events.push(*event);
        application::compress_with_progress(compress_request, &mut observer)
            .expect("compress with typed progress");
    }
    assert_progress_lifecycle(&compress_events, OperationKind::Compress);
    assert!(compress_events.iter().any(|event| {
        event.phase == ProgressPhase::Compressing
            && event.state == ProgressState::Advanced
            && event.completed_items > 0
            && event.total_items.is_some()
    }));

    let mut decompress_events = Vec::new();
    {
        let mut observer = |event: &ProgressEvent| decompress_events.push(*event);
        application::decompress_with_progress(
            DecompressRequest::new(&archive, &restored),
            &mut observer,
        )
        .expect("decompress with typed progress");
    }
    assert_progress_lifecycle(&decompress_events, OperationKind::Decompress);
    assert!(decompress_events.iter().any(|event| {
        event.phase == ProgressPhase::Decompressing
            && event.state == ProgressState::Advanced
            && event.completed_items > 0
            && event.total_items.is_some()
    }));

    let mut validate_events = Vec::new();
    {
        let mut observer = |event: &ProgressEvent| validate_events.push(*event);
        application::validate_with_progress(ValidateRequest::new(&archive), &mut observer)
            .expect("validate with typed progress");
    }
    assert_progress_lifecycle(&validate_events, OperationKind::Validate);
    assert!(validate_events.iter().any(|event| {
        event.phase == ProgressPhase::Validating && event.state == ProgressState::Completed
    }));

    let mut compare_request = CompareRequest::new(&input);
    compare_request.runs = 1;
    let mut compare_events = Vec::new();
    {
        let mut observer = |event: &ProgressEvent| compare_events.push(*event);
        application::compare_with_progress(compare_request, &mut observer)
            .expect("compare with typed progress");
    }
    assert_progress_lifecycle(&compare_events, OperationKind::Compare);
    assert!(compare_events.iter().any(|event| {
        event.phase == ProgressPhase::Comparing && event.state == ProgressState::Completed
    }));

    let mut benchmark_request = BenchmarkRequest::new(&input);
    benchmark_request.quick = true;
    benchmark_request.runs = 1;
    let mut benchmark_events = Vec::new();
    {
        let mut observer = |event: &ProgressEvent| benchmark_events.push(*event);
        application::benchmark_with_progress(benchmark_request, &mut observer)
            .expect("benchmark with typed progress");
    }
    assert_progress_lifecycle(&benchmark_events, OperationKind::Benchmark);
    assert_eq!(
        benchmark_events
            .first()
            .map(|event| (event.phase, event.state)),
        Some((ProgressPhase::Benchmarking, ProgressState::Started))
    );
    assert!(benchmark_events
        .last()
        .is_some_and(|event| event.is_terminal_success()));
    assert!(benchmark_events.iter().any(|event| {
        event.phase == ProgressPhase::Planning && event.state == ProgressState::Completed
    }));
}

#[test]
fn progress_is_observational_and_v2_completion_tracks_committed_chunks() {
    let directory = tempfile::tempdir().expect("progress equivalence directory");
    let input = directory.path().join("input.bin");
    let silent_archive = directory.path().join("silent.dpack");
    let observed_archive = directory.path().join("observed.dpack");
    let original = (0u8..=255)
        .cycle()
        .take(4 * 1024 * 1024)
        .collect::<Vec<_>>();
    fs::write(&input, &original).expect("write progress equivalence input");

    let mut silent_request = CompressRequest::new(&input, &silent_archive);
    silent_request.format = CompressionFormat::V2(v2_options(256 * 1024));
    let silent_started = Instant::now();
    application::compress(silent_request).expect("compress without progress");
    let silent_elapsed = silent_started.elapsed();

    let mut observed_request = CompressRequest::new(&input, &observed_archive);
    observed_request.format = CompressionFormat::V2(v2_options(256 * 1024));
    let mut events = Vec::new();
    let observed_started = Instant::now();
    {
        let mut observer = |event: &ProgressEvent| events.push(*event);
        application::compress_with_progress(observed_request, &mut observer)
            .expect("compress with no-op progress collection");
    }
    let observed_elapsed = observed_started.elapsed();

    assert_eq!(
        fs::read(&silent_archive).expect("read silent archive"),
        fs::read(&observed_archive).expect("read observed archive")
    );
    assert_progress_lifecycle(&events, OperationKind::Compress);
    let committed = events
        .iter()
        .rev()
        .find(|event| event.phase == ProgressPhase::Compressing)
        .expect("v2 compression completion");
    assert_eq!(committed.state, ProgressState::Completed);
    assert_eq!(committed.completed_bytes, original.len() as u64);
    assert_eq!(committed.completed_items, committed.total_items.unwrap());
    assert_eq!(committed.percentage(), Some(100.0));

    eprintln!(
        "OBSERVATIONAL progress overhead: disabled={silent_elapsed:?} enabled={observed_elapsed:?} events={}",
        events.len()
    );
}

#[test]
fn failed_operation_never_emits_terminal_success() {
    let directory = tempfile::tempdir().expect("failed progress directory");
    let missing = directory.path().join("missing.csv");
    let mut events = Vec::new();
    let mut observer = |event: &ProgressEvent| events.push(*event);
    assert!(
        application::analyze_with_progress(AnalyzeRequest::new(missing), &mut observer).is_err()
    );
    assert!(!events.is_empty());
    assert!(!events.iter().any(|event| event.is_terminal_success()));
}

#[test]
fn empty_v2_progress_has_safe_zero_totals() {
    let directory = tempfile::tempdir().expect("empty progress directory");
    let input = directory.path().join("empty.bin");
    let archive = directory.path().join("empty.dpack");
    let restored = directory.path().join("empty-restored.bin");
    fs::write(&input, []).expect("write empty input");

    let mut request = CompressRequest::new(&input, &archive);
    request.format = CompressionFormat::V2(v2_options(1_024));
    let mut events = Vec::new();
    {
        let mut observer = |event: &ProgressEvent| events.push(*event);
        application::compress_with_progress(request, &mut observer)
            .expect("compress empty input with progress");
    }
    let phase_events = events
        .iter()
        .filter(|event| event.phase == ProgressPhase::Compressing)
        .collect::<Vec<_>>();
    assert_eq!(phase_events.len(), 2);
    assert_eq!(phase_events[0].total_bytes, Some(0));
    assert_eq!(phase_events[0].total_items, Some(0));
    assert_eq!(phase_events[0].percentage(), None);
    assert_eq!(phase_events[1].percentage(), Some(100.0));
    assert_eq!(phase_events[1].completed_items, 0);
    assert_progress_lifecycle(&events, OperationKind::Compress);

    application::decompress(DecompressRequest::new(&archive, &restored))
        .expect("decompress empty v2 archive");
    assert!(fs::read(restored)
        .expect("read empty restored output")
        .is_empty());
}

#[test]
fn panicking_rust_observer_unwinds_without_publishing_partial_output() {
    let directory = tempfile::tempdir().expect("panicking observer directory");
    let input = directory.path().join("input.bin");
    let archive = directory.path().join("output.dpack");
    let original = (0u8..=255).cycle().take(4_097).collect::<Vec<_>>();
    fs::write(&input, &original).expect("write panicking observer input");
    let mut request = CompressRequest::new(&input, &archive);
    request.format = CompressionFormat::V2(v2_options(1_024));

    let result = catch_unwind(AssertUnwindSafe(|| {
        let mut observer = |event: &ProgressEvent| {
            if event.state == ProgressState::Advanced {
                panic!("intentional progress observer panic");
            }
        };
        let _ = application::compress_with_progress(request, &mut observer);
    }));

    assert!(result.is_err());
    assert!(!archive.exists());
    assert_eq!(fs::read(input).expect("read source after panic"), original);
}

#[test]
fn invalid_request_limits_fail_without_creating_output() {
    let directory = tempfile::tempdir().expect("invalid application API directory");
    let input = directory.path().join("input.csv");
    let archive = directory.path().join("invalid.dpack");
    write_repetitive_csv(&input, 8);

    let mut v2 = V2CompressionOptions::default();
    v2.threads = 0;
    let mut compress = CompressRequest::new(&input, &archive);
    compress.format = CompressionFormat::V2(v2);
    assert!(application::compress(compress).is_err());
    assert!(!archive.exists());

    let mut compare = CompareRequest::new(&input);
    compare.runs = 0;
    assert!(application::compare(compare).is_err());

    let mut validate = ValidateRequest::new(directory.path().join("missing.dpack"));
    validate.max_memory_bytes = 0;
    assert!(application::validate(validate).is_err());
}

#[test]
fn existing_outputs_are_preserved_without_explicit_overwrite() {
    let directory = tempfile::tempdir().expect("transactional application API directory");
    let input = directory.path().join("input.csv");
    let protected_archive = directory.path().join("protected.dpack");
    let archive = directory.path().join("valid.dpack");
    let protected_output = directory.path().join("protected.csv");
    let original = write_repetitive_csv(&input, 16);
    let archive_sentinel = b"EXISTING_ARCHIVE_MUST_SURVIVE";
    let output_sentinel = b"EXISTING_OUTPUT_MUST_SURVIVE";
    fs::write(&protected_archive, archive_sentinel).expect("write protected archive");

    let error = application::compress(CompressRequest::new(&input, &protected_archive))
        .expect_err("protected archive must require explicit overwrite");
    assert_eq!(error.category(), ErrorCategory::Format);
    assert_eq!(error.code(), "invalid_format");
    assert!(error
        .to_string()
        .contains(protected_archive.to_string_lossy().as_ref()));
    assert!(error.to_string().contains("--force"));
    assert_eq!(
        fs::read(&protected_archive).expect("read protected archive"),
        archive_sentinel
    );

    application::compress(CompressRequest::new(&input, &archive)).expect("create valid archive");
    fs::write(&protected_output, output_sentinel).expect("write protected output");
    assert!(application::decompress(DecompressRequest::new(&archive, &protected_output)).is_err());
    assert_eq!(
        fs::read(&protected_output).expect("read protected output"),
        output_sentinel
    );
    assert_eq!(fs::read(input).expect("read source"), original);
}

#[test]
fn explicit_overwrite_replaces_outputs_transactionally() {
    let directory = tempfile::tempdir().expect("overwrite application API directory");
    let input = directory.path().join("input.bin");
    let archive = directory.path().join("replacement.dpack");
    let restored = directory.path().join("replacement.bin");
    let original = (0u8..=255).cycle().take(4_097).collect::<Vec<_>>();
    fs::write(&input, &original).expect("write overwrite input");
    fs::write(&archive, b"ARCHIVE_SENTINEL").expect("write archive sentinel");

    let mut compress = CompressRequest::new(&input, &archive);
    compress.overwrite = true;
    application::compress(compress).expect("overwrite archive through public API");
    assert_ne!(
        fs::read(&archive).expect("read replaced archive"),
        b"ARCHIVE_SENTINEL"
    );

    fs::write(&restored, b"OUTPUT_SENTINEL").expect("write output sentinel");
    let mut decompress = DecompressRequest::new(&archive, &restored);
    decompress.overwrite = true;
    application::decompress(decompress).expect("overwrite restored output through public API");
    assert_eq!(fs::read(restored).expect("read replaced output"), original);
}

#[test]
fn keep_partial_preserves_owned_output_after_corrupt_v2_failure() {
    let directory = tempfile::tempdir().expect("keep-partial application API directory");
    let input = directory.path().join("input.bin");
    let archive = directory.path().join("corrupt-v2.dpack");
    let restored = directory.path().join("corrupt-restored.bin");
    let original = (0u8..=255).cycle().take(12_345).collect::<Vec<_>>();
    fs::write(&input, original).expect("write keep-partial input");
    create_v2_archive(&input, &archive, 1_024);

    let mut corrupt = fs::read(&archive).expect("read v2 archive for corruption");
    let last = corrupt.last_mut().expect("v2 archive is non-empty");
    *last ^= 0xff;
    fs::write(&archive, corrupt).expect("write corrupt v2 archive");

    let mut request = DecompressRequest::new(&archive, &restored);
    request.keep_partial = true;
    assert!(application::decompress(request).is_err());
    assert!(!restored.exists());
    let partials = partial_files(directory.path());
    assert_eq!(partials.len(), 1, "expected one retained partial output");
    assert!(partials[0].is_file());
}
