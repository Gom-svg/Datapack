use std::time::{Duration, Instant};

use crate::planning::ArchiveMode;
use crate::storage;

pub(super) struct DirectProfile<'a> {
    pub(super) operation: &'a str,
    pub(super) archive_version: u16,
    pub(super) mode: &'a str,
    pub(super) backend: &'a str,
    pub(super) verify_enabled: Option<bool>,
    pub(super) input_size_bytes: u64,
    pub(super) output_size_bytes: u64,
    pub(super) planning_ms: Option<u64>,
    pub(super) read_ms: Option<u64>,
    pub(super) transform_ms: u64,
    pub(super) write_ms: Option<u64>,
    pub(super) total_ms: u64,
    pub(super) throughput_mb_per_sec: f64,
}

pub(super) fn elapsed_ms(started: Instant) -> u64 {
    duration_ms(started.elapsed())
}

pub(super) fn duration_ms(duration: Duration) -> u64 {
    duration.as_millis() as u64
}

pub(super) fn mb_per_second(bytes: usize, duration: Duration) -> f64 {
    let seconds = duration.as_secs_f64();
    if seconds <= 0.0 {
        0.0
    } else {
        bytes as f64 / 1_048_576.0 / seconds
    }
}

pub(super) fn mb_per_second_u64(bytes: u64, duration: Duration) -> f64 {
    let seconds = duration.as_secs_f64();
    if seconds <= 0.0 {
        0.0
    } else {
        bytes as f64 / 1_048_576.0 / seconds
    }
}

pub(super) fn ratio(original_size: u64, compressed_size: u64) -> f64 {
    if compressed_size == 0 {
        0.0
    } else {
        original_size as f64 / compressed_size as f64
    }
}

pub(super) fn print_direct_profile(profile: &DirectProfile<'_>) {
    eprintln!("profile diagnostics:");
    eprintln!("  operation={}", profile.operation);
    eprintln!("  archive_version={}", profile.archive_version);
    eprintln!("  selected_mode={}", profile.mode);
    eprintln!("  backend={}", profile.backend);
    eprintln!("  input_size_bytes={}", profile.input_size_bytes);
    eprintln!("  output_size_bytes={}", profile.output_size_bytes);
    let compression_ratio = if profile.operation == "decompress" {
        ratio(profile.output_size_bytes, profile.input_size_bytes)
    } else {
        ratio(profile.input_size_bytes, profile.output_size_bytes)
    };
    eprintln!("  compression_ratio={compression_ratio:.4}");
    eprintln!(
        "  verify_enabled={}",
        profile
            .verify_enabled
            .map(|value| value.to_string())
            .unwrap_or_else(|| "not_stored_in_v1".to_string())
    );
    eprintln!(
        "  planning_ms={}",
        display_optional_u64(profile.planning_ms)
    );
    eprintln!("  read_ms={}", display_optional_u64(profile.read_ms));
    eprintln!("  transform_ms={}", profile.transform_ms);
    eprintln!("  write_ms={}", display_optional_u64(profile.write_ms));
    eprintln!("  total_elapsed_ms={}", profile.total_ms);
    eprintln!(
        "  throughput_mb_per_sec={:.3}",
        profile.throughput_mb_per_sec
    );
}

pub(super) fn print_chunked_profile(
    operation: &str,
    stats: &storage::chunked::ChunkedStats,
    input_size_bytes: u64,
    output_size_bytes: u64,
    total_elapsed_ms: u64,
    throughput_mb_per_sec: f64,
) {
    let profile = &stats.profile;
    eprintln!("profile diagnostics:");
    eprintln!("  operation={operation}");
    eprintln!("  archive_version={}", storage::chunked::CHUNKED_VERSION);
    eprintln!("  selected_mode={}", ArchiveMode::RawZstd.as_str());
    eprintln!("  backend={}", profile.backend);
    eprintln!("  input_size_bytes={input_size_bytes}");
    eprintln!("  output_size_bytes={output_size_bytes}");
    eprintln!(
        "  compression_ratio={:.4}",
        ratio(stats.original_size_bytes, stats.archive_size_bytes)
    );
    eprintln!("  chunk_count={}", stats.chunk_count);
    eprintln!(
        "  chunk_size_mb={:.3}",
        stats.chunk_size_target as f64 / 1_048_576.0
    );
    if operation == "compress" {
        eprintln!("  threads={}", profile.threads);
        eprintln!("  max_in_flight_chunks={}", profile.max_in_flight_chunks);
        eprintln!("  adaptive_level={}", profile.adaptive_level);
    }
    if operation == "decompress" {
        eprintln!("  verify_enabled={}", profile.verify_enabled);
    }
    eprintln!("  read_ms={}", profile.read_ms);
    eprintln!("  hash_ms={}", profile.hash_ms);
    eprintln!("  compress_ms={}", profile.compress_ms);
    eprintln!("  decompress_ms={}", profile.decompress_ms);
    eprintln!("  write_ms={}", profile.write_ms);
    eprintln!("  verify_ms={}", profile.verify_ms);
    eprintln!("  table_write_ms={}", profile.table_write_ms);
    eprintln!(
        "  average_chunk_{}_ms={:.3}",
        if operation == "compress" {
            "compress"
        } else {
            "decompress"
        },
        profile.average_chunk_transform_ms
    );
    eprintln!("  fastest_chunk_ms={:.3}", profile.fastest_chunk_ms);
    eprintln!("  slowest_chunk_ms={:.3}", profile.slowest_chunk_ms);
    eprintln!(
        "  average_compressed_chunk_size={}",
        profile.average_compressed_chunk_size
    );
    if operation == "compress" {
        let distribution = profile
            .zstd_level_distribution
            .iter()
            .map(|(level, count)| format!("{level}:{count}"))
            .collect::<Vec<_>>()
            .join(",");
        eprintln!("  zstd_level_distribution={distribution}");
    }
    eprintln!("  pipeline_elapsed_ms={}", profile.total_elapsed_ms);
    eprintln!("  total_elapsed_ms={total_elapsed_ms}");
    eprintln!("  throughput_mb_per_sec={throughput_mb_per_sec:.3}");
}

pub(super) fn display_optional_u64(value: Option<u64>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "unavailable".to_string())
}

pub(super) fn display_optional_f64(value: Option<f64>) -> String {
    value
        .map(|value| format!("{value:.3}"))
        .unwrap_or_else(|| "unavailable".to_string())
}
