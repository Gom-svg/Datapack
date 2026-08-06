use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::analysis;
use crate::compression::zstd_backend;
use crate::error::{DatapackError, Result};
use crate::planning::ArchiveMode;
use crate::storage;

use super::archive::encode_for_plan_detailed;
use super::chunked_options::build_chunked_compress_options;
use super::profile::{duration_ms, elapsed_ms, mb_per_second};
use super::progress::{progress_phase, read_prefix_buffered_progress, IO_BUFFER_BYTES};
use super::{BenchmarkOptions, DEFAULT_SAMPLE_MB};

mod model;
mod report;
mod streaming;
mod temp;

#[cfg(test)]
mod tests;

use model::{
    benchmark_partial_reasons, benchmark_scope, compression_ratio, compression_ratio_u64,
    median_duration, validation_status, BenchmarkMetrics, ChunkedBenchmarkMetrics, ProfileTimings,
};
use report::{
    print_benchmark_json, print_benchmark_table, print_estimate_only_json,
    print_estimate_only_table, print_profile_timings,
};
use streaming::sha256_hex;
use temp::{
    benchmark_chunked_restore_path, benchmark_chunked_sample_path, benchmark_chunked_temp_path,
    benchmark_temp_path, BenchmarkTempFiles,
};

const LARGE_BENCHMARK_WARNING_BYTES: u64 = 1024 * 1024 * 1024;

fn warn_for_large_full_benchmark(source_size: u64, options: &BenchmarkOptions) {
    let full_flow = !options.estimate_only
        && options.max_input_mb.is_none()
        && !options.no_zstd_baseline
        && !options.no_roundtrip
        && !options.no_hash;
    if source_size > LARGE_BENCHMARK_WARNING_BYTES && full_flow {
        eprintln!(
            "warning: Large input detected ({:.2} GiB). Full benchmark may run zstd baseline, DataPack compression, decompression, and SHA256 validation. Use --quick, --max-input-mb, --no-roundtrip, or --no-zstd-baseline for faster partial tests.",
            source_size as f64 / 1_073_741_824.0
        );
    }
}

fn benchmark_max_input_bytes(max_input_mb: Option<u64>) -> Result<Option<u64>> {
    max_input_mb
        .map(|value| {
            if value == 0 {
                return Err(DatapackError::InvalidFormat(
                    "--max-input-mb must be greater than zero".to_string(),
                ));
            }
            value.checked_mul(1024 * 1024).ok_or_else(|| {
                DatapackError::InvalidFormat("--max-input-mb is too large".to_string())
            })
        })
        .transpose()
}

