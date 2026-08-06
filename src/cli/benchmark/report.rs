use crate::analysis::DatasetAnalysis;

use super::super::profile::{display_optional_f64, display_optional_u64};
use super::model::{BenchmarkMetrics, ProfileTimings};

pub(super) fn print_estimate_only_table(
    analysis: &DatasetAnalysis,
    partial_reasons: &[&str],
    total_elapsed_ms: u64,
) {
    let measured_input_size = analysis.facts.coverage.bytes_read;
    let input_sampled = measured_input_size < analysis.facts.source_size_bytes;
    println!("{:<32} {:>18}  Description", "Metric", "Value");
    println!("{:-<32} {:-<18}  {:-<40}", "", "", "");
    println!(
        "{:<32} {:>18}  Full source file size",
        "source_size_bytes", analysis.facts.source_size_bytes
    );
    println!(
        "{:<32} {:>18}  Bytes inspected by the planning-only benchmark",
        "measured_input_size_bytes", measured_input_size
    );
    println!(
        "{:<32} {:>18}  Backward-compatible measured-size alias",
        "original_size_bytes", measured_input_size
    );
    println!(
        "{:<32} {:>18}  Planning-only benchmark scope",
        "benchmark_scope", "estimate_only"
    );
    println!(
        "{:<32} {:>18}  No output identity validation was performed",
        "validation_status", "not_validated"
    );
    println!(
        "{:<32} {:>18}  Why results are partial/non-validating",
        "partial_reasons",
        partial_reasons.join("; ")
    );
    println!(
        "{:<32} {:>18}  Whether planning inspected only a source prefix",
        "input_sampled", input_sampled
    );
    println!(
        "{:<32} {:>18}  Standalone zstd compression intentionally skipped",
        "zstd_baseline_performed", false
    );
    println!(
        "{:<32} {:>18}  DataPack decompression intentionally skipped",
        "roundtrip_performed", false
    );
    println!(
        "{:<32} {:>18}  SHA256 identity validation intentionally skipped",
        "hash_performed", false
    );
    println!(
        "{:<32} {:>18}  No archive was selected or produced",
        "selected_mode", "unavailable"
    );
    println!(
        "{:<32} {:>18}  Mode predicted from bounded planning sample",
        "estimated_mode",
        analysis.plan.archive_mode.as_str()
    );
    println!(
        "{:<32} {:>18}  No selected mode exists to compare with the estimate",
        "plan_was_correct", "unavailable"
    );
    println!(
        "{:<32} {:>18}  Sample bytes inspected by planner",
        "planning_sample_bytes", analysis.facts.coverage.bytes_read
    );
    println!(
        "{:<32} {:>18}  Planning time",
        "planning_time_ms", analysis.plan.planning_time_ms
    );
    println!(
        "{:<32} {:>18}  Compression intentionally skipped",
        "datapack_size_bytes", "unavailable"
    );
    println!(
        "{:<32} {:>18}  Standalone zstd compression intentionally skipped",
        "zstd_only_size_bytes", "unavailable"
    );
    println!(
        "{:<32} {:>18}  Chunked compression intentionally skipped",
        "chunked_raw_zstd_size_bytes", "unavailable"
    );
    println!(
        "{:<32} {:>18}  Chunked backend was not executed",
        "chunked_backend", "unavailable"
    );
    for metric in [
        "chunked_chunk_size_mb",
        "chunked_threads",
        "chunked_max_in_flight_chunks",
    ] {
        println!(
            "{:<32} {:>18}  Chunked backend was not executed",
            metric, "unavailable"
        );
    }
    for metric in [
        "compression_ratio",
        "zstd_only_ratio",
        "chunked_raw_zstd_ratio",
        "compression_time_ms",
        "decompression_time_ms",
        "zstd_only_compression_time_ms",
        "chunked_compression_time_ms",
        "chunked_decompression_time_ms",
        "compression_mb_per_sec",
        "decompression_mb_per_sec",
        "zstd_only_compression_mb_per_sec",
        "chunked_compression_mb_per_sec",
        "chunked_decompression_mb_per_sec",
        "roundtrip_sha256_match",
    ] {
        println!(
            "{:<32} {:>18}  Unavailable in estimate-only mode",
            metric, "unavailable"
        );
    }
    println!(
        "{:<32} {:>18}  Total estimate-only elapsed time",
        "total_elapsed_time_ms", total_elapsed_ms
    );
}

