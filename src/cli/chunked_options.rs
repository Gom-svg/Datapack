use crate::error::{DatapackError, Result};
use crate::storage;

use super::validation::megabytes_to_bytes;
use super::ChunkedBackendArg;

pub(super) fn build_chunked_compress_options(
    chunk_size_mb: Option<u64>,
    threads: Option<usize>,
    max_in_flight_chunks: Option<usize>,
    backend: Option<ChunkedBackendArg>,
    adaptive_level: bool,
    profile: bool,
) -> Result<storage::chunked::ChunkedCompressOptions> {
    let chunk_size_bytes = storage::chunked::chunk_size_mb_to_bytes(
        chunk_size_mb.unwrap_or(storage::chunked::DEFAULT_CHUNK_SIZE_MB),
    )?;
    let threads = threads.unwrap_or_else(storage::chunked::default_thread_count);
    let backend = backend.unwrap_or(ChunkedBackendArg::ChunkedRawZstd);
    let outer_threads = if backend == ChunkedBackendArg::ZstdMtExperimental {
        1
    } else {
        threads
    };
    storage::chunked::ChunkedCompressOptions::new_with_max_in_flight(
        chunk_size_bytes,
        threads,
        max_in_flight_chunks
            .unwrap_or_else(|| storage::chunked::default_max_in_flight_chunks(outer_threads)),
        adaptive_level,
        profile,
    )
    .and_then(|options| options.with_backend(backend.storage_backend()))
}

pub(super) fn validate_chunk_size(mb: u64) -> Result<()> {
    storage::chunked::chunk_size_mb_to_bytes(mb).map(|_| ())
}

pub(super) fn validate_thread_count(n: usize) -> Result<()> {
    if n == 0 || n > storage::chunked::MAX_THREAD_COUNT {
        return Err(DatapackError::InvalidFormat(format!(
            "--threads must be between 1 and {}",
            storage::chunked::MAX_THREAD_COUNT
        )));
    }
    Ok(())
}

pub(super) fn validate_max_in_flight_chunks(n: usize) -> Result<()> {
    if n == 0 || n > storage::chunked::MAX_IN_FLIGHT_CHUNKS {
        return Err(DatapackError::InvalidFormat(format!(
            "--max-in-flight-chunks must be between 1 and {}",
            storage::chunked::MAX_IN_FLIGHT_CHUNKS
        )));
    }
    Ok(())
}

pub(super) fn validate_backend_support(name: &str) -> Result<()> {
    if matches!(name, "chunked-raw-zstd" | "zstd-mt-experimental") {
        Ok(())
    } else {
        Err(DatapackError::InvalidFormat(format!(
            "--backend '{name}' is unsupported; expected chunked-raw-zstd or zstd-mt-experimental"
        )))
    }
}

pub(super) fn validate_compression_memory_limit(
    options: &storage::chunked::ChunkedCompressOptions,
    maximum_mb: Option<u64>,
) -> Result<()> {
    let Some(maximum_mb) = maximum_mb else {
        return Ok(());
    };
    let maximum = megabytes_to_bytes("--max-memory-mb", maximum_mb)?;
    let chunk_size = u64::try_from(options.chunk_size_bytes).map_err(|_| {
        DatapackError::InvalidFormat("--chunk-size-mb exceeds u64 capacity".to_string())
    })?;
    let in_flight = u64::try_from(options.max_in_flight_chunks).map_err(|_| {
        DatapackError::InvalidFormat("--max-in-flight-chunks exceeds u64 capacity".to_string())
    })?;
    let required = chunk_size.checked_mul(in_flight).ok_or_else(|| {
        DatapackError::InvalidFormat(
            "--max-in-flight-chunks * --chunk-size-mb overflows u64".to_string(),
        )
    })?;
    if required > maximum {
        return Err(DatapackError::InvalidFormat(format!(
            "--max-in-flight-chunks * --chunk-size-mb requires at least {required} bytes, exceeding --max-memory-mb limit of {maximum} bytes"
        )));
    }
    Ok(())
}
