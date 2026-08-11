use std::fs::File;
use std::io::{BufReader, BufWriter, Write};
use std::time::{Duration, Instant};

use crate::compression::planned::archive_mode_for_payload;
use crate::error::{DatapackError, Result};
use crate::metadata::{DpackMetadata, PayloadKind};
use crate::storage;

use super::io::{self, ObservedWriter};
use super::model::{
    ArchiveModeV1, CodecBackendV1, DecompressRequest, DecompressionResultV1, OperationDiagnosticV1,
    OperationProfileV1,
};
use super::progress::{
    OperationKind, ProgressEmitter, ProgressObserver, ProgressPhase, ProgressState,
};
use super::validation::{
    operation_failed, validate_input_output_paths, validate_output_overwrite_policy,
};

pub(super) fn decompress(request: DecompressRequest) -> Result<DecompressionResultV1> {
    let mut emitter = ProgressEmitter::silent(OperationKind::Decompress);
    run(request, &mut emitter)
}

pub(super) fn decompress_with_progress(
    request: DecompressRequest,
    observer: &mut dyn ProgressObserver,
) -> Result<DecompressionResultV1> {
    let mut emitter = ProgressEmitter::observed(OperationKind::Decompress, observer);
    run(request, &mut emitter)
}

fn run(
    request: DecompressRequest,
    emitter: &mut ProgressEmitter<'_>,
) -> Result<DecompressionResultV1> {
    validate_input_output_paths(&request.archive, &request.output)?;
    validate_output_overwrite_policy(&request.output, request.overwrite)?;
    if request.max_output_bytes == Some(0) {
        return Err(DatapackError::InvalidFormat(
            "max_output_bytes must be greater than zero".to_string(),
        ));
    }
    if request.max_chunks == Some(0) {
        return Err(DatapackError::InvalidFormat(
            "max_chunks must be greater than zero".to_string(),
        ));
    }
    if request.max_memory_bytes == Some(0) {
        return Err(DatapackError::InvalidFormat(
            "max_memory_bytes must be greater than zero".to_string(),
        ));
    }

    let output_existed = request.output.exists();
    decompress_inner(&request, emitter).map_err(|error| {
        operation_failed(
            "decompression",
            &request.archive,
            &request.output,
            output_existed,
            error,
        )
    })
}

fn decompress_inner(
    request: &DecompressRequest,
    emitter: &mut ProgressEmitter<'_>,
) -> Result<DecompressionResultV1> {
    let total_started = Instant::now();
    let archive_size = std::fs::metadata(&request.archive)?.len();
    let version = storage::archive_version_from_path(&request.archive).map_err(|error| {
        DatapackError::ArchiveParse {
            path: request.archive.clone(),
            reason: error.to_string(),
        }
    })?;

    if version == storage::chunked::CHUNKED_VERSION {
        return decompress_v2(request, archive_size, total_started, emitter);
    }

    let file = File::open(&request.archive)?;
    let mut reader = BufReader::with_capacity(io::IO_BUFFER_BYTES, file);
    let metadata = storage::read_v1_archive_header(&mut reader).map_err(|error| {
        DatapackError::ArchiveParse {
            path: request.archive.clone(),
            reason: error.to_string(),
        }
    })?;
    validate_v1_decompression_limits(&metadata, archive_size, request)?;

    if matches!(
        metadata.payload_kind,
        PayloadKind::RawZstd | PayloadKind::Plain | PayloadKind::Dictionary
    ) {
        return decompress_v1_streaming(
            request,
            archive_size,
            version,
            metadata,
            reader,
            total_started,
            emitter,
        );
    }

    let read_started = Instant::now();
    let archive_bytes = io::read_all(&request.archive, ProgressPhase::ReadingArchive, emitter)?;
    let read_ms = duration_ms(read_started.elapsed());
    let archive =
        storage::decode_archive(&archive_bytes).map_err(|error| DatapackError::ArchiveParse {
            path: request.archive.clone(),
            reason: error.to_string(),
        })?;

    emitter.started(ProgressPhase::Decompressing, Some(archive_size));
    let transform_started = Instant::now();
    let restored = storage::restore_archive(&archive)?;
    let transform_elapsed = transform_started.elapsed();
    emitter.completed(
        ProgressPhase::Decompressing,
        archive_size,
        Some(archive_size),
    );

    let write_started = Instant::now();
    let (mut temp_output, temp_file) =
        storage::output::TempOutput::create(&request.output, request.keep_partial)?;
    let mut writer = ObservedWriter::new(
        BufWriter::with_capacity(io::IO_BUFFER_BYTES, temp_file),
        ProgressPhase::WritingOutput,
        Some(restored.len() as u64),
        emitter,
    );
    writer.write_all(&restored)?;
    writer.flush()?;
    writer.finish();
    drop(writer);
    let cleanup_warning =
        temp_output.commit_with_cleanup_warning(&request.output, request.overwrite)?;
    let write_ms = duration_ms(write_started.elapsed());
    let total_ms = duration_ms(total_started.elapsed());

    Ok(DecompressionResultV1 {
        schema_version: 1,
        report_type: "decompression",
        archive_version: version,
        selected_mode: mode_for_payload(&archive.metadata.payload_kind),
        archive_size_bytes: archive_size,
        restored_size_bytes: restored.len() as u64,
        verified: None,
        backend: CodecBackendV1::Zstd,
        diagnostics: cleanup_diagnostics(cleanup_warning),
        profile: OperationProfileV1 {
            planning_ms: None,
            read_ms: Some(read_ms),
            transform_ms: duration_ms(transform_elapsed),
            write_ms: Some(write_ms),
            total_ms,
            throughput_mib_per_second: throughput(restored.len() as u64, transform_elapsed),
        },
        chunked_stats: None,
    })
}