pub(super) fn print_estimate_only_json(
    analysis: &DatasetAnalysis,
    partial_reasons: &[&str],
    total_elapsed_ms: u64,
) {
    let measured_input_size = analysis.facts.coverage.bytes_read;
    let input_sampled = measured_input_size < analysis.facts.source_size_bytes;
    println!("{{");
    println!(
        "  \"source_size_bytes\": {},",
        analysis.facts.source_size_bytes
    );
    println!("  \"measured_input_size_bytes\": {},", measured_input_size);
    println!("  \"original_size_bytes\": {},", measured_input_size);
    println!("  \"benchmark_scope\": \"estimate_only\",");
    println!("  \"validation_status\": \"not_validated\",");
    println!(
        "  \"partial_reasons\": \"{}\",",
        json_escape(&partial_reasons.join("; "))
    );
    println!("  \"input_sampled\": {},", input_sampled);
    println!("  \"zstd_baseline_performed\": false,");
    println!("  \"roundtrip_performed\": false,");
    println!("  \"hash_performed\": false,");
    println!("  \"selected_mode\": null,");
    println!(
        "  \"estimated_mode\": \"{}\",",
        analysis.plan.archive_mode.as_str()
    );
    println!("  \"plan_was_correct\": null,");
    println!(
        "  \"planning_sample_bytes\": {},",
        analysis.facts.coverage.bytes_read
    );
    println!(
        "  \"planning_time_ms\": {},",
        analysis.plan.planning_time_ms
    );
    println!("  \"datapack_size_bytes\": null,");
    println!("  \"zstd_only_size_bytes\": null,");
    println!("  \"chunked_raw_zstd_size_bytes\": null,");
    println!("  \"chunked_backend\": null,");
    println!("  \"chunked_chunk_size_mb\": null,");
    println!("  \"chunked_threads\": null,");
    println!("  \"chunked_max_in_flight_chunks\": null,");
    println!("  \"compression_ratio\": null,");
    println!("  \"zstd_only_ratio\": null,");
    println!("  \"chunked_raw_zstd_ratio\": null,");
    println!("  \"compression_time_ms\": null,");
    println!("  \"decompression_time_ms\": null,");
    println!("  \"compression_mb_per_sec\": null,");
    println!("  \"decompression_mb_per_sec\": null,");
    println!("  \"zstd_only_compression_time_ms\": null,");
    println!("  \"zstd_only_compression_mb_per_sec\": null,");
    println!("  \"chunked_compression_time_ms\": null,");
    println!("  \"chunked_decompression_time_ms\": null,");
    println!("  \"chunked_compression_mb_per_sec\": null,");
    println!("  \"chunked_decompression_mb_per_sec\": null,");
    println!("  \"roundtrip_sha256_match\": null,");
    println!("  \"total_elapsed_time_ms\": {}", total_elapsed_ms);
    println!("}}");
}

