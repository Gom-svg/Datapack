use std::fs;
use std::sync::Arc;
use std::time::{Duration, Instant};

use datapack::application::{
    CancellationToken, OperationControl, ProgressEvent, ProgressPhase, ProgressState,
};
use datapack_desktop::adapter::{execute, CompressionChoice, Controller, Job, Outcome};
use datapack_desktop::model::*;
use datapack_desktop::{presentation, smoke};
use tempfile::TempDir;

fn run(job: Job) -> Outcome {
    execute(job, CancellationToken::new(), &mut |_: &ProgressEvent| {}).unwrap()
}
fn source(dir: &TempDir) -> std::path::PathBuf {
    let path = dir.path().join("demo.csv");
    fs::write(&path, smoke::DEMO).unwrap();
    path
}
fn compress(input: &std::path::Path, output: &std::path::Path, choice: CompressionChoice) -> Job {
    Job::Compress {
        input: input.into(),
        output: output.into(),
        choice,
    }
}
fn wait(controller: &mut Controller) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while controller.busy() {
        controller.poll();
        assert!(Instant::now() < deadline, "worker did not finish");
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn csv_analysis_is_product_language_and_honest_scope() {
    let dir = TempDir::new().unwrap();
    let input = source(&dir);
    let Outcome::Analyzed(a) = run(Job::Analyze { input }) else {
        panic!()
    };
    assert_eq!(a.format, "CSV");
    assert_eq!(a.delimiter, "Comma");
    assert_eq!(a.columns, Some(4));
    assert!(!a.sampled);
    assert_eq!(a.bytes_analyzed, a.source_bytes);
    assert!(["Structured", "Standard"].contains(&a.recommendation));
}

#[test]
fn tsv_and_pipe_analysis_detect_delimiters() {
    for (name, delimiter, label) in [("data.tsv", '\t', "Tab"), ("data.psv", '|', "Pipe")] {
        let dir = TempDir::new().unwrap();
        let input = dir.path().join(name);
        fs::write(
            &input,
            String::from_utf8(smoke::DEMO.to_vec())
                .unwrap()
                .replace(',', &delimiter.to_string()),
        )
        .unwrap();
        let Outcome::Analyzed(a) = run(Job::Analyze { input }) else {
            panic!()
        };
        assert_eq!(a.delimiter, label);
    }
}

#[test]
fn partial_analysis_discloses_sample_and_estimates() {
    let dir = TempDir::new().unwrap();
    let input = dir.path().join("sample.csv");
    fs::write(&input, format!("a,b\n{}", "North,Open\n".repeat(12000))).unwrap();
    let Outcome::Analyzed(a) = run(Job::Analyze { input }) else {
        panic!()
    };
    assert!(a.sampled);
    assert!(a.bytes_analyzed < a.source_bytes);
    let mut controller = Controller::default();
    controller.state = State::AnalysisReady;
    controller.analysis = Some(a);
    let view = presentation::view(&controller);
    assert!(view.body.contains("PARTIAL"));
    assert!(view.body.contains("not guaranteed"));
}

#[test]
fn recommended_and_chunked_complete_lifecycle_is_byte_exact() {
    let dir = TempDir::new().unwrap();
    smoke::run(&dir.path().join("certification")).unwrap();
    assert!(dir.path().join("certification/SMOKE-PASSED.txt").is_file());
}

#[test]
fn v1_restoration_never_claims_unperformed_hash_verification() {
    let dir = TempDir::new().unwrap();
    let input = source(&dir);
    let archive = dir.path().join("a.dpack");
    let Outcome::Compressed(c) = run(compress(&input, &archive, CompressionChoice::Recommended))
    else {
        panic!()
    };
    assert!(c.integrity.contains("Validate"));
    let Outcome::Decompressed(d) = run(Job::Decompress {
        archive: archive.clone(),
        output: dir.path().join("restored"),
    }) else {
        panic!()
    };
    assert!(d.integrity.contains("no embedded SHA-256"));
    let Outcome::Validated(v) = run(Job::Validate {
        archive,
        against: None,
    }) else {
        panic!()
    };
    assert!(v.valid);
    assert!(v.integrity.contains("supply the original"));
}

#[test]
fn validation_with_wrong_source_is_invalid() {
    let dir = TempDir::new().unwrap();
    let input = source(&dir);
    let archive = dir.path().join("a.dpack");
    run(compress(&input, &archive, CompressionChoice::Chunked));
    let wrong = dir.path().join("wrong.csv");
    fs::write(&wrong, b"different").unwrap();
    let Outcome::Validated(v) = run(Job::Validate {
        archive,
        against: Some(wrong),
    }) else {
        panic!()
    };
    assert!(!v.valid);
    assert_eq!(v.source_match, "Source does not match");
}

#[test]
fn malformed_archive_validation_is_an_invalid_result() {
    let dir = TempDir::new().unwrap();
    let archive = dir.path().join("bad.dpack");
    fs::write(&archive, b"broken").unwrap();
    let Outcome::Validated(v) = run(Job::Validate {
        archive,
        against: None,
    }) else {
        panic!()
    };
    assert!(!v.valid);
    assert!(v.checks.contains(&("Header", "FAILED")));
    assert!(!v.diagnostics.is_empty());
    let mut c = Controller::default();
    c.state = State::ValidationComplete(v);
    assert!(presentation::view(&c).title.starts_with("INVALID"));
}

#[test]
fn malformed_decompression_leaves_no_output_and_maps_context() {
    let dir = TempDir::new().unwrap();
    let archive = dir.path().join("bad.dpack");
    fs::write(&archive, b"broken").unwrap();
    let output = dir.path().join("out");
    let error = execute(
        Job::Decompress {
            archive,
            output: output.clone(),
        },
        CancellationToken::new(),
        &mut |_: &ProgressEvent| {},
    )
    .unwrap_err();
    let p = Problem::from_engine(&error);
    assert_eq!(p.code, error.code());
    assert_eq!(p.category, error.category().as_str());
    assert!(!p.context.is_empty());
    assert!(!output.exists());
}

#[test]
fn unsupported_analysis_can_recover_with_chunked_compression() {
    let dir = TempDir::new().unwrap();
    let input = dir.path().join("binary.bin");
    fs::write(&input, [0xff, 0x00, 0x80]).unwrap();
    let mut controller = Controller::default();
    controller
        .start(
            Job::Analyze {
                input: input.clone(),
            },
            Arc::new(|| {}),
        )
        .unwrap();
    wait(&mut controller);
    assert!(matches!(controller.state, State::Failed(_)));
    smoke::complete(
        &mut controller,
        compress(
            &input,
            &dir.path().join("a.dpack"),
            CompressionChoice::Chunked,
        ),
    )
    .unwrap();
    assert!(matches!(controller.state, State::CompressComplete(_)));
}

#[test]
fn malformed_structured_analysis_is_not_a_false_ready_state() {
    let dir = TempDir::new().unwrap();
    let input = dir.path().join("bad.csv");
    fs::write(&input, "a,b\n\"unterminated,b\n").unwrap();
    let mut c = Controller::default();
    c.start(Job::Analyze { input }, Arc::new(|| {})).unwrap();
    wait(&mut c);
    assert!(matches!(c.state, State::Failed(_)));
}

#[test]
fn existing_destination_and_input_alias_are_protected() {
    let dir = TempDir::new().unwrap();
    let input = source(&dir);
    let output = dir.path().join("keep.dpack");
    fs::write(&output, b"sentinel").unwrap();
    for path in [&output, &input] {
        let before = fs::read(path).unwrap();
        assert!(execute(
            compress(&input, path, CompressionChoice::Chunked),
            CancellationToken::new(),
            &mut |_: &ProgressEvent| {}
        )
        .is_err());
        assert_eq!(fs::read(path).unwrap(), before);
    }
}

#[test]
fn decompress_existing_destination_is_protected() {
    let dir = TempDir::new().unwrap();
    let input = source(&dir);
    let archive = dir.path().join("a.dpack");
    run(compress(&input, &archive, CompressionChoice::Chunked));
    let output = dir.path().join("restored");
    fs::write(&output, b"sentinel").unwrap();
    assert!(execute(
        Job::Decompress {
            archive,
            output: output.clone()
        },
        CancellationToken::new(),
        &mut |_: &ProgressEvent| {}
    )
    .is_err());
    assert_eq!(fs::read(output).unwrap(), b"sentinel");
}

#[test]
fn sparse_large_selection_reads_metadata_only() {
    let dir = TempDir::new().unwrap();
    let input = dir.path().join("large.csv");
    let file = fs::File::create(&input).unwrap();
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        let mut returned = 0;
        // SAFETY: live file handle, synchronous FSCTL, no input/output buffers.
        let result = unsafe {
            windows_sys::Win32::System::IO::DeviceIoControl(
                file.as_raw_handle(),
                windows_sys::Win32::System::Ioctl::FSCTL_SET_SPARSE,
                std::ptr::null(),
                0,
                std::ptr::null_mut(),
                0,
                &mut returned,
                std::ptr::null_mut(),
            )
        };
        assert_ne!(result, 0, "NTFS sparse-file marking failed");
    }
    file.set_len(16 * 1024 * 1024 * 1024).unwrap();
    let Outcome::Selected(file) = run(Job::Select {
        path: input,
        kind: FileKind::Data,
    }) else {
        panic!()
    };
    assert_eq!(file.bytes, 16 * 1024 * 1024 * 1024);
}

