use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};

use crate::analysis::DatasetAnalysis;
use crate::compression::zstd_backend;
use crate::error::{DatapackError, Result};
use crate::planning::ArchiveMode;
use crate::storage;

use super::super::chunked_options::build_chunked_compress_options;
use super::super::profile::{duration_ms, elapsed_ms, mb_per_second_u64};
use super::super::progress::{ProgressReader, ProgressWriter, IO_BUFFER_BYTES};
use super::super::BenchmarkOptions;
use super::model::{
    benchmark_partial_reasons, benchmark_scope, compression_ratio_u64, median_duration,
    validation_status, BenchmarkMetrics, ChunkedBenchmarkMetrics, ProfileTimings,
};
use super::report::{print_benchmark_json, print_benchmark_table, print_profile_timings};
use super::temp::{
    benchmark_chunked_restore_path, benchmark_chunked_sample_path, benchmark_chunked_temp_path,
    benchmark_restore_path, benchmark_temp_path, benchmark_zstd_temp_path, BenchmarkTempFiles,
};

#[allow(clippy::too_many_arguments)]
pub(super) fn run(
    input: PathBuf,
    options: BenchmarkOptions,
    analysis: DatasetAnalysis,
    source_size: u64,
    benchmark_input_size: u64,
    input_sampled: bool,
    runs_used: usize,
    total_started: Instant,
    mut profile_timings: ProfileTimings,
) -> Result<()> {
    eprintln!("benchmark execution path: streaming RawZstd");
    let hash_enabled = !options.no_hash && !options.no_roundtrip;
    let temp_path = benchmark_temp_path(&input);
    let restore_path = benchmark_restore_path(&input);
    let zstd_temp_path = benchmark_zstd_temp_path(&input);
    let chunked_temp_path = benchmark_chunked_temp_path(&input);
    let chunked_restore_path = benchmark_chunked_restore_path(&input);
    let chunked_sample_path = benchmark_chunked_sample_path(&input);
    let _temp_files = BenchmarkTempFiles::new(
        options.keep_temp,
        vec![
            temp_path.clone(),
            restore_path.clone(),
            zstd_temp_path.clone(),
            chunked_temp_path.clone(),
            chunked_restore_path.clone(),
            chunked_sample_path.clone(),
        ],
    );

    let original_hash = if hash_enabled {
        let started = Instant::now();
        let hash = sha256_path_prefix(&input, benchmark_input_size, "benchmark hash input")?;
        profile_timings.hash_ms = Some(elapsed_ms(started));
        Some(hash)
    } else {
        None
    };

    let mut zstd_only_size = 0u64;
    let mut zstd_only_duration = None;
    if !options.no_zstd_baseline {
        let started = Instant::now();
        zstd_only_size = write_zstd_prefix(
            &input,
            &zstd_temp_path,
            benchmark_input_size,
            "benchmark zstd-only",
        )?;
        let duration = started.elapsed();
        profile_timings.zstd_only_ms = Some(duration_ms(duration));
        zstd_only_duration = Some(duration);
    }

    let mut compression_times = Vec::with_capacity(runs_used);
    let mut datapack_size = 0u64;
    for run_index in 0..runs_used {
        eprintln!("benchmark compress run {}/{}", run_index + 1, runs_used);
        let started = Instant::now();
        datapack_size = write_raw_zstd_prefix(
            &input,
            &temp_path,
            benchmark_input_size,
            "benchmark raw-zstd compress",
        )?;
        compression_times.push(started.elapsed());
    }

    let mut decompression_times = Vec::with_capacity(runs_used);
    if !options.no_roundtrip {
        for run_index in 0..runs_used {
            eprintln!("benchmark decompress run {}/{}", run_index + 1, runs_used);
            let started = Instant::now();
            restore_v1_raw_path(&temp_path, &restore_path, "benchmark raw-zstd decompress")?;
            decompression_times.push(started.elapsed());
        }
    } else {
        decompression_times.push(Duration::ZERO);
    }

    let restored_hash = if hash_enabled {
        let started = Instant::now();
        let hash = sha256_path_prefix(
            &restore_path,
            benchmark_input_size,
            "benchmark hash restored",
        )?;
        profile_timings.hash_ms = Some(
            profile_timings
                .hash_ms
                .unwrap_or(0)
                .saturating_add(elapsed_ms(started)),
        );
        Some(hash)
    } else {
        None
    };

    let median_compression = median_duration(&compression_times);
    let median_decompression = median_duration(&decompression_times);
    let compression_time_ms = median_compression.as_secs_f64() * 1000.0;
    let decompression_time_ms = median_decompression.as_secs_f64() * 1000.0;
    let compression_mb_per_sec = mb_per_second_u64(benchmark_input_size, median_compression);
    let decompression_mb_per_sec = mb_per_second_u64(benchmark_input_size, median_decompression);
    profile_timings.datapack_compress_ms = Some(duration_ms(median_compression));
    profile_timings.compression_mb_per_sec = Some(compression_mb_per_sec);
    if !options.no_roundtrip {
        profile_timings.datapack_decompress_ms = Some(duration_ms(median_decompression));
        profile_timings.decompression_mb_per_sec = Some(decompression_mb_per_sec);
    }

    let mut chunked_metrics = None;
    if options.uses_chunked() {
        let chunked_input = if input_sampled {
            copy_path_prefix(
                &input,
                &chunked_sample_path,
                benchmark_input_size,
                "benchmark write input prefix",
            )?;
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
                // v2 decompression has already verified every chunk hash and the
                // global input hash before returning successfully.
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
            compression_ratio: compression_ratio_u64(benchmark_input_size, chunked_archive_size),
            compression_time_ms: chunked_compression_duration.as_secs_f64() * 1000.0,
            compression_mb_per_second: mb_per_second_u64(
                benchmark_input_size,
                chunked_compression_duration,
            ),
            decompression_time_ms: chunked_decompression_duration
                .map(|duration| duration.as_secs_f64() * 1000.0),
            decompression_mb_per_second: chunked_decompression_duration
                .map(|duration| mb_per_second_u64(benchmark_input_size, duration)),
            roundtrip_sha256_match: chunked_roundtrip_sha256_match,
        });
    }

    let roundtrip_sha256_match = restored_hash.as_deref().is_some_and(|restored| {
        original_hash
            .as_deref()
            .is_some_and(|original| original == restored)
    });
    let metrics = BenchmarkMetrics {
        source_size_bytes: source_size,
        measured_input_size_bytes: benchmark_input_size,
        original_size_bytes: benchmark_input_size,
        datapack_size_bytes: datapack_size,
        zstd_only_size_bytes: zstd_only_size,
        zstd_only_compression_time_ms: zstd_only_duration
            .map(|duration| duration.as_secs_f64() * 1000.0),
        zstd_only_compression_mb_per_second: zstd_only_duration
            .map(|duration| mb_per_second_u64(benchmark_input_size, duration)),
        compression_ratio: compression_ratio_u64(benchmark_input_size, datapack_size),
        zstd_only_ratio: if options.no_zstd_baseline {
            0.0
        } else {
            compression_ratio_u64(benchmark_input_size, zstd_only_size)
        },
        selected_mode: ArchiveMode::RawZstd,
        estimated_mode: ArchiveMode::RawZstd,
        plan_was_correct: true,
        peak_memory_estimate_mb: Some(analysis.plan.estimated_memory_mb),
        planning_time_ms: analysis.plan.planning_time_ms,
        columnar_candidate_error: None,
        runs_used,
        total_elapsed_time_ms: elapsed_ms(total_started),
        compression_time_ms,
        decompression_time_ms,
        compression_input_mb_per_second: compression_mb_per_sec,
        decompression_input_mb_per_second: decompression_mb_per_sec,
        roundtrip_sha256_match,
        datapack_beats_zstd: !options.no_zstd_baseline && datapack_size < zstd_only_size,
        chunked_raw_zstd: chunked_metrics,
        input_sampled,
        zstd_baseline_performed: !options.no_zstd_baseline,
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
        println!("mode                     RawZstd");
        if options.keep_temp {
            println!("temp_artifact             {}", temp_path.display());
            if !options.no_zstd_baseline {
                println!("zstd_temp_artifact        {}", zstd_temp_path.display());
            }
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

fn write_raw_zstd_prefix(
    input_path: &Path,
    output_path: &Path,
    input_size: u64,
    phase: &str,
) -> Result<u64> {
    let input = BufReader::with_capacity(IO_BUFFER_BYTES, File::open(input_path)?);
    let limited = input.take(input_size);
    let mut reader = ProgressReader::new(limited, phase, Some(input_size));
    let mut writer = BufWriter::with_capacity(IO_BUFFER_BYTES, File::create(output_path)?);
    storage::write_raw_zstd_archive_stream(input_path, input_size, &mut reader, &mut writer)?;
    writer.flush()?;
    reader.finish();
    Ok(std::fs::metadata(output_path)?.len())
}

fn write_zstd_prefix(
    input_path: &Path,
    output_path: &Path,
    input_size: u64,
    phase: &str,
) -> Result<u64> {
    let input = BufReader::with_capacity(IO_BUFFER_BYTES, File::open(input_path)?);
    let limited = input.take(input_size);
    let mut reader = ProgressReader::new(limited, phase, Some(input_size));
    let mut writer = BufWriter::with_capacity(IO_BUFFER_BYTES, File::create(output_path)?);
    let read =
        zstd_backend::compress_stream(&mut reader, &mut writer, zstd_backend::DEFAULT_LEVEL)?;
    if read != input_size {
        return Err(DatapackError::InvalidFormat(format!(
            "benchmark read {read} bytes, expected {input_size}"
        )));
    }
    writer.flush()?;
    reader.finish();
    Ok(std::fs::metadata(output_path)?.len())
}

fn restore_v1_raw_path(input_path: &Path, output_path: &Path, phase: &str) -> Result<u64> {
    let input = File::open(input_path)?;
    let mut reader = BufReader::with_capacity(IO_BUFFER_BYTES, input);
    let metadata = storage::read_v1_archive_header(&mut reader)?;
    let output = File::create(output_path)?;
    let mut writer = BufWriter::with_capacity(IO_BUFFER_BYTES, output);
    let mut progress_writer = ProgressWriter::new(&mut writer, phase, Some(metadata.original_size));
    let restored = storage::restore_raw_zstd_stream(&metadata, &mut reader, &mut progress_writer)?;
    progress_writer.flush()?;
    progress_writer.finish();
    Ok(restored)
}

fn sha256_path_prefix(path: &Path, input_size: u64, phase: &str) -> Result<String> {
    let input = BufReader::with_capacity(IO_BUFFER_BYTES, File::open(path)?);
    let limited = input.take(input_size);
    let mut reader = ProgressReader::new(limited, phase, Some(input_size));
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; IO_BUFFER_BYTES];
    let mut total = 0u64;
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        total = total.saturating_add(read as u64);
    }
    reader.finish();
    if total != input_size {
        return Err(DatapackError::InvalidFormat(format!(
            "hash read {total} bytes, expected {input_size}"
        )));
    }
    Ok(digest_hex(hasher.finalize()))
}

fn copy_path_prefix(
    input_path: &Path,
    output_path: &Path,
    input_size: u64,
    phase: &str,
) -> Result<()> {
    let input = BufReader::with_capacity(IO_BUFFER_BYTES, File::open(input_path)?);
    let limited = input.take(input_size);
    let mut reader = ProgressReader::new(limited, phase, Some(input_size));
    let mut writer = BufWriter::with_capacity(IO_BUFFER_BYTES, File::create(output_path)?);
    let copied = std::io::copy(&mut reader, &mut writer)?;
    writer.flush()?;
    reader.finish();
    if copied != input_size {
        return Err(DatapackError::InvalidFormat(format!(
            "prefix copy read {copied} bytes, expected {input_size}"
        )));
    }
    Ok(())
}

pub(super) fn sha256_hex(bytes: &[u8]) -> String {
    digest_hex(Sha256::digest(bytes))
}

fn digest_hex(hash: impl AsRef<[u8]>) -> String {
    let hash = hash.as_ref();
    let mut output = String::with_capacity(hash.len() * 2);
    for byte in hash {
        output.push_str(&format!("{byte:02x}"));
    }
    output
}