pub(super) fn print_benchmark_table(metrics: &BenchmarkMetrics) {
    println!("{:<28} {:>18}  Description", "Metric", "Value");
    println!("{:-<28} {:-<18}  {:-<40}", "", "", "");
    println!(
        "{:<28} {:>18}  Full source file size before optional prefix limiting",
        "source_size_bytes", metrics.source_size_bytes
    );
    println!(
        "{:<28} {:>18}  Bytes actually used for compression measurements",
        "measured_input_size_bytes", metrics.measured_input_size_bytes
    );
    println!(
        "{:<28} {:>18}  Backward-compatible alias for measured_input_size_bytes",
        "original_size_bytes", metrics.original_size_bytes
    );
    println!(
        "{:<28} {:>18}  full or partial benchmark execution",
        "benchmark_scope", metrics.benchmark_scope
    );
    println!(
        "{:<28} {:>18}  SHA256 validation coverage",
        "validation_status", metrics.validation_status
    );
    println!(
        "{:<28} {:>18}  Why this benchmark is partial/non-validating",
        "partial_reasons",
        if metrics.partial_reasons.is_empty() {
            "none"
        } else {
            &metrics.partial_reasons
        }
    );
    println!(
        "{:<28} {:>18}  Whether --max-input-mb truncated the source",
        "input_sampled", metrics.input_sampled
    );
    println!(
        "{:<28} {:>18}  Whether the standalone zstd comparison ran",
        "zstd_baseline_performed", metrics.zstd_baseline_performed
    );
    println!(
        "{:<28} {:>18}  Whether DataPack decompression ran",
        "roundtrip_performed", metrics.roundtrip_performed
    );
    println!(
        "{:<28} {:>18}  Whether SHA256 identity validation ran",
        "hash_performed", metrics.hash_performed
    );
    println!(
        "{:<28} {:>18}  Size of the .dpack output file produced by DataPack",
        "datapack_size_bytes", metrics.datapack_size_bytes
    );
    println!(
        "{:<28} {:>18}  Size when compressed with zstd at default level",
        "zstd_only_size_bytes",
        if metrics.zstd_baseline_performed {
            metrics.zstd_only_size_bytes.to_string()
        } else {
            "unavailable".to_string()
        }
    );
    println!(
        "{:<28} {:>18}  V2 backend selected for this benchmark",
        "chunked_backend",
        metrics
            .chunked_raw_zstd
            .as_ref()
            .map(|value| value.backend.as_str())
            .unwrap_or("unavailable")
    );
    println!(
        "{:<28} {:>18}  V2 chunk target used for this benchmark",
        "chunked_chunk_size_mb",
        metrics
            .chunked_raw_zstd
            .as_ref()
            .map(|value| format!("{:.3}", value.chunk_size_mb))
            .unwrap_or_else(|| "unavailable".to_string())
    );
    println!(
        "{:<28} {:>18}  Outer workers or native zstd workers, by backend",
        "chunked_threads",
        metrics
            .chunked_raw_zstd
            .as_ref()
            .map(|value| value.threads.to_string())
            .unwrap_or_else(|| "unavailable".to_string())
    );
    println!(
        "{:<28} {:>18}  Bounded pipeline admission limit",
        "chunked_max_in_flight_chunks",
        metrics
            .chunked_raw_zstd
            .as_ref()
            .map(|value| value.max_in_flight_chunks.to_string())
            .unwrap_or_else(|| "unavailable".to_string())
    );
    println!(
        "{:<28} {:>18}  Size of v2 chunked RawZstd, when --chunked is used",
        "chunked_raw_zstd_size_bytes",
        metrics
            .chunked_raw_zstd
            .as_ref()
            .map(|value| value.size_bytes.to_string())
            .unwrap_or_else(|| "unavailable".to_string())
    );
    println!(
        "{:<28} {:>18.4}  original_size / datapack_size, higher is better",
        "compression_ratio", metrics.compression_ratio
    );
    println!(
        "{:<28} {:>18}  benchmark_input_size / zstd_only_size",
        "zstd_only_ratio",
        if metrics.zstd_baseline_performed {
            format!("{:.4}", metrics.zstd_only_ratio)
        } else {
            "unavailable".to_string()
        }
    );
    println!(
        "{:<28} {:>18}  original_size / chunked_raw_zstd_size_bytes",
        "chunked_raw_zstd_ratio",
        metrics
            .chunked_raw_zstd
            .as_ref()
            .map(|value| format!("{:.4}", value.compression_ratio))
            .unwrap_or_else(|| "unavailable".to_string())
    );
    println!(
        "{:<28} {:>18}  Selected archive mode after compression",
        "selected_mode",
        metrics.selected_mode.as_str()
    );
    println!(
        "{:<28} {:>18}  Mode predicted before compression",
        "estimated_mode",
        metrics.estimated_mode.as_str()
    );
    println!(
        "{:<28} {:>18}  selected_mode == estimated_mode",
        "plan_was_correct", metrics.plan_was_correct
    );
    println!(
        "{:<28} {:>18.3}  From CompressionPlan.estimated_memory_mb",
        "peak_memory_estimate_mb",
        metrics.peak_memory_estimate_mb.unwrap_or(0.0)
    );
    println!(
        "{:<28} {:>18}  Time spent in SampleAnalyzer + plan building",
        "planning_time_ms", metrics.planning_time_ms
    );
    println!(
        "{:<28} {:>18}  Benchmark timing runs used",
        "runs_used", metrics.runs_used
    );
    println!(
        "{:<28} {:>18}  Total benchmark elapsed time",
        "total_elapsed_time_ms", metrics.total_elapsed_time_ms
    );
    println!(
        "{:<28} {:>18}  Columnar candidate error, if fallback was needed",
        "columnar_candidate_error",
        metrics.columnar_candidate_error.as_deref().unwrap_or("")
    );
    println!(
        "{:<28} {:>18.3}  Wall-clock compression time, median of runs_used",
        "compression_time_ms", metrics.compression_time_ms
    );
    println!(
        "{:<28} {:>18}  v2 RawZstd compression time, median of runs_used",
        "chunked_compression_time_ms",
        metrics
            .chunked_raw_zstd
            .as_ref()
            .map(|value| format!("{:.3}", value.compression_time_ms))
            .unwrap_or_else(|| "unavailable".to_string())
    );
    println!(
        "{:<28} {:>18}  Standalone zstd compression time",
        "zstd_only_compression_time_ms",
        metrics
            .zstd_only_compression_time_ms
            .map(|value| format!("{value:.3}"))
            .unwrap_or_else(|| "unavailable".to_string())
    );
    println!(
        "{:<28} {:>18.3}  Input MB/s during compression",
        "compression_mb_per_sec", metrics.compression_input_mb_per_second
    );
    println!(
        "{:<28} {:>18}  Input MB/s during standalone zstd compression",
        "zstd_only_compression_mb_per_sec",
        metrics
            .zstd_only_compression_mb_per_second
            .map(|value| format!("{value:.3}"))
            .unwrap_or_else(|| "unavailable".to_string())
    );
    println!(
        "{:<28} {:>18}  Input MB/s during v2 RawZstd compression",
        "chunked_compression_mb_per_sec",
        metrics
            .chunked_raw_zstd
            .as_ref()
            .map(|value| format!("{:.3}", value.compression_mb_per_second))
            .unwrap_or_else(|| "unavailable".to_string())
    );
    println!(
        "{:<28} {:>18}  Wall-clock decompression time, median of runs_used",
        "decompression_time_ms",
        if metrics.roundtrip_performed {
            format!("{:.3}", metrics.decompression_time_ms)
        } else {
            "unavailable".to_string()
        }
    );
    println!(
        "{:<28} {:>18}  v2 RawZstd decompression time, median of runs_used",
        "chunked_decompression_time_ms",
        metrics
            .chunked_raw_zstd
            .as_ref()
            .and_then(|value| value.decompression_time_ms)
            .map(|value| format!("{value:.3}"))
            .unwrap_or_else(|| "unavailable".to_string())
    );
    println!(
        "{:<28} {:>18}  Input MB/s during decompression",
        "decompression_mb_per_sec",
        if metrics.roundtrip_performed {
            format!("{:.3}", metrics.decompression_input_mb_per_second)
        } else {
            "unavailable".to_string()
        }
    );
    println!(
        "{:<28} {:>18}  Input MB/s during v2 RawZstd decompression",
        "chunked_decompression_mb_per_sec",
        metrics
            .chunked_raw_zstd
            .as_ref()
            .and_then(|value| value.decompression_mb_per_second)
            .map(|value| format!("{value:.3}"))
            .unwrap_or_else(|| "unavailable".to_string())
    );
    println!(
        "{:<28} {:>18}  SHA256 of decompressed output equals SHA256 of input",
        "roundtrip_sha256_match",
        if metrics.hash_performed {
            metrics.roundtrip_sha256_match.to_string()
        } else {
            "unavailable".to_string()
        }
    );
    println!(
        "{:<28} {:>18}  Whether round-trip decompression was skipped",
        "no_roundtrip", metrics.no_roundtrip
    );
    println!(
        "{:<28} {:>18}  Backward-compatible name for no_roundtrip",
        "skip_full_roundtrip", metrics.no_roundtrip
    );
    println!(
        "{:<28} {:>18}  Whether SHA256 validation was skipped",
        "no_hash", metrics.no_hash
    );
    println!(
        "{:<28} {:>18}  v2 RawZstd SHA256 validation, when --chunked is used",
        "chunked_roundtrip_sha256_match",
        metrics
            .chunked_raw_zstd
            .as_ref()
            .and_then(|value| value.roundtrip_sha256_match)
            .map(|value| value.to_string())
            .unwrap_or_else(|| "unavailable".to_string())
    );
    println!(
        "{:<28} {:>18}  datapack_size_bytes < zstd_only_size_bytes",
        "datapack_beats_zstd",
        if metrics.zstd_baseline_performed {
            metrics.datapack_beats_zstd.to_string()
        } else {
            "unavailable".to_string()
        }
    );
}

