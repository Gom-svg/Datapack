use std::fs::File;
use std::io::{BufReader, BufWriter, Write};
use std::path::Path;
use std::time::{Duration, Instant};

use crate::analysis;
use crate::compression::planned::{encode_best_archive, encode_for_plan_detailed};
use crate::error::{DatapackError, Result};
use crate::planning::{self, ArchiveMode, ColumnExecutionPlan};
use crate::storage;

use super::io::{self, ObservedReader, ObservedWriter, IO_BUFFER_BYTES};
use super::model::{
    ArchiveModeV1, CodecBackendV1, CompressRequest, CompressionBackend, CompressionFormat,
    CompressionMode, CompressionNotice, CompressionResultV1, OperationDiagnosticV1,
    OperationProfileV1, V1CompressionOptions, V2CompressionOptions,
};
use super::progress::{
    OperationKind, ProgressEmitter, ProgressObserver, ProgressPhase, ProgressState,
};
use super::validation::{
    operation_failed, validate_input_output_paths, validate_output_overwrite_policy,
};

const ANALYSIS_LIMITED_FALLBACK_CODE: &str = "ANALYSIS_LIMITED_RAW_ZSTD_FALLBACK";
const DICTIONARY_LIMIT_APPLIED_CODE: &str = "DICTIONARY_LIMIT_APPLIED";
const STRUCTURED_ENCODER_FALLBACK_CODE: &str = "STRUCTURED_ENCODER_FALLBACK";
const OUTPUT_BACKUP_CLEANUP_FAILED_CODE: &str = "OUTPUT_BACKUP_CLEANUP_FAILED";

pub(super) fn compress(request: CompressRequest) -> Result<CompressionResultV1> {
    let mut emitter = ProgressEmitter::silent(OperationKind::Compress);
    compress_entry(request, &mut emitter)
}

pub(super) fn compress_with_progress(
    request: CompressRequest,
    observer: &mut dyn ProgressObserver,
) -> Result<CompressionResultV1> {
    let mut emitter = ProgressEmitter::observed(OperationKind::Compress, observer);
    compress_entry(request, &mut emitter)
}

fn compress_entry(
    request: CompressRequest,
    emitter: &mut ProgressEmitter<'_>,
) -> Result<CompressionResultV1> {
    validate_input_output_paths(&request.input, &request.output)?;
    validate_output_overwrite_policy(&request.output, request.overwrite)?;
    let chunked_options = match &request.format {
        CompressionFormat::V1(_) => None,
        CompressionFormat::V2(options) => Some(build_chunked_options(
            options,
            request.overwrite,
            request.keep_partial,
        )?),
    };
    let output_existed = request.output.exists();
    let input = request.input.clone();
    let output = request.output.clone();
    compress_inner(request, chunked_options, emitter)
        .map_err(|error| operation_failed("compression", &input, &output, output_existed, error))
}

fn compress_inner(
    request: CompressRequest,
    chunked_options: Option<storage::chunked::ChunkedCompressOptions>,
    emitter: &mut ProgressEmitter<'_>,
) -> Result<CompressionResultV1> {
    let total_started = Instant::now();
    let CompressRequest {
        input,
        output,
        format,
        overwrite,
        keep_partial,
    } = request;
    let result = match format {
        CompressionFormat::V1(options) => compress_v1(
            &input,
            &output,
            overwrite,
            keep_partial,
            options,
            total_started,
            emitter,
        )?,
        CompressionFormat::V2(options) => compress_v2(
            &input,
            &output,
            chunked_options.ok_or_else(|| {
                DatapackError::InvalidFormat(
                    "validated v2 compression options were unavailable".to_string(),
                )
            })?,
            compression_backend(options.backend),
            total_started,
            emitter,
        )?,
    };
    emitter.succeeded();
    Ok(result)
}

