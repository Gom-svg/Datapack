use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::Path;
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};

use crate::analysis::DatasetAnalysis;
use crate::application::control::{CancellationToken, OperationResult};
use crate::compression::zstd_backend;
use crate::error::{DatapackError, Result};
use crate::planning::ArchiveMode;
use crate::storage;

use super::model::{
    benchmark_partial_reasons, benchmark_scope, compression_ratio_u64, median_duration,
    validation_status, BenchmarkArtifacts, BenchmarkEvent, BenchmarkExecution, BenchmarkMetrics,
    BenchmarkNotice, BenchmarkRequest, ChunkedBenchmarkMetrics, ProfileTimings,
};
use super::temp::{BenchmarkTempFiles, BenchmarkTempPaths};
use super::{
    build_chunked_compress_options, duration_ms, elapsed_ms, mb_per_second_u64,
    BenchmarkProgressReporter, IO_BUFFER_BYTES,
};

#[allow(clippy::too_many_arguments)]
pub(super) fn execute(
    options: BenchmarkRequest,
    analysis: DatasetAnalysis,
    source_size: u64,
    benchmark_input_size: u64,
    input_sampled: bool,
    runs_used: usize,
    total_started: Instant,
    mut profile_timings: ProfileTimings,
    events: &mut dyn FnMut(BenchmarkEvent),
    cancellation: Option<&CancellationToken>,
) -> OperationResult<BenchmarkExecution> {
    checkpoint(cancellation)?;
    events(BenchmarkEvent::Notice(
        BenchmarkNotice::StreamingRawZstdExecution,
    ));
    checkpoint(cancellation)?;
    let input = &options.input;
    let hash_enabled = !options.no_hash && !options.no_roundtrip;
    let temp_paths = BenchmarkTempPaths::new(input);
    let temp_path = temp_paths.datapack.clone();
    let restore_path = temp_paths.restore.clone();
    let zstd_temp_path = temp_paths.zstd.clone();
    let chunked_temp_path = temp_paths.chunked.clone();
    let chunked_restore_path = temp_paths.chunked_restore.clone();
    let chunked_sample_path = temp_paths.chunked_sample.clone();
    let _temp_files = BenchmarkTempFiles::new(options.keep_temp, temp_paths.owned_paths());

    let original_hash = if hash_enabled {
        let started = Instant::now();
        let hash = sha256_path_prefix(input, benchmark_input_size, "benchmark hash input", events)?;
        checkpoint(cancellation)?;
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
            input,
            &zstd_temp_path,
            benchmark_input_size,
            "benchmark zstd-only",
            events,
        )?;
        checkpoint(cancellation)?;
        let duration = started.elapsed();
        profile_timings.zstd_only_ms = Some(duration_ms(duration));
        zstd_only_duration = Some(duration);
    }

    let mut compression_times = Vec::with_capacity(runs_used);
    let mut datapack_size = 0u64;
    for run_index in 0..runs_used {
        checkpoint(cancellation)?;
        events(BenchmarkEvent::Notice(BenchmarkNotice::CompressRun {
            run: run_index + 1,
            total_runs: runs_used,
        }));
        checkpoint(cancellation)?;
        let started = Instant::now();
        datapack_size = write_raw_zstd_prefix(
            input,
            &temp_path,
            benchmark_input_size,
            "benchmark raw-zstd compress",
            events,
        )?;
        checkpoint(cancellation)?;
        compression_times.push(started.elapsed());
    }

    let mut decompression_times = Vec::with_capacity(runs_used);
    if !options.no_roundtrip {
        for run_index in 0..runs_used {
            checkpoint(cancellation)?;
            events(BenchmarkEvent::Notice(BenchmarkNotice::DecompressRun {
                run: run_index + 1,
                total_runs: runs_used,
            }));
            checkpoint(cancellation)?;
            let started = Instant::now();
            restore_v1_raw_path(
                &temp_path,
                &restore_path,
                "benchmark raw-zstd decompress",
                events,
            )?;
            checkpoint(cancellation)?;
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
            events,
        )?;
        checkpoint(cancellation)?;
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
                input,
                &chunked_sample_path,
                benchmark_input_size,
                "benchmark write input prefix",
                events,
            )?;
            chunked_sample_path.as_path()
        } else {
            input.as_path()
        };
        let mut chunk_options = build_chunked_compress_options(&options)?;
        chunk_options.force = true;
        chunk_options.keep_temp = options.keep_temp;
        let mut chunked_compression_times = Vec::with_capacity(runs_used);
        let mut chunked_decompression_times = Vec::with_capacity(runs_used);
        let mut chunked_archive_size = 0u64;
        let mut chunked_roundtrip_sha256_match = None;

        for run_index in 0..runs_used {
            checkpoint(cancellation)?;
            events(BenchmarkEvent::Notice(
                BenchmarkNotice::ChunkedCompressRun {
                    run: run_index + 1,
                    total_runs: runs_used,
                },
            ));
            checkpoint(cancellation)?;
            let started = Instant::now();
            let (stats, _) = storage::chunked::encode_raw_zstd_chunked_file_with_control(
                chunked_input,
                &chunked_temp_path,
                chunk_options,
                cancellation,
                &mut |_| {},
            )?;
            chunked_compression_times.push(started.elapsed());
            chunked_archive_size = stats.archive_size_bytes;
        }

        if !options.no_roundtrip {
            for run_index in 0..runs_used {
                checkpoint(cancellation)?;
                events(BenchmarkEvent::Notice(
                    BenchmarkNotice::ChunkedDecompressRun {
                        run: run_index + 1,
                        total_runs: runs_used,
                    },
                ));
                checkpoint(cancellation)?;
                let started = Instant::now();
                storage::chunked::decode_raw_zstd_chunked_file_with_control(
                    &chunked_temp_path,
                    &chunked_restore_path,
                    storage::chunked::ChunkedDecompressOptions {
                        verify: hash_enabled,
                        force: true,
                        keep_temp: options.keep_temp,
                        ..storage::chunked::ChunkedDecompressOptions::default()
                    },
                    cancellation,
                    &mut |_| {},
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
        partial_reasons: benchmark_partial_reasons(&options, input_sampled),
        no_roundtrip: options.no_roundtrip,
        no_hash: options.no_hash,
    };
    profile_timings.total_elapsed_ms = Some(elapsed_ms(total_started));

    let artifacts = if options.keep_temp {
        BenchmarkArtifacts {
            datapack: Some(temp_path),
            restore: (!options.no_roundtrip).then_some(restore_path),
            zstd: (!options.no_zstd_baseline).then_some(zstd_temp_path),
            chunked: options.uses_chunked().then_some(chunked_temp_path),
            chunked_restore: (options.uses_chunked() && !options.no_roundtrip)
                .then_some(chunked_restore_path),
            chunked_sample: (options.uses_chunked() && input_sampled)
                .then_some(chunked_sample_path),
        }
    } else {
        BenchmarkArtifacts::default()
    };
    checkpoint(cancellation)?;
    Ok(BenchmarkExecution::Measured {
        metrics: Box::new(metrics),
        profile_timings,
        mode_label: "RawZstd".to_string(),
        artifacts,
    })
}

fn checkpoint(cancellation: Option<&CancellationToken>) -> OperationResult<()> {
    match cancellation {
        Some(cancellation) => cancellation.checkpoint(),
        None => Ok(()),
    }
}

fn write_raw_zstd_prefix(
    input_path: &Path,
    output_path: &Path,
    input_size: u64,
    phase: &'static str,
    events: &mut dyn FnMut(BenchmarkEvent),
) -> Result<u64> {
    let input = BufReader::with_capacity(IO_BUFFER_BYTES, File::open(input_path)?);
    let limited = input.take(input_size);
    let mut reader = BenchmarkProgressReader::new(limited, phase, Some(input_size), events);
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
    phase: &'static str,
    events: &mut dyn FnMut(BenchmarkEvent),
) -> Result<u64> {
    let input = BufReader::with_capacity(IO_BUFFER_BYTES, File::open(input_path)?);
    let limited = input.take(input_size);
    let mut reader = BenchmarkProgressReader::new(limited, phase, Some(input_size), events);
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

fn restore_v1_raw_path(
    input_path: &Path,
    output_path: &Path,
    phase: &'static str,
    events: &mut dyn FnMut(BenchmarkEvent),
) -> Result<u64> {
    let input = File::open(input_path)?;
    let mut reader = BufReader::with_capacity(IO_BUFFER_BYTES, input);
    let metadata = storage::read_v1_archive_header(&mut reader)?;
    let output = File::create(output_path)?;
    let mut writer = BufWriter::with_capacity(IO_BUFFER_BYTES, output);
    let mut progress_writer =
        BenchmarkProgressWriter::new(&mut writer, phase, Some(metadata.original_size), events);
    let restored = storage::restore_raw_zstd_stream(&metadata, &mut reader, &mut progress_writer)?;
    progress_writer.flush()?;
    progress_writer.finish();
    Ok(restored)
}

fn sha256_path_prefix(
    path: &Path,
    input_size: u64,
    phase: &'static str,
    events: &mut dyn FnMut(BenchmarkEvent),
) -> Result<String> {
    let input = BufReader::with_capacity(IO_BUFFER_BYTES, File::open(path)?);
    let limited = input.take(input_size);
    let mut reader = BenchmarkProgressReader::new(limited, phase, Some(input_size), events);
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
    phase: &'static str,
    events: &mut dyn FnMut(BenchmarkEvent),
) -> Result<()> {
    let input = BufReader::with_capacity(IO_BUFFER_BYTES, File::open(input_path)?);
    let limited = input.take(input_size);
    let mut reader = BenchmarkProgressReader::new(limited, phase, Some(input_size), events);
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

struct BenchmarkProgressReader<'a, R> {
    inner: R,
    reporter: BenchmarkProgressReporter<'a>,
}

impl<'a, R> BenchmarkProgressReader<'a, R> {
    fn new(
        inner: R,
        phase: &'static str,
        total_bytes: Option<u64>,
        events: &'a mut dyn FnMut(BenchmarkEvent),
    ) -> Self {
        Self {
            inner,
            reporter: BenchmarkProgressReporter::new(phase, total_bytes, events),
        }
    }

    fn finish(&mut self) {
        self.reporter.finish();
    }
}

impl<R: Read> Read for BenchmarkProgressReader<'_, R> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        let read = self.inner.read(buffer)?;
        self.reporter.add_bytes(read as u64);
        Ok(read)
    }
}

struct BenchmarkProgressWriter<'a, W> {
    inner: W,
    reporter: BenchmarkProgressReporter<'a>,
}

impl<'a, W> BenchmarkProgressWriter<'a, W> {
    fn new(
        inner: W,
        phase: &'static str,
        total_bytes: Option<u64>,
        events: &'a mut dyn FnMut(BenchmarkEvent),
    ) -> Self {
        Self {
            inner,
            reporter: BenchmarkProgressReporter::new(phase, total_bytes, events),
        }
    }

    fn finish(&mut self) {
        self.reporter.finish();
    }
}

impl<W: Write> Write for BenchmarkProgressWriter<'_, W> {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        let written = self.inner.write(buffer)?;
        self.reporter.add_bytes(written as u64);
        Ok(written)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
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
