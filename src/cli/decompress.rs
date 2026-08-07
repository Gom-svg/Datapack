use std::fs::File;
use std::io::{BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;

use crate::compression::planned::archive_mode_for_payload;
use crate::error::{DatapackError, Result};
use crate::metadata::{DpackMetadata, PayloadKind};
use crate::storage;

use super::profile::{
    duration_ms, elapsed_ms, mb_per_second_u64, print_chunked_profile, print_direct_profile,
    DirectProfile,
};
use super::progress::{
    progress_phase, read_all_buffered_progress, ProgressWriter, IO_BUFFER_BYTES,
};
use super::validation::{
    operation_failed, optional_megabytes_to_bytes, validate_input_output_paths,
    validate_nonzero_megabyte_limit, validate_output_overwrite_policy,
};
use super::DecompressOptions;

pub(super) fn run(input: PathBuf, output: PathBuf, options: DecompressOptions) -> Result<()> {
    validate_input_output_paths(&input, &output)?;
    validate_output_overwrite_policy(&output, options.force)?;
    if let Some(max_memory_mb) = options.max_memory_mb {
        validate_nonzero_megabyte_limit("--max-memory-mb", max_memory_mb)?;
    }
    let output_existed = output.exists();
    decompress_command_inner(&input, &output, options)
        .map_err(|error| operation_failed("decompression", &input, &output, output_existed, error))
}

fn decompress_command_inner(input: &Path, output: &Path, options: DecompressOptions) -> Result<()> {
    let total_started = Instant::now();
    let archive_size = std::fs::metadata(input)?.len();
    let version =
        storage::archive_version_from_path(input).map_err(|error| DatapackError::ArchiveParse {
            path: input.to_path_buf(),
            reason: error.to_string(),
        })?;
    if version == storage::chunked::CHUNKED_VERSION {
        let transform_started = Instant::now();
        let stats = storage::chunked::decode_raw_zstd_chunked_file(
            input,
            output,
            storage::chunked::ChunkedDecompressOptions {
                verify: !options.no_verify,
                max_output_bytes: optional_megabytes_to_bytes(
                    "--max-output-mb",
                    options.max_output_mb,
                )?,
                max_chunks: options.max_chunks,
                max_memory_bytes: optional_megabytes_to_bytes(
                    "--max-memory-mb",
                    options.max_memory_mb,
                )?,
                force: options.force,
                keep_temp: options.keep_temp,
            },
        )?;
        let transform_elapsed = transform_started.elapsed();
        if options.profile {
            print_chunked_profile(
                "decompress",
                &stats,
                archive_size,
                stats.original_size_bytes,
                elapsed_ms(total_started),
                mb_per_second_u64(stats.original_size_bytes, transform_elapsed),
            );
        }
        return Ok(());
    }

    let file = File::open(input)?;
    let mut reader = BufReader::with_capacity(IO_BUFFER_BYTES, file);
    let metadata = storage::read_v1_archive_header(&mut reader).map_err(|error| {
        DatapackError::ArchiveParse {
            path: input.to_path_buf(),
            reason: error.to_string(),
        }
    })?;
    validate_v1_decompression_limits(&metadata, archive_size, options)?;
    if matches!(
        metadata.payload_kind,
        PayloadKind::RawZstd | PayloadKind::Plain | PayloadKind::Dictionary
    ) {
        let (mut temp_output, temp_file) =
            storage::output::TempOutput::create(output, options.keep_temp)?;
        let mut output_writer = BufWriter::with_capacity(IO_BUFFER_BYTES, temp_file);
        let mut progress_writer = ProgressWriter::new(
            &mut output_writer,
            "raw-zstd-decompress",
            Some(metadata.original_size),
        );
        let transform_started = Instant::now();
        let restored_size =
            storage::restore_raw_zstd_stream(&metadata, &mut reader, &mut progress_writer)?;
        progress_writer.flush()?;
        progress_writer.finish();
        drop(progress_writer);
        drop(output_writer);
        temp_output.commit(output, options.force)?;
        let transform_elapsed = transform_started.elapsed();
        if options.profile {
            print_direct_profile(&DirectProfile {
                operation: "decompress",
                archive_version: version,
                mode: archive_mode_for_payload(&metadata.payload_kind).as_str(),
                backend: "raw-zstd-streaming",
                verify_enabled: None,
                input_size_bytes: archive_size,
                output_size_bytes: restored_size,
                planning_ms: None,
                read_ms: None,
                transform_ms: duration_ms(transform_elapsed),
                write_ms: None,
                total_ms: elapsed_ms(total_started),
                throughput_mb_per_sec: mb_per_second_u64(restored_size, transform_elapsed),
            });
        }
        return Ok(());
    }

    let read_started = Instant::now();
    let archive_bytes = read_all_buffered_progress(input, "read archive")?;
    let read_ms = elapsed_ms(read_started);
    let archive =
        storage::decode_archive(&archive_bytes).map_err(|error| DatapackError::ArchiveParse {
            path: input.to_path_buf(),
            reason: error.to_string(),
        })?;

    let phase_started = Instant::now();
    let restored = storage::restore_archive(&archive)?;
    let transform_elapsed = phase_started.elapsed();
    progress_phase(
        "decompress+decode",
        archive.payload.len() as u64,
        Some(archive.payload.len() as u64),
        phase_started,
    );

    let phase_started = Instant::now();
    let (mut temp_output, temp_file) =
        storage::output::TempOutput::create(output, options.keep_temp)?;
    let mut writer = BufWriter::with_capacity(IO_BUFFER_BYTES, temp_file);
    writer.write_all(&restored)?;
    writer.flush()?;
    drop(writer);
    temp_output.commit(output, options.force)?;
    let write_ms = elapsed_ms(phase_started);
    progress_phase(
        "write restored",
        restored.len() as u64,
        Some(restored.len() as u64),
        phase_started,
    );
    if options.profile {
        print_direct_profile(&DirectProfile {
            operation: "decompress",
            archive_version: version,
            mode: archive_mode_for_payload(&archive.metadata.payload_kind).as_str(),
            backend: "zstd",
            verify_enabled: None,
            input_size_bytes: archive_size,
            output_size_bytes: restored.len() as u64,
            planning_ms: None,
            read_ms: Some(read_ms),
            transform_ms: duration_ms(transform_elapsed),
            write_ms: Some(write_ms),
            total_ms: elapsed_ms(total_started),
            throughput_mb_per_sec: mb_per_second_u64(restored.len() as u64, transform_elapsed),
        });
    }
    Ok(())
}

fn validate_v1_decompression_limits(
    metadata: &DpackMetadata,
    archive_size: u64,
    options: DecompressOptions,
) -> Result<()> {
    if let Some(maximum) = optional_megabytes_to_bytes("--max-output-mb", options.max_output_mb)? {
        if metadata.original_size > maximum {
            return Err(DatapackError::InvalidFormat(format!(
                "archive declares {} output bytes, exceeding --max-output-mb limit of {maximum} bytes",
                metadata.original_size
            )));
        }
    }

    let Some(maximum) = optional_megabytes_to_bytes("--max-memory-mb", options.max_memory_mb)?
    else {
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
        let io_bytes = u64::try_from(IO_BUFFER_BYTES)
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