fn compress_v2(
    input: &Path,
    output: &Path,
    options: storage::chunked::ChunkedCompressOptions,
    backend: CodecBackendV1,
    total_started: Instant,
    emitter: &mut ProgressEmitter<'_>,
) -> Result<CompressionResultV1> {
    let input_size = std::fs::metadata(input)?.len();
    let chunk_size = u64::try_from(options.chunk_size_bytes).map_err(|_| {
        DatapackError::InvalidFormat("chunk_size_bytes exceeds u64 capacity".to_string())
    })?;
    let total_chunks = if input_size == 0 {
        0
    } else {
        input_size.div_ceil(chunk_size)
    };
    emitter.started_with_items(
        ProgressPhase::Compressing,
        Some(input_size),
        Some(total_chunks),
    );
    let transform_started = Instant::now();
    let (stats, cleanup_warning) = storage::chunked::encode_raw_zstd_chunked_file_with_progress(
        input,
        output,
        options,
        &mut |event| {
            if let storage::chunked::ChunkedProgress::Compression {
                chunks_written,
                total_chunks,
                bytes_written,
                total_bytes,
                ..
            } = event
            {
                emitter.emit(
                    ProgressPhase::Compressing,
                    ProgressState::Advanced,
                    bytes_written,
                    Some(total_bytes),
                    chunks_written,
                    Some(total_chunks),
                );
            }
        },
    )?;
    let transform_elapsed = transform_started.elapsed();
    emitter.completed_with_items(
        ProgressPhase::Compressing,
        input_size,
        Some(input_size),
        stats.chunk_count,
        Some(stats.chunk_count),
    );

    let mut diagnostics = Vec::new();
    if let Some(warning) = cleanup_warning {
        diagnostics.push(diagnostic(OUTPUT_BACKUP_CLEANUP_FAILED_CODE, warning, None));
    }

    Ok(CompressionResultV1 {
        schema_version: 1,
        report_type: "compression",
        archive_version: storage::chunked::CHUNKED_VERSION,
        selected_mode: ArchiveModeV1::ChunkedRawZstd,
        input_size_bytes: input_size,
        archive_size_bytes: stats.archive_size_bytes,
        backend,
        diagnostics,
        profile: OperationProfileV1 {
            planning_ms: None,
            read_ms: Some(stats.profile.read_ms),
            transform_ms: duration_ms(transform_elapsed),
            write_ms: None,
            total_ms: elapsed_ms(total_started),
            throughput_mib_per_second: throughput_mib_per_second(input_size, transform_elapsed),
        },
        cli_notices: Vec::new(),
        chunked_stats: Some(stats),
    })
}

