use crate::benchmark::{
    self, BenchmarkArtifacts, BenchmarkEvent, BenchmarkEventState, BenchmarkExecution,
    ProfileTimings,
};
use crate::error::{DatapackError, Result};

use super::control::{uncontrolled, OperationContext, OperationControl, OperationResult};
use super::model::{
    ArchiveModeV1, BenchmarkArtifactsV1, BenchmarkProfileV1, BenchmarkReportV1, BenchmarkRequest,
    BenchmarkScopeV1, BenchmarkValidationStatusV1, ChunkedBenchmarkReportV1, V2CompressionOptions,
};
use super::progress::{OperationKind, ProgressObserver, ProgressPhase, ProgressState};

const MIB: u64 = 1024 * 1024;

pub(super) fn benchmark(request: BenchmarkRequest) -> Result<BenchmarkReportV1> {
    report(benchmark::execute(internal_request(request)?)?)
}

pub(super) fn benchmark_with_progress(
    request: BenchmarkRequest,
    observer: &mut dyn ProgressObserver,
) -> Result<BenchmarkReportV1> {
    let mut context = OperationContext::observed(OperationKind::Benchmark, observer);
    uncontrolled(run(request, &mut context))
}

pub(super) fn benchmark_with_control(
    request: BenchmarkRequest,
    control: OperationControl<'_>,
) -> OperationResult<BenchmarkReportV1> {
    let mut context = OperationContext::controlled(OperationKind::Benchmark, control);
    run(request, &mut context)
}

fn run(
    request: BenchmarkRequest,
    context: &mut OperationContext<'_>,
) -> OperationResult<BenchmarkReportV1> {
    let source_size = std::fs::metadata(&request.input)
        .ok()
        .map(|value| value.len());
    context.started(ProgressPhase::Benchmarking, source_size)?;
    let internal = internal_request(request)?;
    let mut active_phases = Vec::<&'static str>::new();
    let cancellation = context.cancellation().cloned();
    let execution = benchmark::execute_with_events_and_control(
        internal,
        &mut |event| {
            let BenchmarkEvent::Phase {
                name,
                state,
                processed_bytes,
                total_bytes,
                ..
            } = event
            else {
                return;
            };
            let phase = benchmark_phase(name);
            if !active_phases.contains(&name) {
                context.emit_unchecked(phase, ProgressState::Started, 0, total_bytes, 0, None);
                if state == BenchmarkEventState::Advanced {
                    active_phases.push(name);
                }
            }
            context.emit_unchecked(
                phase,
                match state {
                    BenchmarkEventState::Advanced => ProgressState::Advanced,
                    BenchmarkEventState::Completed => ProgressState::Completed,
                },
                processed_bytes,
                total_bytes,
                0,
                None,
            );
            if state == BenchmarkEventState::Completed {
                active_phases.retain(|active| *active != name);
            }
        },
        cancellation.as_ref(),
    )?;
    context.checkpoint()?;
    let report = report(execution)?;
    context.completed(
        ProgressPhase::Benchmarking,
        report.measured_input_size_bytes,
        Some(report.source_size_bytes),
    )?;
    context.succeeded();
    Ok(report)
}

fn internal_request(request: BenchmarkRequest) -> Result<benchmark::BenchmarkRequest> {
    let chunked = request.chunked.as_ref().map(chunked_options).transpose()?;
    Ok(benchmark::BenchmarkRequest {
        input: request.input,
        keep_temp: request.keep_artifacts,
        quick: request.quick,
        runs: request.runs,
        profile: false,
        chunked: chunked.is_some(),
        chunk_size_mb: chunked.as_ref().map(|value| value.chunk_size_mb),
        threads: chunked.as_ref().map(|value| value.threads),
        max_in_flight_chunks: chunked.as_ref().map(|value| value.max_in_flight_chunks),
        backend: chunked.as_ref().map(|value| value.backend),
        adaptive_level: chunked.as_ref().is_some_and(|value| value.adaptive_level),
        no_zstd_baseline: !request.include_zstd_baseline,
        no_roundtrip: !request.roundtrip,
        no_hash: !request.hash,
        estimate_only: request.estimate_only,
        max_input_mb: request.max_input_mb,
    })
}

struct InternalChunkedOptions {
    chunk_size_mb: u64,
    threads: usize,
    max_in_flight_chunks: usize,
    backend: crate::storage::chunked::ChunkedBackend,
    adaptive_level: bool,
}

