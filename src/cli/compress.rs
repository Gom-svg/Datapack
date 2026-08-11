use std::path::PathBuf;

use crate::application::{
    self, ArchiveModeV1, CompressRequest, CompressionBackend, CompressionFormat, CompressionMode,
    CompressionNotice, CompressionResultV1, V1CompressionOptions, V2CompressionOptions,
};
use crate::error::Result;
use crate::storage;

use super::chunked_options::{
    build_chunked_compress_options, validate_backend_support, validate_chunk_size,
    validate_compression_memory_limit, validate_max_in_flight_chunks, validate_thread_count,
};
use super::profile::{print_chunked_profile, print_direct_profile, DirectProfile};
use super::progress::TerminalProgressObserver;
use super::validation::{optional_megabytes_to_bytes, validate_nonzero_megabyte_limit};
use super::{ChunkedBackendArg, CompressMode, CompressOptions};

pub(super) fn run(input: PathBuf, output: PathBuf, options: CompressOptions) -> Result<()> {
    validate_cli_options(options)?;
    let request = build_request(input, output, options)?;
    let mut observer = TerminalProgressObserver::new();
    let result = application::compress_with_progress(request, &mut observer)?;
    for notice in &result.cli_notices {
        print_notice(notice);
    }
    for diagnostic in &result.diagnostics {
        if diagnostic.code == "OUTPUT_BACKUP_CLEANUP_FAILED" {
            eprintln!("warning: {}", diagnostic.message);
        }
    }
    if options.profile {
        print_profile(&result);
    }
    Ok(())
}

fn print_notice(notice: &CompressionNotice) {
    match notice {
        CompressionNotice::StructuredAnalysisUnavailable { reason } => eprintln!(
            "structured analysis unavailable ({reason}); using streaming RawZstd fallback."
        ),
        CompressionNotice::AnalysisLimited { limitation_codes } => eprintln!(
            "analysis limited ({}); using streaming RawZstd fallback.",
            limitation_codes.join(", ")
        ),
        CompressionNotice::DictionaryLimitApplied { column_name } => {
            eprintln!("Column '{column_name}' exceeded dictionary limit; switching to Plain.")
        }
        CompressionNotice::VerifyBest {
            selected_mode,
            saved_bytes,
        } => eprintln!(
            "verify-best: selected {selected_mode}, saved {saved_bytes} bytes over alternative."
        ),
        CompressionNotice::ColumnarCandidateError { error } => {
            eprintln!("columnar_candidate_error = {error:?}");
        }
    }
}

fn validate_cli_options(options: CompressOptions) -> Result<()> {
    if let Some(chunk_size_mb) = options.chunk_size_mb {
        validate_chunk_size(chunk_size_mb)?;
    }
    if let Some(threads) = options.threads {
        validate_thread_count(threads)?;
    }
    if let Some(max_in_flight) = options.max_in_flight_chunks {
        validate_max_in_flight_chunks(max_in_flight)?;
    }
    if let Some(backend) = options.backend {
        validate_backend_support(backend.storage_backend().as_str())?;
    }
    if let Some(max_memory_mb) = options.max_memory_mb {
        validate_nonzero_megabyte_limit("--max-memory-mb", max_memory_mb)?;
    }
    if options.uses_chunked() {
        let storage_options = build_chunked_compress_options(
            options.chunk_size_mb,
            options.threads,
            options.max_in_flight_chunks,
            options.backend,
            options.adaptive_level,
            false,
        )?;
        validate_compression_memory_limit(&storage_options, options.max_memory_mb)?;
    }
    Ok(())
}

fn build_request(
    input: PathBuf,
    output: PathBuf,
    options: CompressOptions,
) -> Result<CompressRequest> {
    let format = if options.uses_chunked() {
        let threads = options
            .threads
            .unwrap_or_else(storage::chunked::default_thread_count);
        let backend = options.backend.unwrap_or(ChunkedBackendArg::ChunkedRawZstd);
        let outer_threads = if backend == ChunkedBackendArg::ZstdMtExperimental {
            1
        } else {
            threads
        };
        CompressionFormat::V2(V2CompressionOptions {
            chunk_size_bytes: storage::chunked::chunk_size_mb_to_bytes(
                options
                    .chunk_size_mb
                    .unwrap_or(storage::chunked::DEFAULT_CHUNK_SIZE_MB),
            )?,
            threads,
            max_in_flight_chunks: options
                .max_in_flight_chunks
                .unwrap_or_else(|| storage::chunked::default_max_in_flight_chunks(outer_threads)),
            backend: match backend {
                ChunkedBackendArg::ChunkedRawZstd => CompressionBackend::ChunkedRawZstd,
                ChunkedBackendArg::ZstdMtExperimental => CompressionBackend::ZstdMtExperimental,
            },
            adaptive_level: options.adaptive_level,
            max_memory_bytes: optional_megabytes_to_bytes(
                "--max-memory-mb",
                options.max_memory_mb,
            )?,
        })
    } else {
        CompressionFormat::V1(V1CompressionOptions {
            mode: match options.mode {
                CompressMode::Fast => CompressionMode::Fast,
                CompressMode::Best => CompressionMode::Best,
            },
            sample_mb: options.sample_mb,
            verify_best: options.verify_best,
            max_dictionary_values: options.max_dictionary_values,
            max_dictionary_mb: options.max_dictionary_mb,
        })
    };

    Ok(CompressRequest {
        input,
        output,
        format,
        overwrite: options.force,
        keep_partial: options.keep_temp,
    })
}

fn print_profile(result: &CompressionResultV1) {
    if let Some(stats) = &result.chunked_stats {
        print_chunked_profile(
            "compress",
            stats,
            result.input_size_bytes,
            result.archive_size_bytes,
            result.profile.total_ms,
            result.profile.throughput_mib_per_second.unwrap_or(0.0),
        );
        return;
    }

    print_direct_profile(&DirectProfile {
        operation: "compress",
        archive_version: result.archive_version,
        mode: archive_mode_label(result.selected_mode),
        backend: result.backend.as_str(),
        verify_enabled: None,
        input_size_bytes: result.input_size_bytes,
        output_size_bytes: result.archive_size_bytes,
        planning_ms: result.profile.planning_ms,
        read_ms: result.profile.read_ms,
        transform_ms: result.profile.transform_ms,
        write_ms: result.profile.write_ms,
        total_ms: result.profile.total_ms,
        throughput_mb_per_sec: result.profile.throughput_mib_per_second.unwrap_or(0.0),
    });
}

fn archive_mode_label(mode: ArchiveModeV1) -> &'static str {
    match mode {
        ArchiveModeV1::RawZstd => "RawZstd",
        ArchiveModeV1::CsvColumnarDictionary => "CsvColumnarDictionary",
        ArchiveModeV1::ChunkedRawZstd => "RawZstd",
    }
}