#[test]
fn directory_and_missing_selection_fail_without_stale_file() {
    let dir = TempDir::new().unwrap();
    let input = source(&dir);
    let mut c = Controller::default();
    smoke::complete(
        &mut c,
        Job::Select {
            path: input,
            kind: FileKind::Data,
        },
    )
    .unwrap();
    assert!(c.selected.is_some());
    for path in [dir.path().to_path_buf(), dir.path().join("missing")] {
        c.start(
            Job::Select {
                path,
                kind: FileKind::Data,
            },
            Arc::new(|| {}),
        )
        .unwrap();
        wait(&mut c);
        assert!(c.selected.is_none());
        assert!(matches!(c.state, State::Failed(_)));
    }
}

#[test]
fn operation_admission_rejects_double_launch_and_recovers() {
    let dir = TempDir::new().unwrap();
    let input = source(&dir);
    let mut c = Controller::default();
    c.start(
        Job::Analyze {
            input: input.clone(),
        },
        Arc::new(|| {}),
    )
    .unwrap();
    assert_eq!(
        c.start(Job::Analyze { input }, Arc::new(|| {}))
            .unwrap_err()
            .code,
        "desktop_busy"
    );
    wait(&mut c);
    assert!(matches!(c.state, State::AnalysisReady));
}

