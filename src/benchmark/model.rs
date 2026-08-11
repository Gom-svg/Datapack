use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::analysis::DatasetAnalysis;
use crate::error::{DatapackError, Result};
use crate::planning::ArchiveMode;
use crate::storage::chunked::ChunkedBackend;

#[derive(Debug, Clone)]
pub(crate) struct BenchmarkRequest {
    pub(crate) input: PathBuf,
    pub(crate) keep_temp: bool,
    pub(crate) quick: bool,
    pub(crate) runs: usize,
    pub(crate) profile: bool,
    pub(crate) chunked: bool,
    pub(crate) chunk_size_mb: Option<u64>,
    pub(crate) threads: Option<usize>,
    pub(crate) max_in_flight_chunks: Option<usize>,
    pub(crate) backend: Option<ChunkedBackend>,
    pub(crate) adaptive_level: bool,
    pub(crate) no_zstd_baseline: bool,
    pub(crate) no_roundtrip: bool,
    pub(crate) no_hash: bool,
    pub(crate) estimate_only: bool,
    pub(crate) max_input_mb: Option<u64>,
}

impl BenchmarkRequest {
    pub(crate) fn uses_chunked(&self) -> bool {
        self.chunked
            || self.chunk_size_mb.is_some()
            || self.threads.is_some()
            || self.max_in_flight_chunks.is_some()
            || self.backend.is_some()
            || self.adaptive_level
    }
}

#[derive(Debug)]
pub(crate) enum BenchmarkEvent {
    Notice(BenchmarkNotice),
    Phase {
        name: &'static str,
        state: BenchmarkEventState,
        processed_bytes: u64,
        total_bytes: Option<u64>,
        started: Instant,
    },
}

#[derive(Debug)]
pub(crate) enum BenchmarkNotice {
    LargeInput { source_size_bytes: u64 },
    CommaEligibilityFallback,
    AnalysisLimited { limitation_codes: Vec<&'static str> },
    StreamingRawZstdExecution,
    CompressRun { run: usize, total_runs: usize },
    DecompressRun { run: usize, total_runs: usize },
    ChunkedCompressRun { run: usize, total_runs: usize },
    ChunkedDecompressRun { run: usize, total_runs: usize },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BenchmarkEventState {
    Advanced,
    Completed,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct BenchmarkArtifacts {
    pub(crate) datapack: Option<PathBuf>,
    pub(crate) restore: Option<PathBuf>,
    pub(crate) zstd: Option<PathBuf>,
    pub(crate) chunked: Option<PathBuf>,
    pub(crate) chunked_restore: Option<PathBuf>,
    pub(crate) chunked_sample: Option<PathBuf>,
}

#[derive(Debug)]
pub(crate) enum BenchmarkExecution {
    EstimateOnly {
        analysis: DatasetAnalysis,
        partial_reasons: Vec<&'static str>,
        total_elapsed_ms: u64,
        profile_timings: ProfileTimings,
    },
    Measured {
        metrics: Box<BenchmarkMetrics>,
        profile_timings: ProfileTimings,
        mode_label: String,
        artifacts: BenchmarkArtifacts,
    },
}

pub(crate) fn benchmark_partial_reasons(
    options: &BenchmarkRequest,
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

pub(crate) fn benchmark_scope(options: &BenchmarkRequest, input_sampled: bool) -> &'static str {
    if input_sampled {
        "sampled"
    } else if benchmark_partial_reasons(options, false).is_empty() {
        "full"
    } else {
        "partial"
    }
}

pub(crate) fn validation_status(
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

pub(crate) fn compression_ratio(original_size: usize, compressed_size: usize) -> f64 {
    if compressed_size == 0 {
        0.0
    } else {
        original_size as f64 / compressed_size as f64
    }
}

pub(crate) fn compression_ratio_u64(original_size: u64, compressed_size: u64) -> f64 {
    if compressed_size == 0 {
        0.0
    } else {
        original_size as f64 / compressed_size as f64
    }
}

#[derive(Debug, Clone)]
pub(crate) struct BenchmarkMetrics {
    pub(crate) source_size_bytes: u64,
    pub(crate) measured_input_size_bytes: u64,
    pub(crate) original_size_bytes: u64,
    pub(crate) datapack_size_bytes: u64,
    pub(crate) zstd_only_size_bytes: u64,
    pub(crate) zstd_only_compression_time_ms: Option<f64>,
    pub(crate) zstd_only_compression_mb_per_second: Option<f64>,
    pub(crate) compression_ratio: f64,
    pub(crate) zstd_only_ratio: f64,
    pub(crate) selected_mode: ArchiveMode,
    pub(crate) estimated_mode: ArchiveMode,
    pub(crate) plan_was_correct: bool,
    pub(crate) peak_memory_estimate_mb: Option<f32>,
    pub(crate) planning_time_ms: u64,
    pub(crate) columnar_candidate_error: Option<String>,
    pub(crate) runs_used: usize,
    pub(crate) total_elapsed_time_ms: u64,
    pub(crate) compression_time_ms: f64,
    pub(crate) decompression_time_ms: f64,
    pub(crate) compression_input_mb_per_second: f64,
    pub(crate) decompression_input_mb_per_second: f64,
    pub(crate) roundtrip_sha256_match: bool,
    pub(crate) datapack_beats_zstd: bool,
    pub(crate) chunked_raw_zstd: Option<ChunkedBenchmarkMetrics>,
    pub(crate) input_sampled: bool,
    pub(crate) zstd_baseline_performed: bool,
    pub(crate) roundtrip_performed: bool,
    pub(crate) hash_performed: bool,
    pub(crate) benchmark_scope: String,
    pub(crate) validation_status: String,
    pub(crate) partial_reasons: Vec<&'static str>,
    pub(crate) no_roundtrip: bool,
    pub(crate) no_hash: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct ChunkedBenchmarkMetrics {
    pub(crate) backend: String,
    pub(crate) chunk_size_mb: f64,
    pub(crate) threads: usize,
    pub(crate) max_in_flight_chunks: usize,
    pub(crate) size_bytes: u64,
    pub(crate) compression_ratio: f64,
    pub(crate) compression_time_ms: f64,
    pub(crate) compression_mb_per_second: f64,
    pub(crate) decompression_time_ms: Option<f64>,
    pub(crate) decompression_mb_per_second: Option<f64>,
    pub(crate) roundtrip_sha256_match: Option<bool>,
}

pub(crate) fn median_duration(values: &[Duration]) -> Duration {
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
pub(crate) struct ProfileTimings {
    pub(crate) planning_ms: Option<u64>,
    pub(crate) read_input_ms: Option<u64>,
    pub(crate) zstd_only_ms: Option<u64>,
    pub(crate) datapack_compress_ms: Option<u64>,
    pub(crate) datapack_decompress_ms: Option<u64>,
    pub(crate) hash_ms: Option<u64>,
    pub(crate) archive_write_ms: Option<u64>,
    pub(crate) archive_read_ms: Option<u64>,
    pub(crate) total_elapsed_ms: Option<u64>,
    pub(crate) compression_mb_per_sec: Option<f64>,
    pub(crate) decompression_mb_per_sec: Option<f64>,
}