fn chunked_options(options: &V2CompressionOptions) -> Result<InternalChunkedOptions> {
    if options.max_memory_bytes.is_some() {
        return Err(DatapackError::InvalidFormat(
            "benchmark chunked max_memory_bytes is not supported by the legacy benchmark methodology"
                .to_string(),
        ));
    }
    let bytes = u64::try_from(options.chunk_size_bytes).map_err(|_| {
        DatapackError::InvalidFormat("benchmark chunk_size_bytes exceeds u64 capacity".to_string())
    })?;
    if bytes == 0 || bytes % MIB != 0 {
        return Err(DatapackError::InvalidFormat(
            "benchmark chunk_size_bytes must be a positive whole number of MiB".to_string(),
        ));
    }
    Ok(InternalChunkedOptions {
        chunk_size_mb: bytes / MIB,
        threads: options.threads,
        max_in_flight_chunks: options.max_in_flight_chunks,
        backend: options.backend.storage_backend(),
        adaptive_level: options.adaptive_level,
    })
}

fn report(execution: BenchmarkExecution) -> Result<BenchmarkReportV1> {
    Ok(match execution {
        BenchmarkExecution::EstimateOnly {
            analysis,
            partial_reasons,
            total_elapsed_ms,
            profile_timings,
        } => BenchmarkReportV1 {
            schema_version: 1,
            report_type: "benchmark",
            estimate_only: true,
            source_size_bytes: analysis.facts.source_size_bytes,
            measured_input_size_bytes: analysis.facts.coverage.bytes_read,
            scope: BenchmarkScopeV1::EstimateOnly,
            validation_status: BenchmarkValidationStatusV1::NotValidated,
            input_sampled: analysis.facts.coverage.bytes_read < analysis.facts.source_size_bytes,
            zstd_baseline_performed: false,
            roundtrip_performed: false,
            hash_performed: false,
            estimated_mode: archive_mode(analysis.plan.archive_mode),
            selected_mode: None,
            plan_was_correct: None,
            peak_memory_estimate_mb: Some(analysis.plan.estimated_memory_mb),
            planning_time_ms: analysis.plan.planning_time_ms,
            runs_used: 0,
            datapack_size_bytes: None,
            zstd_size_bytes: None,
            compression_ratio: None,
            zstd_ratio: None,
            compression_time_ms: None,
            compression_mib_per_second: None,
            decompression_time_ms: None,
            decompression_mib_per_second: None,
            zstd_compression_time_ms: None,
            zstd_compression_mib_per_second: None,
            roundtrip_sha256_match: None,
            datapack_beats_zstd: None,
            columnar_candidate_error: None,
            total_elapsed_time_ms: total_elapsed_ms,
            partial_reasons: partial_reasons.into_iter().map(str::to_string).collect(),
            chunked: None,
            artifacts: BenchmarkArtifactsV1::default(),
            profile: benchmark_profile(profile_timings),
        },
        BenchmarkExecution::Measured {
            metrics,
            profile_timings,
            artifacts,
            ..
        } => {
            let metrics = *metrics;
            let scope = benchmark_scope(&metrics.benchmark_scope)?;
            let validation_status = benchmark_validation_status(&metrics.validation_status)?;
            BenchmarkReportV1 {
                schema_version: 1,
                report_type: "benchmark",
                estimate_only: false,
                source_size_bytes: metrics.source_size_bytes,
                measured_input_size_bytes: metrics.measured_input_size_bytes,
                scope,
                validation_status,
                input_sampled: metrics.input_sampled,
                zstd_baseline_performed: metrics.zstd_baseline_performed,
                roundtrip_performed: metrics.roundtrip_performed,
                hash_performed: metrics.hash_performed,
                estimated_mode: archive_mode(metrics.estimated_mode),
                selected_mode: Some(archive_mode(metrics.selected_mode)),
                plan_was_correct: Some(metrics.plan_was_correct),
                peak_memory_estimate_mb: metrics.peak_memory_estimate_mb,
                planning_time_ms: metrics.planning_time_ms,
                runs_used: metrics.runs_used,
                datapack_size_bytes: Some(metrics.datapack_size_bytes),
                zstd_size_bytes: metrics
                    .zstd_baseline_performed
                    .then_some(metrics.zstd_only_size_bytes),
                compression_ratio: Some(metrics.compression_ratio),
                zstd_ratio: metrics
                    .zstd_baseline_performed
                    .then_some(metrics.zstd_only_ratio),
                compression_time_ms: Some(metrics.compression_time_ms),
                compression_mib_per_second: Some(metrics.compression_input_mb_per_second),
                decompression_time_ms: metrics
                    .roundtrip_performed
                    .then_some(metrics.decompression_time_ms),
                decompression_mib_per_second: metrics
                    .roundtrip_performed
                    .then_some(metrics.decompression_input_mb_per_second),
                zstd_compression_time_ms: metrics.zstd_only_compression_time_ms,
                zstd_compression_mib_per_second: metrics.zstd_only_compression_mb_per_second,
                roundtrip_sha256_match: metrics
                    .hash_performed
                    .then_some(metrics.roundtrip_sha256_match),
                datapack_beats_zstd: metrics
                    .zstd_baseline_performed
                    .then_some(metrics.datapack_beats_zstd),
                columnar_candidate_error: metrics.columnar_candidate_error,
                total_elapsed_time_ms: metrics.total_elapsed_time_ms,
                partial_reasons: metrics
                    .partial_reasons
                    .into_iter()
                    .map(str::to_string)
                    .collect(),
                chunked: metrics
                    .chunked_raw_zstd
                    .map(|chunked| ChunkedBenchmarkReportV1 {
                        backend: chunked.backend,
                        chunk_size_mb: chunked.chunk_size_mb,
                        threads: chunked.threads,
                        max_in_flight_chunks: chunked.max_in_flight_chunks,
                        size_bytes: chunked.size_bytes,
                        compression_ratio: chunked.compression_ratio,
                        compression_time_ms: chunked.compression_time_ms,
                        compression_mib_per_second: chunked.compression_mb_per_second,
                        decompression_time_ms: chunked.decompression_time_ms,
                        decompression_mib_per_second: chunked.decompression_mb_per_second,
                        roundtrip_sha256_match: chunked.roundtrip_sha256_match,
                    }),
                artifacts: benchmark_artifacts(artifacts),
                profile: benchmark_profile(profile_timings),
            }
        }
    })
}