#[test]
fn worker_does_not_block_ui_poll_even_when_observer_is_paused() {
    let dir = TempDir::new().unwrap();
    let input = source(&dir);
    let (entered_tx, entered_rx) = std::sync::mpsc::sync_channel(1);
    let (release_tx, release_rx) = std::sync::mpsc::sync_channel(1);
    let receiver = std::sync::Mutex::new(release_rx);
    let first = std::sync::atomic::AtomicBool::new(true);
    let mut c = Controller::default();
    c.start(
        Job::Analyze { input },
        Arc::new(move || {
            if first.swap(false, std::sync::atomic::Ordering::SeqCst) {
                entered_tx.send(()).unwrap();
                receiver.lock().unwrap().recv().unwrap();
            }
        }),
    )
    .unwrap();
    entered_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    let started = Instant::now();
    c.poll();
    assert!(started.elapsed() < Duration::from_secs(1));
    assert!(c.busy());
    assert!(c.progress.is_some());
    c.cancel();
    assert!(matches!(c.state, State::Cancelling(_)));
    release_tx.send(()).unwrap();
    wait(&mut c);
    assert!(matches!(c.state, State::Cancelled(_)));
}

#[test]
fn early_cancellation_and_retry_use_fresh_tokens() {
    let dir = TempDir::new().unwrap();
    let input = source(&dir);
    let output = dir.path().join("a.dpack");
    let token = CancellationToken::new();
    token.cancel();
    assert!(execute(
        compress(&input, &output, CompressionChoice::Chunked),
        token,
        &mut |_: &ProgressEvent| {}
    )
    .unwrap_err()
    .is_cancelled());
    assert!(!output.exists());
    run(compress(&input, &output, CompressionChoice::Chunked));
    assert!(output.exists());
}