pub(super) fn print_benchmark_json(metrics: &BenchmarkMetrics) {
    println!("{{");
    println!("  \"source_size_bytes\": {},", metrics.source_size_bytes);
    println!(
        "  \"measured_input_size_bytes\": {},",
        metrics.measured_input_size_bytes
    );
    println!(
        "  \"original_size_bytes\": {},",
        metrics.original_size_bytes
    );
    println!(
        "  \"benchmark_scope\": \"{}\",",
        json_escape(&metrics.benchmark_scope)
    );
    println!(
        "  \"validation_status\": \"{}\",",
        json_escape(&metrics.validation_status)
    );
    println!(
        "  \"partial_reasons\": \"{}\",",
        json_escape(&metrics.partial_reasons)
    );
    println!("  \"input_sampled\": {},", metrics.input_sampled);
    println!(
        "  \"zstd_baseline_performed\": {},",
        metrics.zstd_baseline_performed
    );
    println!(
        "  \"roundtrip_performed\": {},",
        metrics.roundtrip_performed
    );
    println!("  \"hash_performed\": {},", metrics.hash_performed);
    println!(
        "  \"datapack_size_bytes\": {},",
        metrics.datapack_size_bytes
    );
    if metrics.zstd_baseline_performed {
        println!(
            "  \"zstd_only_size_bytes\": {},",
            metrics.zstd_only_size_bytes
        );
    } else {
        println!("  \"zstd_only_size_bytes\": null,");
    }
    match &metrics.chunked_raw_zstd {
        Some(value) => println!(
            "  \"chunked_backend\": \"{}\",",
            json_escape(&value.backend)
        ),
        None => println!("  \"chunked_backend\": null,"),
    }
    match &metrics.chunked_raw_zstd {
        Some(value) => println!("  \"chunked_chunk_size_mb\": {:.3},", value.chunk_size_mb),
        None => println!("  \"chunked_chunk_size_mb\": null,"),
    }
    match &metrics.chunked_raw_zstd {
        Some(value) => println!("  \"chunked_threads\": {},", value.threads),
        None => println!("  \"chunked_threads\": null,"),
    }
    match &metrics.chunked_raw_zstd {
        Some(value) => println!(
            "  \"chunked_max_in_flight_chunks\": {},",
            value.max_in_flight_chunks
        ),
        None => println!("  \"chunked_max_in_flight_chunks\": null,"),
    }
    match &metrics.chunked_raw_zstd {
        Some(value) => println!("  \"chunked_raw_zstd_size_bytes\": {},", value.size_bytes),
        None => println!("  \"chunked_raw_zstd_size_bytes\": null,"),
    }
    println!("  \"compression_ratio\": {:.6},", metrics.compression_ratio);
    println!(
        "  \"selected_mode\": \"{}\",",
        metrics.selected_mode.as_str()
    );
    println!(
        "  \"estimated_mode\": \"{}\",",
        metrics.estimated_mode.as_str()
    );
    println!("  \"plan_was_correct\": {},", metrics.plan_was_correct);
    match metrics.peak_memory_estimate_mb {
        Some(value) => println!("  \"peak_memory_estimate_mb\": {:.3},", value),
        None => println!("  \"peak_memory_estimate_mb\": null,"),
    }
    println!("  \"planning_time_ms\": {},", metrics.planning_time_ms);
    println!("  \"runs_used\": {},", metrics.runs_used);
    println!(
        "  \"total_elapsed_time_ms\": {},",
        metrics.total_elapsed_time_ms
    );
    match &metrics.columnar_candidate_error {
        Some(value) => println!(
            "  \"columnar_candidate_error\": \"{}\",",
            json_escape(value)
        ),
        None => println!("  \"columnar_candidate_error\": null,"),
    }
    if metrics.zstd_baseline_performed {
        println!("  \"zstd_only_ratio\": {:.6},", metrics.zstd_only_ratio);
    } else {
        println!("  \"zstd_only_ratio\": null,");
    }
    match &metrics.chunked_raw_zstd {
        Some(value) => println!(
            "  \"chunked_raw_zstd_ratio\": {:.6},",
            value.compression_ratio
        ),
        None => println!("  \"chunked_raw_zstd_ratio\": null,"),
    }
    println!(
        "  \"compression_time_ms\": {:.3},",
        metrics.compression_time_ms
    );
    match &metrics.chunked_raw_zstd {
        Some(value) => println!(
            "  \"chunked_compression_time_ms\": {:.3},",
            value.compression_time_ms
        ),
        None => println!("  \"chunked_compression_time_ms\": null,"),
    }
    match metrics.zstd_only_compression_time_ms {
        Some(value) => println!("  \"zstd_only_compression_time_ms\": {:.3},", value),
        None => println!("  \"zstd_only_compression_time_ms\": null,"),
    }
    println!(
        "  \"compression_mb_per_sec\": {:.3},",
        metrics.compression_input_mb_per_second
    );
    match metrics.zstd_only_compression_mb_per_second {
        Some(value) => println!("  \"zstd_only_compression_mb_per_sec\": {:.3},", value),
        None => println!("  \"zstd_only_compression_mb_per_sec\": null,"),
    }
    match &metrics.chunked_raw_zstd {
        Some(value) => println!(
            "  \"chunked_compression_mb_per_sec\": {:.3},",
            value.compression_mb_per_second
        ),
        None => println!("  \"chunked_compression_mb_per_sec\": null,"),
    }
    if metrics.roundtrip_performed {
        println!(
            "  \"decompression_time_ms\": {:.3},",
            metrics.decompression_time_ms
        );
    } else {
        println!("  \"decompression_time_ms\": null,");
    }
    match metrics
        .chunked_raw_zstd
        .as_ref()
        .and_then(|value| value.decompression_time_ms)
    {
        Some(value) => println!("  \"chunked_decompression_time_ms\": {:.3},", value),
        None => println!("  \"chunked_decompression_time_ms\": null,"),
    }
    if metrics.roundtrip_performed {
        println!(
            "  \"decompression_mb_per_sec\": {:.3},",
            metrics.decompression_input_mb_per_second
        );
    } else {
        println!("  \"decompression_mb_per_sec\": null,");
    }
    match metrics
        .chunked_raw_zstd
        .as_ref()
        .and_then(|value| value.decompression_mb_per_second)
    {
        Some(value) => println!("  \"chunked_decompression_mb_per_sec\": {:.3},", value),
        None => println!("  \"chunked_decompression_mb_per_sec\": null,"),
    }
    if metrics.hash_performed {
        println!(
            "  \"roundtrip_sha256_match\": {},",
            metrics.roundtrip_sha256_match
        );
    } else {
        println!("  \"roundtrip_sha256_match\": null,");
    }
    println!("  \"no_roundtrip\": {},", metrics.no_roundtrip);
    println!("  \"skip_full_roundtrip\": {},", metrics.no_roundtrip);
    println!("  \"no_hash\": {},", metrics.no_hash);
    match metrics
        .chunked_raw_zstd
        .as_ref()
        .and_then(|value| value.roundtrip_sha256_match)
    {
        Some(value) => println!("  \"chunked_roundtrip_sha256_match\": {},", value),
        None => println!("  \"chunked_roundtrip_sha256_match\": null,"),
    }
    if metrics.zstd_baseline_performed {
        println!("  \"datapack_beats_zstd\": {}", metrics.datapack_beats_zstd);
    } else {
        println!("  \"datapack_beats_zstd\": null");
    }
    println!("}}");
}