fn decompress_v1_streaming(
    request: &DecompressRequest,
    archive_size: u64,
    version: u16,
    metadata: DpackMetadata,
    mut reader: BufReader<File>,
    total_started: Instant,
    emitter: &mut ProgressEmitter<'_>,
) -> Result<DecompressionResultV1> {
    let (mut temp_output, temp_file) =
        storage::output::TempOutput::create(&request.output, request.keep_partial)?;
    let mut writer = ObservedWriter::new(
        BufWriter::with_capacity(io::IO_BUFFER_BYTES, temp_file),
        ProgressPhase::WritingOutput,
        Some(metadata.original_size),
        emitter,
    );
    let transform_started = Instant::now();
    let restored_size = storage::restore_raw_zstd_stream(&metadata, &mut reader, &mut writer)?;
    writer.flush()?;
    writer.finish();
    drop(writer);
    let cleanup_warning =
        temp_output.commit_with_cleanup_warning(&request.output, request.overwrite)?;
    let transform_elapsed = transform_started.elapsed();

    Ok(DecompressionResultV1 {
        schema_version: 1,
        report_type: "decompression",
        archive_version: version,
        selected_mode: mode_for_payload(&metadata.payload_kind),
        archive_size_bytes: archive_size,
        restored_size_bytes: restored_size,
        verified: None,
        backend: CodecBackendV1::RawZstdStreaming,
        diagnostics: cleanup_diagnostics(cleanup_warning),
        profile: OperationProfileV1 {
            planning_ms: None,
            read_ms: None,
            transform_ms: duration_ms(transform_elapsed),
            write_ms: None,
            total_ms: duration_ms(total_started.elapsed()),
            throughput_mib_per_second: throughput(restored_size, transform_elapsed),
        },
        chunked_stats: None,
    })
}

