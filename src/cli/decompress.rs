use std::path::PathBuf;

use crate::application::{self, ArchiveModeV1, DecompressRequest, DecompressionResultV1};
use crate::error::Result;

use super::profile::{print_chunked_profile, print_direct_profile, DirectProfile};
use super::progress::TerminalProgressObserver;
use super::validation::{optional_megabytes_to_bytes, validate_nonzero_megabyte_limit};
use super::DecompressOptions;

pub(super) fn run(input: PathBuf, output: PathBuf, options: DecompressOptions) -> Result<()> {
    if let Some(max_memory_mb) = options.max_memory_mb {
        validate_nonzero_megabyte_limit("--max-memory-mb", max_memory_mb)?;
    }
    if let Some(max_output_mb) = options.max_output_mb {
        validate_nonzero_megabyte_limit("--max-output-mb", max_output_mb)?;
    }

    let request = DecompressRequest {
        archive: input,
        output,
        verify: !options.no_verify,
        max_output_bytes: optional_megabytes_to_bytes("--max-output-mb", options.max_output_mb)?,
        max_chunks: options.max_chunks,
        max_memory_bytes: optional_megabytes_to_bytes("--max-memory-mb", options.max_memory_mb)?,
        overwrite: options.force,
        keep_partial: options.keep_temp,
    };
    let mut observer = TerminalProgressObserver::new();
    let result = application::decompress_with_progress(request, &mut observer)?;
    if options.profile {
        print_profile(&result);
    }
    for diagnostic in &result.diagnostics {
        if diagnostic.code == "OUTPUT_BACKUP_CLEANUP_FAILED" {
            eprintln!("warning: {}", diagnostic.message);
        } else {
            eprintln!("{}: {}", diagnostic.code, diagnostic.message);
        }
    }
    Ok(())
}

fn print_profile(result: &DecompressionResultV1) {
    if let Some(stats) = &result.chunked_stats {
        print_chunked_profile(
            "decompress",
            stats,
            result.archive_size_bytes,
            result.restored_size_bytes,
            result.profile.total_ms,
            result.profile.throughput_mib_per_second.unwrap_or(0.0),
        );
        return;
    }

    print_direct_profile(&DirectProfile {
        operation: "decompress",
        archive_version: result.archive_version,
        mode: archive_mode_label(result.selected_mode),
        backend: result.backend.as_str(),
        verify_enabled: result.verified,
        input_size_bytes: result.archive_size_bytes,
        output_size_bytes: result.restored_size_bytes,
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