#[test]
fn mid_cancellation_cleans_partial_and_retry_succeeds() {
    let dir = TempDir::new().unwrap();
    let input = dir.path().join("data.bin");
    fs::File::create(&input)
        .unwrap()
        .set_len(17 * 1024 * 1024)
        .unwrap();
    let output = dir.path().join("out.dpack");
    let token = CancellationToken::new();
    let cancel = token.clone();
    let mut terminal = false;
    let mut advanced = false;
    let result = execute(
        compress(&input, &output, CompressionChoice::Chunked),
        token,
        &mut |e: &ProgressEvent| {
            if e.phase == ProgressPhase::Compressing && e.state == ProgressState::Advanced {
                advanced = true;
                cancel.cancel();
            }
            terminal |= e.is_terminal_success();
        },
    );
    assert!(advanced);
    assert!(result.unwrap_err().is_cancelled());
    assert!(!terminal);
    assert!(!output.exists());
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    run(compress(&input, &output, CompressionChoice::Chunked));
}

#[test]
fn late_cancel_after_commit_preserves_success() {
    let dir = TempDir::new().unwrap();
    let input = source(&dir);
    let output = dir.path().join("out.dpack");
    let token = CancellationToken::new();
    let cancel = token.clone();
    let result = execute(
        compress(&input, &output, CompressionChoice::Chunked),
        token,
        &mut |e: &ProgressEvent| {
            if e.is_terminal_success() {
                cancel.cancel();
            }
        },
    )
    .unwrap();
    assert!(matches!(result, Outcome::Compressed(_)));
    assert!(output.exists());
}

#[test]
fn controller_late_cancel_uses_returned_success() {
    let dir = TempDir::new().unwrap();
    let input = source(&dir);
    let (signal_tx, signal_rx) = std::sync::mpsc::sync_channel(8);
    let mut c = Controller::default();
    c.start(
        compress(
            &input,
            &dir.path().join("a.dpack"),
            CompressionChoice::Chunked,
        ),
        Arc::new(move || {
            let _ = signal_tx.try_send(());
        }),
    )
    .unwrap();
    // Wait for the worker to return without polling its completion into UI state.
    while signal_rx.recv_timeout(Duration::from_secs(10)).is_ok() {}
    c.cancel();
    wait(&mut c);
    assert!(matches!(c.state, State::CompressComplete(_)));
}

#[test]
fn progress_mapping_uses_only_engine_facts() {
    let dir = TempDir::new().unwrap();
    let input = source(&dir);
    let mut seen_unknown = false;
    let mut seen_known = false;
    datapack::application::analyze_with_control(
        datapack::application::AnalyzeRequest::new(input),
        OperationControl::new().with_progress(&mut |e: &ProgressEvent| {
            let progress = Progress::from(e);
            assert_eq!(progress.percent, e.percentage().map(|n| n as f32));
            seen_unknown |= progress.percent.is_none();
            seen_known |= progress.percent.is_some();
            assert!(!presentation::progress_text(Some(&progress)).is_empty());
        }),
    )
    .unwrap();
    assert!(seen_unknown && seen_known);
}