fn decompress_v2(
    request: &DecompressRequest,
    archive_size: u64,
    total_started: Instant,
    emitter: &mut ProgressEmitter<'_>,
) -> Result<DecompressionResultV1> {
    emitter.started(ProgressPhase::Decompressing, None);
    let transform_started = Instant::now();
    let mut progress = |progress| {
        if let storage::chunked::ChunkedProgress::Decompression {
            chunks_completed,
            total_chunks,
            bytes_written,
            total_bytes,
        } = progress
        {
            emitter.emit(
                ProgressPhase::Decompressing,
                ProgressState::Advanced,
                bytes_written,
                Some(total_bytes),
                chunks_completed,
                Some(total_chunks),
            );
        }
    };
    let (stats, cleanup_warning) = storage::chunked::decode_raw_zstd_chunked_file_with_progress(
        &request.archive,
        &request.output,
        storage::chunked::ChunkedDecompressOptions {
            verify: request.verify,
            max_output_bytes: request.max_output_bytes,
            max_chunks: request.max_chunks,
            max_memory_bytes: request.max_memory_bytes,
            force: request.overwrite,
            keep_temp: request.keep_partial,
        },
        &mut progress,
    )?;
    let transform_elapsed = transform_started.elapsed();
    emitter.completed(
        ProgressPhase::Decompressing,
        stats.original_size_bytes,
        Some(stats.original_size_bytes),
    );

    Ok(DecompressionResultV1 {
        schema_version: 1,
        report_type: "decompression",
        archive_version: storage::chunked::CHUNKED_VERSION,
        selected_mode: ArchiveModeV1::ChunkedRawZstd,
        archive_size_bytes: archive_size,
        restored_size_bytes: stats.original_size_bytes,
        verified: Some(request.verify),
        backend: CodecBackendV1::V2RawZstdFrame,
        diagnostics: cleanup_diagnostics(cleanup_warning),
        profile: OperationProfileV1 {
            planning_ms: None,
            read_ms: Some(stats.profile.read_ms),
            transform_ms: stats.profile.decompress_ms,
            write_ms: Some(stats.profile.write_ms),
            total_ms: duration_ms(total_started.elapsed()),
            throughput_mib_per_second: throughput(stats.original_size_bytes, transform_elapsed),
        },
        chunked_stats: Some(stats),
    })
}

fn validate_v1_decompression_limits(
    metadata: &DpackMetadata,
    archive_size: u64,
    request: &DecompressRequest,
) -> Result<()> {
    if let Some(maximum) = request.max_output_bytes {
        if metadata.original_size > maximum {
            return Err(DatapackError::InvalidFormat(format!(
                "archive declares {} output bytes, exceeding --max-output-mb limit of {maximum} bytes",
                metadata.original_size
            )));
        }
    }

    let Some(maximum) = request.max_memory_bytes else {
        return Ok(());
    };
    let estimate = if matches!(metadata.payload_kind, PayloadKind::CsvColumnarDictionary) {
        metadata
            .original_size
            .checked_mul(18)
            .and_then(|value| value.checked_add(archive_size))
            .ok_or_else(|| {
                DatapackError::InvalidFormat(
                    "v1 decompression memory estimate overflowed u64".to_string(),
                )
            })?
    } else {
        let io_bytes = u64::try_from(io::IO_BUFFER_BYTES)
            .ok()
            .and_then(|value| value.checked_mul(2))
            .ok_or_else(|| {
                DatapackError::InvalidFormat(
                    "v1 decompression memory estimate overflowed u64".to_string(),
                )
            })?;
        io_bytes.checked_add(16 * 1024 * 1024).ok_or_else(|| {
            DatapackError::InvalidFormat(
                "v1 decompression memory estimate overflowed u64".to_string(),
            )
        })?
    };
    if estimate > maximum {
        return Err(DatapackError::InvalidFormat(format!(
            "estimated decompression memory {estimate} bytes exceeds --max-memory-mb limit of {maximum} bytes"
        )));
    }
    Ok(())
}

fn mode_for_payload(payload_kind: &PayloadKind) -> ArchiveModeV1 {
    match archive_mode_for_payload(payload_kind) {
        crate::planning::ArchiveMode::RawZstd => ArchiveModeV1::RawZstd,
        crate::planning::ArchiveMode::CsvColumnarDictionary => ArchiveModeV1::CsvColumnarDictionary,
    }
}

fn cleanup_diagnostics(warning: Option<String>) -> Vec<OperationDiagnosticV1> {
    warning.map_or_else(Vec::new, |message| {
        vec![OperationDiagnosticV1 {
            code: "OUTPUT_BACKUP_CLEANUP_FAILED".to_string(),
            message,
            column_index: None,
        }]
    })
}

fn duration_ms(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

fn throughput(bytes: u64, duration: Duration) -> Option<f64> {
    let seconds = duration.as_secs_f64();
    if bytes == 0 || seconds == 0.0 {
        None
    } else {
        Some(bytes as f64 / 1_048_576.0 / seconds)
    }
}