fn compress_v1(
    input: &Path,
    output: &Path,
    overwrite: bool,
    keep_partial: bool,
    options: V1CompressionOptions,
    total_started: Instant,
    emitter: &mut ProgressEmitter<'_>,
) -> Result<CompressionResultV1> {
    let mut diagnostics = Vec::new();
    let mut cli_notices = Vec::new();
    let planning_started = Instant::now();
    emitter.started(ProgressPhase::Planning, None);
    let analysis = match analysis::analyze_cli_path(input, options.sample_mb) {
        Ok(analysis) => analysis,
        Err(DatapackError::InvalidCsv(reason)) => {
            let planning_ms = elapsed_ms(planning_started);
            emitter.completed(ProgressPhase::Planning, 0, None);
            cli_notices.push(CompressionNotice::StructuredAnalysisUnavailable { reason });
            diagnostics.push(diagnostic(
                analysis::STRUCTURED_ANALYSIS_UNAVAILABLE_CODE,
                "Structured analysis was unavailable; compression used RawZstd.",
                None,
            ));
            return compress_raw_zstd_streaming(
                input,
                output,
                overwrite,
                keep_partial,
                total_started,
                planning_ms,
                diagnostics,
                cli_notices,
                emitter,
            );
        }
        Err(error) => return Err(error),
    };
    let delimiter = analysis.structured_delimiter();
    let planning_ms = elapsed_ms(planning_started);
    emitter.completed(
        ProgressPhase::Planning,
        analysis.facts.coverage.bytes_analyzed,
        Some(
            analysis
                .facts
                .coverage
                .scope_size_bytes
                .min(analysis.facts.coverage.max_bytes),
        ),
    );

    if analysis.requires_raw_fallback() {
        let limitation_codes = analysis
            .facts
            .limitations
            .iter()
            .map(|limitation| limitation.code().to_string())
            .collect::<Vec<_>>();
        cli_notices.push(CompressionNotice::AnalysisLimited { limitation_codes });
        let reason_code = analysis::archive_reason_code(&analysis.plan.reason)
            .unwrap_or(ANALYSIS_LIMITED_FALLBACK_CODE);
        diagnostics.push(diagnostic(reason_code, analysis.plan.reason.clone(), None));
        for limitation in &analysis.facts.limitations {
            diagnostics.push(diagnostic(
                limitation.code(),
                limitation.message(analysis.facts.parser),
                None,
            ));
        }
        return compress_raw_zstd_streaming(
            input,
            output,
            overwrite,
            keep_partial,
            total_started,
            planning_ms,
            diagnostics,
            cli_notices,
            emitter,
        );
    }

    let mut plan = analysis.plan.clone();
    for column_name in planning::apply_dictionary_limits(
        &mut plan,
        &analysis.columns,
        options.max_dictionary_values,
        options.max_dictionary_mb,
    ) {
        cli_notices.push(CompressionNotice::DictionaryLimitApplied { column_name });
    }
    diagnostics.extend(
        plan.columns
            .iter()
            .filter(|column| column.reason == "Dictionary limit exceeded; switched to Plain")
            .map(|column| {
                diagnostic(
                    DICTIONARY_LIMIT_APPLIED_CODE,
                    "The configured dictionary limit changed this column to Plain execution.",
                    Some(column.column_index),
                )
            }),
    );

    let compares_candidates = options.verify_best
        || (options.mode == CompressionMode::Best && plan.estimated_savings_percent < 15.0);
    if plan.archive_mode == ArchiveMode::RawZstd && !compares_candidates {
        return compress_raw_zstd_streaming(
            input,
            output,
            overwrite,
            keep_partial,
            total_started,
            planning_ms,
            diagnostics,
            cli_notices,
            emitter,
        );
    }

    let execution_plan = ColumnExecutionPlan::from_compression_plan(
        &plan,
        options.max_dictionary_values,
        options.max_dictionary_mb,
    );
    let read_started = Instant::now();
    let bytes = io::read_all(input, ProgressPhase::ReadingInput, emitter)?;
    let read_ms = elapsed_ms(read_started);

    emitter.started(ProgressPhase::Compressing, Some(bytes.len() as u64));
    let transform_started = Instant::now();
    let (archive, selected_mode, columnar_error) = if options.verify_best {
        let (archive, selected, saved, error) =
            encode_best_archive(input, &bytes, delimiter, &execution_plan)?;
        cli_notices.push(CompressionNotice::VerifyBest {
            selected_mode: selected.as_str().to_string(),
            saved_bytes: saved,
        });
        if let Some(error) = &error {
            cli_notices.push(CompressionNotice::ColumnarCandidateError {
                error: error.clone(),
            });
        }
        (archive, selected, error)
    } else if options.mode == CompressionMode::Best && plan.estimated_savings_percent < 15.0 {
        let (archive, selected, _, error) =
            encode_best_archive(input, &bytes, delimiter, &execution_plan)?;
        (archive, selected, error)
    } else {
        encode_for_plan_detailed(input, &bytes, plan.archive_mode, delimiter, &execution_plan)?
    };
    let transform_elapsed = transform_started.elapsed();
    emitter.completed(
        ProgressPhase::Compressing,
        bytes.len() as u64,
        Some(bytes.len() as u64),
    );
    if let Some(error) = columnar_error {
        diagnostics.push(diagnostic(STRUCTURED_ENCODER_FALLBACK_CODE, error, None));
    }

    let (mut temp_output, temp_file) = storage::output::TempOutput::create(output, keep_partial)?;
    let writer = BufWriter::with_capacity(IO_BUFFER_BYTES, temp_file);
    let write_started = Instant::now();
    let mut writer = ObservedWriter::new(
        writer,
        ProgressPhase::WritingArchive,
        Some(archive.len() as u64),
        emitter,
    );
    writer.write_all(&archive)?;
    writer.flush()?;
    writer.finish();
    drop(writer);
    if let Some(warning) = temp_output.commit_with_cleanup_warning(output, overwrite)? {
        diagnostics.push(diagnostic(OUTPUT_BACKUP_CLEANUP_FAILED_CODE, warning, None));
    }
    let write_ms = elapsed_ms(write_started);

    Ok(CompressionResultV1 {
        schema_version: 1,
        report_type: "compression",
        archive_version: 1,
        selected_mode: application_archive_mode(selected_mode),
        input_size_bytes: bytes.len() as u64,
        archive_size_bytes: archive.len() as u64,
        backend: CodecBackendV1::Zstd,
        diagnostics,
        profile: OperationProfileV1 {
            planning_ms: Some(planning_ms),
            read_ms: Some(read_ms),
            transform_ms: duration_ms(transform_elapsed),
            write_ms: Some(write_ms),
            total_ms: elapsed_ms(total_started),
            throughput_mib_per_second: throughput_mib_per_second(
                bytes.len() as u64,
                transform_elapsed,
            ),
        },
        cli_notices,
        chunked_stats: None,
    })
}