pub(super) fn print_profile_timings(timings: &ProfileTimings) {
    eprintln!("profile diagnostics:");
    eprintln!(
        "  planning_ms={}",
        display_optional_u64(timings.planning_ms)
    );
    eprintln!(
        "  read_input_ms={}",
        display_optional_u64(timings.read_input_ms)
    );
    eprintln!(
        "  zstd_only_ms={}",
        display_optional_u64(timings.zstd_only_ms)
    );
    eprintln!(
        "  datapack_compress_ms={}",
        display_optional_u64(timings.datapack_compress_ms)
    );
    eprintln!(
        "  datapack_decompress_ms={}",
        display_optional_u64(timings.datapack_decompress_ms)
    );
    eprintln!("  hash_ms={}", display_optional_u64(timings.hash_ms));
    eprintln!(
        "  archive_write_ms={}",
        display_optional_u64(timings.archive_write_ms)
    );
    eprintln!(
        "  archive_read_ms={}",
        display_optional_u64(timings.archive_read_ms)
    );
    eprintln!(
        "  total_elapsed_ms={}",
        display_optional_u64(timings.total_elapsed_ms)
    );
    eprintln!(
        "  compression_mb_per_sec={}",
        display_optional_f64(timings.compression_mb_per_sec)
    );
    eprintln!(
        "  decompression_mb_per_sec={}",
        display_optional_f64(timings.decompression_mb_per_sec)
    );
}

pub(super) fn json_escape(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\u{08}' => escaped.push_str("\\b"),
            '\u{0c}' => escaped.push_str("\\f"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            character if character <= '\u{1f}' => {
                escaped.push_str(&format!("\\u{:04x}", character as u32));
            }
            character => escaped.push(character),
        }
    }
    escaped
}
