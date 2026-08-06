use std::time::Duration;

use crate::error::{DatapackError, Result};
use crate::planning::ArchiveMode;

use super::super::BenchmarkOptions;

pub(super) fn benchmark_partial_reasons(
    options: &BenchmarkOptions,
    input_sampled: bool,
) -> Vec<&'static str> {
    let mut reasons = Vec::new();
    if options.no_zstd_baseline {
        push_unique_reason(&mut reasons, "zstd baseline skipped");
    }
    if options.no_roundtrip {
        push_unique_reason(
            &mut reasons,
            "round-trip decompression skipped; output identity not validated",
        );
    }
    if options.no_hash {
        push_unique_reason(&mut reasons, "SHA256 identity validation skipped");
    }
    if input_sampled {
        push_unique_reason(
            &mut reasons,
            "only the configured input prefix was benchmarked",
        );
    }
    reasons
}

fn push_unique_reason(reasons: &mut Vec<&'static str>, reason: &'static str) {
    if !reasons.contains(&reason) {
        reasons.push(reason);
    }
}

pub(super) fn benchmark_scope(options: &BenchmarkOptions, input_sampled: bool) -> &'static str {
    if input_sampled {
        "sampled"
    } else if benchmark_partial_reasons(options, false).is_empty() {
        "full"
    } else {
        "partial"
    }
}

pub(super) fn validation_status(
    hash_enabled: bool,
    input_sampled: bool,
    restored_hash: Option<&str>,
    original_hash: Option<&str>,
) -> Result<&'static str> {
    if !hash_enabled {
        return Ok("not_validated");
    }
    match (original_hash, restored_hash) {
        (Some(original), Some(restored)) if original == restored => {
            if input_sampled {
                Ok("partially_validated")
            } else {
                Ok("validated")
            }
        }
        (Some(_), Some(_)) => Err(DatapackError::InvalidFormat(
            "benchmark SHA256 validation failed: restored output does not match input".to_string(),
        )),
        _ => Err(DatapackError::InvalidFormat(
            "benchmark SHA256 validation could not be completed".to_string(),
        )),
    }
}

pub(super) fn compression_ratio(original_size: usize, compressed_size: usize) -> f64 {
    if compressed_size == 0 {
        0.0
    } else {
        original_size as f64 / compressed_size as f64
    }
}

pub(super) fn compression_ratio_u64(original_size: u64, compressed_size: u64) -> f64 {
    if compressed_size == 0 {
        0.0
    } else {
        original_size as f64 / compressed_size as f64
    }
}

#[derive(Debug, Clone)]
pub(super) struct BenchmarkMetrics {
    pub(super) source_size_bytes: u64,
    pub(super) measured_input_size_bytes: u64,
    pub(super) original_size_bytes: u64,
    pub(super) datapack_size_bytes: u64,
    pub(super) zstd_only_size_bytes: u64,
    pub(super) zstd_only_compression_time_ms: Option<f64>,
    pub(super) zstd_only_compression_mb_per_second: Option<f64>,
    pub(super) compression_ratio: f64,
    pub(super) zstd_only_ratio: f64,
    pub(super) selected_mode: ArchiveMode,
    pub(super) estimated_mode: ArchiveMode,
    pub(super) plan_was_correct: bool,
    pub(super) peak_memory_estimate_mb: Option<f32>,
    pub(super) planning_time_ms: u64,
    pub(super) columnar_candidate_error: Option<String>,
    pub(super) runs_used: usize,
    pub(super) total_elapsed_time_ms: u64,
    pub(super) compression_time_ms: f64,
    pub(super) decompression_time_ms: f64,
    pub(super) compression_input_mb_per_second: f64,
    pub(super) decompression_input_mb_per_second: f64,
    pub(super) roundtrip_sha256_match: bool,
    pub(super) datapack_beats_zstd: bool,
    pub(super) chunked_raw_zstd: Option<ChunkedBenchmarkMetrics>,
    pub(super) input_sampled: bool,
    pub(super) zstd_baseline_performed: bool,
    pub(super) roundtrip_performed: bool,
    pub(super) hash_performed: bool,
    pub(super) benchmark_scope: String,
    pub(super) validation_status: String,
    pub(super) partial_reasons: String,
    pub(super) no_roundtrip: bool,
    pub(super) no_hash: bool,
}

#[derive(Debug, Clone)]
pub(super) struct ChunkedBenchmarkMetrics {
    pub(super) backend: String,
    pub(super) chunk_size_mb: f64,
    pub(super) threads: usize,
    pub(super) max_in_flight_chunks: usize,
    pub(super) size_bytes: u64,
    pub(super) compression_ratio: f64,
    pub(super) compression_time_ms: f64,
    pub(super) compression_mb_per_second: f64,
    pub(super) decompression_time_ms: Option<f64>,
    pub(super) decompression_mb_per_second: Option<f64>,
    pub(super) roundtrip_sha256_match: Option<bool>,
}

pub(super) fn median_duration(values: &[Duration]) -> Duration {
    if values.is_empty() {
        return Duration::ZERO;
    }
    let mut sorted = values.to_vec();
    sorted.sort();
    sorted
        .get(sorted.len() / 2)
        .copied()
        .unwrap_or(Duration::ZERO)
}

#[derive(Debug, Default)]
pub(super) struct ProfileTimings {
    pub(super) planning_ms: Option<u64>,
    pub(super) read_input_ms: Option<u64>,
    pub(super) zstd_only_ms: Option<u64>,
    pub(super) datapack_compress_ms: Option<u64>,
    pub(super) datapack_decompress_ms: Option<u64>,
    pub(super) hash_ms: Option<u64>,
    pub(super) archive_write_ms: Option<u64>,
    pub(super) archive_read_ms: Option<u64>,
    pub(super) total_elapsed_ms: Option<u64>,
    pub(super) compression_mb_per_sec: Option<f64>,
    pub(super) decompression_mb_per_sec: Option<f64>,
}