#[allow(clippy::too_many_arguments)]
fn compress_raw_zstd_streaming(
    input: &Path,
    output: &Path,
    overwrite: bool,
    keep_partial: bool,
    total_started: Instant,
    planning_ms: u64,
    mut diagnostics: Vec<OperationDiagnosticV1>,
    cli_notices: Vec<CompressionNotice>,
    emitter: &mut ProgressEmitter<'_>,
) -> Result<CompressionResultV1> {
    let input_size = std::fs::metadata(input)?.len();
    let file = File::open(input)?;
    let reader = BufReader::with_capacity(IO_BUFFER_BYTES, file);
    let mut reader = ObservedReader::new(
        reader,
        ProgressPhase::Compressing,
        Some(input_size),
        emitter,
    );
    let (mut temp_output, temp_file) = storage::output::TempOutput::create(output, keep_partial)?;
    let mut writer = BufWriter::with_capacity(IO_BUFFER_BYTES, temp_file);
    let transform_started = Instant::now();
    storage::write_raw_zstd_archive_stream(input, input_size, &mut reader, &mut writer)?;
    writer.flush()?;
    let archive_size = writer.get_ref().metadata()?.len();
    drop(writer);
    reader.finish();
    drop(reader);
    if let Some(warning) = temp_output.commit_with_cleanup_warning(output, overwrite)? {
        diagnostics.push(diagnostic(OUTPUT_BACKUP_CLEANUP_FAILED_CODE, warning, None));
    }
    let transform_elapsed = transform_started.elapsed();

    Ok(CompressionResultV1 {
        schema_version: 1,
        report_type: "compression",
        archive_version: 1,
        selected_mode: ArchiveModeV1::RawZstd,
        input_size_bytes: input_size,
        archive_size_bytes: archive_size,
        backend: CodecBackendV1::RawZstdStreaming,
        diagnostics,
        profile: OperationProfileV1 {
            planning_ms: Some(planning_ms),
            read_ms: None,
            transform_ms: duration_ms(transform_elapsed),
            write_ms: None,
            total_ms: elapsed_ms(total_started),
            throughput_mib_per_second: throughput_mib_per_second(input_size, transform_elapsed),
        },
        cli_notices,
        chunked_stats: None,
    })
}

fn build_chunked_options(
    options: &V2CompressionOptions,
    overwrite: bool,
    keep_partial: bool,
) -> Result<storage::chunked::ChunkedCompressOptions> {
    let mut storage_options = storage::chunked::ChunkedCompressOptions::new_with_max_in_flight(
        options.chunk_size_bytes,
        options.threads,
        options.max_in_flight_chunks,
        options.adaptive_level,
        false,
    )?
    .with_backend(options.backend.storage_backend())?;

    if let Some(maximum) = options.max_memory_bytes {
        if maximum == 0 {
            return Err(DatapackError::InvalidFormat(
                "max_memory_bytes must be greater than zero".to_string(),
            ));
        }
        let chunk_size = u64::try_from(options.chunk_size_bytes).map_err(|_| {
            DatapackError::InvalidFormat("chunk_size_bytes exceeds u64 capacity".to_string())
        })?;
        let in_flight = u64::try_from(options.max_in_flight_chunks).map_err(|_| {
            DatapackError::InvalidFormat("max_in_flight_chunks exceeds u64 capacity".to_string())
        })?;
        let required = chunk_size.checked_mul(in_flight).ok_or_else(|| {
            DatapackError::InvalidFormat(
                "max_in_flight_chunks * chunk_size_bytes overflows u64".to_string(),
            )
        })?;
        if required > maximum {
            return Err(DatapackError::InvalidFormat(format!(
                "max_in_flight_chunks * chunk_size_bytes requires at least {required} bytes, exceeding max_memory_bytes limit of {maximum} bytes"
            )));
        }
    }

    storage_options.force = overwrite;
    storage_options.keep_temp = keep_partial;
    Ok(storage_options)
}

fn application_archive_mode(mode: ArchiveMode) -> ArchiveModeV1 {
    match mode {
        ArchiveMode::RawZstd => ArchiveModeV1::RawZstd,
        ArchiveMode::CsvColumnarDictionary => ArchiveModeV1::CsvColumnarDictionary,
    }
}

fn compression_backend(backend: CompressionBackend) -> CodecBackendV1 {
    match backend {
        CompressionBackend::ChunkedRawZstd => CodecBackendV1::ChunkedRawZstd,
        CompressionBackend::ZstdMtExperimental => CodecBackendV1::ZstdMtExperimental,
    }
}

fn diagnostic(
    code: impl Into<String>,
    message: impl Into<String>,
    column_index: Option<usize>,
) -> OperationDiagnosticV1 {
    OperationDiagnosticV1 {
        code: code.into(),
        message: message.into(),
        column_index,
    }
}

fn duration_ms(duration: Duration) -> u64 {
    duration.as_millis().min(u64::MAX as u128) as u64
}

fn elapsed_ms(started: Instant) -> u64 {
    duration_ms(started.elapsed())
}

fn throughput_mib_per_second(bytes: u64, duration: Duration) -> Option<f64> {
    let seconds = duration.as_secs_f64();
    if bytes == 0 || seconds == 0.0 {
        None
    } else {
        Some(bytes as f64 / 1_048_576.0 / seconds)
    }
}