fn benchmark_scope(value: &str) -> Result<BenchmarkScopeV1> {
    match value {
        "full" => Ok(BenchmarkScopeV1::Full),
        "partial" => Ok(BenchmarkScopeV1::Partial),
        "sampled" => Ok(BenchmarkScopeV1::Sampled),
        other => Err(DatapackError::InvalidFormat(format!(
            "benchmark engine returned unknown scope '{other}'"
        ))),
    }
}

fn benchmark_validation_status(value: &str) -> Result<BenchmarkValidationStatusV1> {
    match value {
        "not_validated" => Ok(BenchmarkValidationStatusV1::NotValidated),
        "partially_validated" => Ok(BenchmarkValidationStatusV1::PartiallyValidated),
        "validated" => Ok(BenchmarkValidationStatusV1::Validated),
        other => Err(DatapackError::InvalidFormat(format!(
            "benchmark engine returned unknown validation status '{other}'"
        ))),
    }
}

fn benchmark_artifacts(artifacts: BenchmarkArtifacts) -> BenchmarkArtifactsV1 {
    BenchmarkArtifactsV1 {
        datapack: artifacts.datapack,
        restored: artifacts.restore,
        zstd: artifacts.zstd,
        chunked: artifacts.chunked,
        chunked_restored: artifacts.chunked_restore,
        chunked_sample: artifacts.chunked_sample,
    }
}

fn benchmark_profile(profile: ProfileTimings) -> BenchmarkProfileV1 {
    BenchmarkProfileV1 {
        planning_ms: profile.planning_ms,
        read_input_ms: profile.read_input_ms,
        zstd_only_ms: profile.zstd_only_ms,
        datapack_compress_ms: profile.datapack_compress_ms,
        datapack_decompress_ms: profile.datapack_decompress_ms,
        hash_ms: profile.hash_ms,
        archive_write_ms: profile.archive_write_ms,
        archive_read_ms: profile.archive_read_ms,
        total_elapsed_ms: profile.total_elapsed_ms,
        compression_mib_per_second: profile.compression_mb_per_sec,
        decompression_mib_per_second: profile.decompression_mb_per_sec,
    }
}

fn archive_mode(mode: crate::planning::ArchiveMode) -> ArchiveModeV1 {
    match mode {
        crate::planning::ArchiveMode::RawZstd => ArchiveModeV1::RawZstd,
        crate::planning::ArchiveMode::CsvColumnarDictionary => ArchiveModeV1::CsvColumnarDictionary,
    }
}

fn benchmark_phase(name: &str) -> ProgressPhase {
    if name.contains("planning") {
        ProgressPhase::Planning
    } else if name.contains("read") {
        ProgressPhase::ReadingInput
    } else if name.contains("hash") {
        ProgressPhase::Hashing
    } else if name.contains("write archive") {
        ProgressPhase::WritingArchive
    } else if name.contains("decompress") {
        ProgressPhase::Decompressing
    } else if name.contains("compress") || name == "zstd-only" {
        ProgressPhase::Compressing
    } else {
        ProgressPhase::Benchmarking
    }
}