pub(super) fn run(input: PathBuf, options: BenchmarkOptions) -> Result<()> {
    let total_started = Instant::now();
    let mut profile_timings = ProfileTimings::default();
    let source_size = std::fs::metadata(&input)?.len();
    warn_for_large_full_benchmark(source_size, &options);
    let runs_used = if options.quick {
        1
    } else {
        options.runs.max(1)
    };
    let max_input_bytes = benchmark_max_input_bytes(options.max_input_mb)?;
    let benchmark_input_size = max_input_bytes
        .map(|limit| source_size.min(limit))
        .unwrap_or(source_size);
    let input_sampled = benchmark_input_size < source_size;
    let planning_sample_mb = options
        .max_input_mb
        .map(|limit_mb| limit_mb.min(DEFAULT_SAMPLE_MB))
        .unwrap_or(DEFAULT_SAMPLE_MB);

    let phase_started = Instant::now();
    let analysis =
        analysis::analyze_path_with_scope(&input, planning_sample_mb, benchmark_input_size)?;
    profile_timings.planning_ms = Some(elapsed_ms(phase_started));
    progress_phase("planning", 0, None, phase_started);
    if analysis.requires_raw_fallback() {
        let limitations = analysis
            .facts
            .limitations
            .iter()
            .map(|limitation| limitation.code())
            .collect::<Vec<_>>()
            .join(", ");
        eprintln!("benchmark analysis limited ({limitations}); using RawZstd execution.");
    }
    let estimated_mode = analysis.plan.archive_mode;

    if options.estimate_only {
        let partial_reasons =
            vec!["estimate-only: compression, decompression, and hashing skipped"];
        if options.json {
            print_estimate_only_json(&analysis, &partial_reasons, elapsed_ms(total_started));
        } else {
            print_estimate_only_table(&analysis, &partial_reasons, elapsed_ms(total_started));
        }
        profile_timings.total_elapsed_ms = Some(elapsed_ms(total_started));
        if options.profile {
            print_profile_timings(&profile_timings);
        }
        return Ok(());
    }

    if estimated_mode == ArchiveMode::RawZstd {
        return streaming::run(
            input,
            options,
            analysis,
            source_size,
            benchmark_input_size,
            input_sampled,
            runs_used,
            total_started,
            profile_timings,
        );
    }

    let phase_started = Instant::now();
    let bytes =
        read_prefix_buffered_progress(&input, "benchmark read input", Some(benchmark_input_size))?;
    profile_timings.read_input_ms = Some(elapsed_ms(phase_started));

    let hash_enabled = !options.no_hash && !options.no_roundtrip;
    let original_hash = if hash_enabled {
        let phase_started = Instant::now();
        let hash = sha256_hex(&bytes);
        profile_timings.hash_ms = Some(elapsed_ms(phase_started));
        Some(hash)
    } else {
        None
    };

    let (zstd_only, zstd_only_duration) = if options.no_zstd_baseline {
        (None, None)
    } else {
        let phase_started = Instant::now();
        let compressed = zstd_backend::compress(&bytes)?;
        let duration = phase_started.elapsed();
        profile_timings.zstd_only_ms = Some(duration_ms(duration));
        progress_phase(
            "zstd-only",
            bytes.len() as u64,
            Some(bytes.len() as u64),
            phase_started,
        );
        (Some(compressed), Some(duration))
    };

    let mut compression_times = Vec::with_capacity(runs_used);
    let mut archive_write_times = Vec::with_capacity(runs_used);
    let mut datapack = Vec::new();
    let mut selected_mode = ArchiveMode::RawZstd;
    let mut columnar_candidate_error = None;
    let temp_path = benchmark_temp_path(&input);
    let chunked_temp_path = benchmark_chunked_temp_path(&input);
    let chunked_restore_path = benchmark_chunked_restore_path(&input);
    let chunked_sample_path = benchmark_chunked_sample_path(&input);
    let _temp_files = BenchmarkTempFiles::new(
        options.keep_temp,
        vec![
            temp_path.clone(),
            chunked_temp_path.clone(),
            chunked_restore_path.clone(),
            chunked_sample_path.clone(),
        ],
    );
    for run_index in 0..runs_used {
        eprintln!("benchmark compress run {}/{}", run_index + 1, runs_used);
        let start = Instant::now();
        let (archive, mode, error) = encode_for_plan_detailed(&input, &bytes, estimated_mode)?;
        compression_times.push(start.elapsed());
        progress_phase(
            "benchmark encode+compress",
            bytes.len() as u64,
            Some(bytes.len() as u64),
            start,
        );

        let write_started = Instant::now();
        std::fs::write(&temp_path, &archive)?;
        archive_write_times.push(write_started.elapsed());
        progress_phase(
            "benchmark write archive",
            archive.len() as u64,
            Some(archive.len() as u64),
            write_started,
        );

        datapack = archive;
        selected_mode = mode;
        if columnar_candidate_error.is_none() {
            columnar_candidate_error = error;
        }
    }

    let archive = storage::decode_archive(&datapack)?;
    let mut decompression_times = Vec::with_capacity(runs_used);
    let mut restored = Vec::new();
    if !options.no_roundtrip {
        for run_index in 0..runs_used {
            eprintln!("benchmark decompress run {}/{}", run_index + 1, runs_used);
            let start = Instant::now();
            let decoded_archive = storage::decode_archive(&datapack)?;
            restored = storage::restore_archive(&decoded_archive)?;
            decompression_times.push(start.elapsed());
            progress_phase(
                "benchmark decompress+decode",
                decoded_archive.payload.len() as u64,
                Some(decoded_archive.payload.len() as u64),
                start,
            );
        }
    } else {
        decompression_times.push(Duration::ZERO);
    }

    let phase_started = Instant::now();
    let restored_hash = if !hash_enabled {
        None
    } else {
        Some(sha256_hex(&restored))
    };
    if hash_enabled {
        profile_timings.hash_ms = Some(
            profile_timings
                .hash_ms
                .unwrap_or(0)
                .saturating_add(elapsed_ms(phase_started)),
        );
    }
    let median_compression = median_duration(&compression_times);
    let median_decompression = median_duration(&decompression_times);
    let compression_time_ms = median_compression.as_secs_f64() * 1000.0;
    let decompression_time_ms = median_decompression.as_secs_f64() * 1000.0;
    let compression_mb_per_sec = mb_per_second(bytes.len(), median_compression);
    let decompression_mb_per_sec = mb_per_second(bytes.len(), median_decompression);
    profile_timings.datapack_compress_ms = Some(duration_ms(median_compression));
    if !options.no_roundtrip {
        profile_timings.datapack_decompress_ms = Some(duration_ms(median_decompression));
    }
    profile_timings.archive_write_ms = Some(duration_ms(median_duration(&archive_write_times)));
    profile_timings.compression_mb_per_sec = Some(compression_mb_per_sec);
    if !options.no_roundtrip {
        profile_timings.decompression_mb_per_sec = Some(decompression_mb_per_sec);
    }

    let mut chunked_metrics = None;
    if options.uses_chunked() {
        let chunked_input = if input_sampled {
            let mut sample_writer =
                BufWriter::with_capacity(IO_BUFFER_BYTES, File::create(&chunked_sample_path)?);
            sample_writer.write_all(&bytes)?;
            sample_writer.flush()?;
            chunked_sample_path.as_path()
        } else {
            input.as_path()
        };
        let mut chunk_options = build_chunked_compress_options(
            options.chunk_size_mb,
            options.threads,
            options.max_in_flight_chunks,
            options.backend,
            options.adaptive_level,
            options.profile,
        )?;
        chunk_options.force = true;
        chunk_options.keep_temp = options.keep_temp;
        let mut chunked_compression_times = Vec::with_capacity(runs_used);
        let mut chunked_decompression_times = Vec::with_capacity(runs_used);
        let mut chunked_archive_size = 0u64;
        let mut chunked_roundtrip_sha256_match = None;

        for run_index in 0..runs_used {
            eprintln!(
                "benchmark chunked RawZstd compress run {}/{}",
                run_index + 1,
                runs_used
            );
            let started = Instant::now();
            let stats = storage::chunked::encode_raw_zstd_chunked_file(
                chunked_input,
                &chunked_temp_path,
                chunk_options,
            )?;
            chunked_compression_times.push(started.elapsed());
            chunked_archive_size = stats.archive_size_bytes;
        }

        if !options.no_roundtrip {
            for run_index in 0..runs_used {
                eprintln!(
                    "benchmark chunked RawZstd decompress run {}/{}",
                    run_index + 1,
                    runs_used
                );
                let started = Instant::now();
                storage::chunked::decode_raw_zstd_chunked_file(
                    &chunked_temp_path,
                    &chunked_restore_path,
                    storage::chunked::ChunkedDecompressOptions {
                        verify: hash_enabled,
                        force: true,
                        keep_temp: options.keep_temp,
                        ..storage::chunked::ChunkedDecompressOptions::default()
                    },
                )?;
                chunked_decompression_times.push(started.elapsed());
            }
            if hash_enabled {
                // Successful verified v2 restore covers per-chunk and global SHA256.
                chunked_roundtrip_sha256_match = Some(true);
            }
        } else {
            chunked_decompression_times.push(Duration::ZERO);
        }

        let chunked_compression_duration = median_duration(&chunked_compression_times);
        let chunked_decompression_duration = if options.no_roundtrip {
            None
        } else {
            Some(median_duration(&chunked_decompression_times))
        };
        chunked_metrics = Some(ChunkedBenchmarkMetrics {
            backend: chunk_options.backend.as_str().to_string(),
            chunk_size_mb: chunk_options.chunk_size_bytes as f64 / 1_048_576.0,
            threads: chunk_options.threads,
            max_in_flight_chunks: chunk_options.max_in_flight_chunks,
            size_bytes: chunked_archive_size,
            compression_ratio: compression_ratio_u64(bytes.len() as u64, chunked_archive_size),
            compression_time_ms: chunked_compression_duration.as_secs_f64() * 1000.0,
            compression_mb_per_second: mb_per_second(bytes.len(), chunked_compression_duration),
            decompression_time_ms: chunked_decompression_duration
                .map(|duration| duration.as_secs_f64() * 1000.0),
            decompression_mb_per_second: chunked_decompression_duration
                .map(|duration| mb_per_second(bytes.len(), duration)),
            roundtrip_sha256_match: chunked_roundtrip_sha256_match,
        });
    }

    let metrics = BenchmarkMetrics {
        measured_input_size_bytes: bytes.len() as u64,
        original_size_bytes: bytes.len() as u64,
        datapack_size_bytes: datapack.len() as u64,
        zstd_only_size_bytes: zstd_only.as_ref().map_or(0, |bytes| bytes.len() as u64),
        zstd_only_compression_time_ms: zstd_only_duration
            .map(|duration| duration.as_secs_f64() * 1000.0),
        zstd_only_compression_mb_per_second: zstd_only_duration
            .map(|duration| mb_per_second(bytes.len(), duration)),
        compression_ratio: compression_ratio(bytes.len(), datapack.len()),
        zstd_only_ratio: zstd_only.as_ref().map_or(0.0, |compressed| {
            compression_ratio(bytes.len(), compressed.len())
        }),
        selected_mode,
        estimated_mode,
        plan_was_correct: selected_mode == estimated_mode,
        peak_memory_estimate_mb: Some(analysis.plan.estimated_memory_mb),
        planning_time_ms: analysis.plan.planning_time_ms,
        columnar_candidate_error,
        runs_used,
        total_elapsed_time_ms: total_started.elapsed().as_millis() as u64,
        compression_time_ms,
        decompression_time_ms,
        compression_input_mb_per_second: compression_mb_per_sec,
        decompression_input_mb_per_second: decompression_mb_per_sec,
        roundtrip_sha256_match: restored_hash.as_deref().is_some_and(|restored_hash| {
            original_hash
                .as_deref()
                .is_some_and(|original_hash| original_hash == restored_hash)
        }),
        datapack_beats_zstd: zstd_only
            .as_ref()
            .is_some_and(|compressed| datapack.len() < compressed.len()),
        chunked_raw_zstd: chunked_metrics,
        source_size_bytes: source_size,
        input_sampled,
        zstd_baseline_performed: zstd_only.is_some(),
        roundtrip_performed: !options.no_roundtrip,
        hash_performed: hash_enabled,
        benchmark_scope: benchmark_scope(&options, input_sampled).to_string(),
        validation_status: validation_status(
            hash_enabled,
            input_sampled,
            restored_hash.as_deref(),
            original_hash.as_deref(),
        )?
        .to_string(),
        partial_reasons: benchmark_partial_reasons(&options, input_sampled).join("; "),
        no_roundtrip: options.no_roundtrip,
        no_hash: options.no_hash,
    };
    profile_timings.total_elapsed_ms = Some(elapsed_ms(total_started));

    if options.json {
        print_benchmark_json(&metrics);
    } else {
        print_benchmark_table(&metrics);
        println!(
            "mode                     {:?}",
            archive.metadata.payload_kind
        );
        if let Some(error) = &metrics.columnar_candidate_error {
            eprintln!("columnar_candidate_error = {error:?}");
        }
        if options.keep_temp {
            println!("temp_artifact             {}", temp_path.display());
            if options.uses_chunked() {
                println!("chunked_temp_artifact     {}", chunked_temp_path.display());
            }
        }
    }
    if options.profile {
        print_profile_timings(&profile_timings);
    }

    Ok(())
}