#[test]
fn calculation_handles_expansion_empty_and_zero_archive() {
    assert_eq!(reduction(100, 25), Some(75.0));
    assert_eq!(ratio(100, 25), Some(4.0));
    assert_eq!(reduction(100, 125), Some(-25.0));
    assert_eq!(reduction(0, 12), None);
    assert_eq!(ratio(100, 0), None);
    assert_eq!(ratio(0, 100), None);
}

#[test]
fn safe_proposals_keep_original_name_and_extension() {
    assert_eq!(
        proposed_output(std::path::Path::new("records.csv"), FileKind::Data),
        std::path::Path::new("records.csv.dpack")
    );
    assert_eq!(
        proposed_output(std::path::Path::new("records.csv.dpack"), FileKind::Archive),
        std::path::Path::new("records.csv.dpack.restored")
    );
}

#[test]
fn unicode_paths_work_through_the_complete_adapter() {
    let dir = TempDir::new().unwrap();
    let folder = dir.path().join("datos ñ 東京");
    fs::create_dir(&folder).unwrap();
    smoke::run(&folder.join("demostración")).unwrap();
}

#[test]
fn error_identity_is_not_derived_from_display_text() {
    let error = datapack::application::OperationError::Failed(
        datapack::error::DatapackError::InvalidCsv("specific contextual explanation".into()),
    );
    let problem = Problem::from_engine(&error);
    assert_eq!(problem.code, "invalid_csv");
    assert_eq!(problem.category, "format");
    assert!(problem.context.contains("specific contextual explanation"));
}

#[test]
fn destination_created_after_selection_is_still_protected() {
    let dir = TempDir::new().unwrap();
    let input = source(&dir);
    let output = proposed_output(&input, FileKind::Data);
    let mut c = Controller::default();
    smoke::complete(
        &mut c,
        Job::Select {
            path: input.clone(),
            kind: FileKind::Data,
        },
    )
    .unwrap();
    fs::write(&output, b"late sentinel").unwrap();
    c.start(
        compress(&input, &output, CompressionChoice::Chunked),
        Arc::new(|| {}),
    )
    .unwrap();
    wait(&mut c);
    assert!(matches!(c.state, State::Failed(_)));
    assert_eq!(fs::read(output).unwrap(), b"late sentinel");
}

#[test]
fn pre_cancel_applies_to_analyze_validate_and_decompress() {
    let dir = TempDir::new().unwrap();
    let input = source(&dir);
    let archive = dir.path().join("a.dpack");
    run(compress(&input, &archive, CompressionChoice::Chunked));
    for job in [
        Job::Analyze { input },
        Job::Validate {
            archive: archive.clone(),
            against: None,
        },
        Job::Decompress {
            archive,
            output: dir.path().join("restored"),
        },
    ] {
        let token = CancellationToken::new();
        token.cancel();
        assert!(execute(job, token, &mut |_: &ProgressEvent| {})
            .unwrap_err()
            .is_cancelled());
    }
    assert!(!dir.path().join("restored").exists());
}

#[test]
fn decompression_cancellation_cleans_partial_and_allows_retry() {
    let dir = TempDir::new().unwrap();
    let input = dir.path().join("data.bin");
    fs::File::create(&input)
        .unwrap()
        .set_len(17 * 1024 * 1024)
        .unwrap();
    let archive = dir.path().join("data.dpack");
    let output = dir.path().join("restored");
    run(compress(&input, &archive, CompressionChoice::Chunked));
    let token = CancellationToken::new();
    let cancel = token.clone();
    let mut advanced = false;
    let job = Job::Decompress {
        archive,
        output: output.clone(),
    };
    let result = execute(job.clone(), token, &mut |event: &ProgressEvent| {
        if event.phase == ProgressPhase::Decompressing && event.state == ProgressState::Advanced {
            advanced = true;
            cancel.cancel();
        }
        assert!(!event.is_terminal_success());
    });
    assert!(advanced);
    assert!(result.unwrap_err().is_cancelled());
    assert!(!output.exists());
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 2);
    run(job);
    assert_eq!(fs::metadata(output).unwrap().len(), 17 * 1024 * 1024);
}
