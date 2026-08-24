use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Write};
use std::time::{Duration, Instant};

use crate::analysis;
use crate::compression::planned::encode_for_plan_detailed;
use crate::compression::zstd_backend;
use crate::error::{DatapackError, Result};
use crate::planning::{ArchiveMode, ColumnExecutionPlan};
use crate::storage;

mod model;
mod streaming;
mod temp;

#[cfg(test)]
mod tests;

pub(crate) use model::{
    benchmark_partial_reasons, benchmark_scope, compression_ratio, compression_ratio_u64,
    median_duration, validation_status,
};
pub(crate) use model::{
    BenchmarkArtifacts, BenchmarkEvent, BenchmarkEventState, BenchmarkExecution, BenchmarkMetrics,
    BenchmarkNotice, BenchmarkRequest, ChunkedBenchmarkMetrics, ProfileTimings,
};
use streaming::sha256_hex;
pub(crate) use temp::BenchmarkTempFiles;
use temp::BenchmarkTempPaths;

const LARGE_BENCHMARK_WARNING_BYTES: u64 = 1024 * 1024 * 1024;
const DEFAULT_SAMPLE_MB: u64 = 64;
const DEFAULT_MAX_DICTIONARY_VALUES: u64 = 65_535;
const DEFAULT_MAX_DICTIONARY_MB: u64 = 64;
const IO_BUFFER_BYTES: usize = 256 * 1024;
const PROGRESS_INTERVAL_BYTES: u64 = 8 * 1024 * 1024;

fn warn_for_large_full_benchmark(
    source_size: u64,
    options: &BenchmarkRequest,
    events: &mut dyn FnMut(BenchmarkEvent),
) {
    let full_flow = !options.estimate_only
        && options.max_input_mb.is_none()
        && !options.no_zstd_baseline
        && !options.no_roundtrip
        && !options.no_hash;
    if source_size > LARGE_BENCHMARK_WARNING_BYTES && full_flow {
        events(BenchmarkEvent::Notice(BenchmarkNotice::LargeInput {
            source_size_bytes: source_size,
        }));
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

pub(crate) fn execute(options: BenchmarkRequest) -> Result<BenchmarkExecution> {
    execute_with_events(options, &mut |_| {})
}

pub(crate) fn execute_with_events(
    options: BenchmarkRequest,
    events: &mut dyn FnMut(BenchmarkEvent),
) -> Result<BenchmarkExecution> {
    let total_started = Instant::now();
    let mut profile_timings = ProfileTimings::default();
    let input = &options.input;
    let source_size = std::fs::metadata(input)?.len();
    warn_for_large_full_benchmark(source_size, &options, events);
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
    let mut analysis =
        analysis::analyze_path_with_scope(input, planning_sample_mb, benchmark_input_size)?;
    let analysis_was_limited = analysis.requires_raw_fallback();
    if !analysis_was_limited
        && !analysis::comma_structured_compression_eligible_path(
            input,
            planning_sample_mb,
            Some(benchmark_input_size),
        )?
    {
        analysis.apply_comma_eligibility_fallback();
        events(BenchmarkEvent::Notice(
            BenchmarkNotice::CommaEligibilityFallback,
        ));
    }
    profile_timings.planning_ms = Some(elapsed_ms(phase_started));
    events(BenchmarkEvent::Phase {
        name: "planning",
        state: BenchmarkEventState::Completed,
        processed_bytes: 0,
        total_bytes: None,
        started: phase_started,
    });
    if analysis_was_limited {
        let limitation_codes = analysis
            .facts
            .limitations
            .iter()
            .map(|limitation| limitation.code())
            .collect::<Vec<_>>();
        events(BenchmarkEvent::Notice(BenchmarkNotice::AnalysisLimited {
            limitation_codes,
        }));
    }
    let estimated_mode = analysis.plan.archive_mode;

    if options.estimate_only {
        let partial_reasons =
            vec!["estimate-only: compression, decompression, and hashing skipped"];
        let total_elapsed_ms = elapsed_ms(total_started);
        profile_timings.total_elapsed_ms = Some(total_elapsed_ms);
        return Ok(BenchmarkExecution::EstimateOnly {
            analysis,
            partial_reasons,
            total_elapsed_ms,
            profile_timings,
        });
    }

    if estimated_mode == ArchiveMode::RawZstd {
        return streaming::execute(
            options,
            analysis,
            source_size,
            benchmark_input_size,
            input_sampled,
            runs_used,
            total_started,
            profile_timings,
            events,
        );
    }
    let execution_plan = ColumnExecutionPlan::from_compression_plan(
        &analysis.plan,
        DEFAULT_MAX_DICTIONARY_VALUES,
        DEFAULT_MAX_DICTIONARY_MB,
    );

    let phase_started = Instant::now();
    let bytes = read_prefix_buffered(
        input,
        Some(benchmark_input_size),
        "benchmark read input",
        events,
    )?;
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
        events(BenchmarkEvent::Phase {
            name: "zstd-only",
            state: BenchmarkEventState::Completed,
            processed_bytes: bytes.len() as u64,
            total_bytes: Some(bytes.len() as u64),
            started: phase_started,
        });
        (Some(compressed), Some(duration))
    };

    let mut compression_times = Vec::with_capacity(runs_used);
    let mut archive_write_times = Vec::with_capacity(runs_used);
    let mut datapack = Vec::new();
    let mut selected_mode = ArchiveMode::RawZstd;
    let mut columnar_candidate_error = None;
    let temp_paths = BenchmarkTempPaths::new(input);
    let temp_path = temp_paths.datapack.clone();
    let chunked_temp_path = temp_paths.chunked.clone();
    let chunked_restore_path = temp_paths.chunked_restore.clone();
    let chunked_sample_path = temp_paths.chunked_sample.clone();
    let _temp_files = BenchmarkTempFiles::new(options.keep_temp, temp_paths.owned_paths());
    for run_index in 0..runs_used {
        events(BenchmarkEvent::Notice(BenchmarkNotice::CompressRun {
            run: run_index + 1,
            total_runs: runs_used,
        }));
        let start = Instant::now();
        let (archive, mode, error) =
            encode_for_plan_detailed(input, &bytes, estimated_mode, b',', &execution_plan)?;
        compression_times.push(start.elapsed());
        events(BenchmarkEvent::Phase {
            name: "benchmark encode+compress",
            state: BenchmarkEventState::Completed,
            processed_bytes: bytes.len() as u64,
            total_bytes: Some(bytes.len() as u64),
            started: start,
        });

        let write_started = Instant::now();
        std::fs::write(&temp_path, &archive)?;
        archive_write_times.push(write_started.elapsed());
        events(BenchmarkEvent::Phase {
            name: "benchmark write archive",
            state: BenchmarkEventState::Completed,
            processed_bytes: archive.len() as u64,
            total_bytes: Some(archive.len() as u64),
            started: write_started,
        });

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
            events(BenchmarkEvent::Notice(BenchmarkNotice::DecompressRun {
                run: run_index + 1,
                total_runs: runs_used,
            }));
            let start = Instant::now();
            let decoded_archive = storage::decode_archive(&datapack)?;
            restored = storage::restore_archive(&decoded_archive)?;
            decompression_times.push(start.elapsed());
            events(BenchmarkEvent::Phase {
                name: "benchmark decompress+decode",
                state: BenchmarkEventState::Completed,
                processed_bytes: decoded_archive.payload.len() as u64,
                total_bytes: Some(decoded_archive.payload.len() as u64),
                started: start,
            });
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
        let mut chunk_options = build_chunked_compress_options(&options)?;
        chunk_options.force = true;
        chunk_options.keep_temp = options.keep_temp;
        let mut chunked_compression_times = Vec::with_capacity(runs_used);
        let mut chunked_decompression_times = Vec::with_capacity(runs_used);
        let mut chunked_archive_size = 0u64;
        let mut chunked_roundtrip_sha256_match = None;

        for run_index in 0..runs_used {
            events(BenchmarkEvent::Notice(
                BenchmarkNotice::ChunkedCompressRun {
                    run: run_index + 1,
                    total_runs: runs_used,
                },
            ));
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
                events(BenchmarkEvent::Notice(
                    BenchmarkNotice::ChunkedDecompressRun {
                        run: run_index + 1,
                        total_runs: runs_used,
                    },
                ));
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
        partial_reasons: benchmark_partial_reasons(&options, input_sampled),
        no_roundtrip: options.no_roundtrip,
        no_hash: options.no_hash,
    };
    profile_timings.total_elapsed_ms = Some(elapsed_ms(total_started));

    let artifacts = if options.keep_temp {
        BenchmarkArtifacts {
            datapack: Some(temp_path),
            restore: None,
            zstd: None,
            chunked: options.uses_chunked().then_some(chunked_temp_path),
            chunked_restore: (options.uses_chunked() && !options.no_roundtrip)
                .then_some(chunked_restore_path),
            chunked_sample: (options.uses_chunked() && input_sampled)
                .then_some(chunked_sample_path),
        }
    } else {
        BenchmarkArtifacts::default()
    };
    Ok(BenchmarkExecution::Measured {
        metrics: Box::new(metrics),
        profile_timings,
        mode_label: format!("{:?}", archive.metadata.payload_kind),
        artifacts,
    })
}

fn build_chunked_compress_options(
    options: &BenchmarkRequest,
) -> Result<storage::chunked::ChunkedCompressOptions> {
    let chunk_size_bytes = storage::chunked::chunk_size_mb_to_bytes(
        options
            .chunk_size_mb
            .unwrap_or(storage::chunked::DEFAULT_CHUNK_SIZE_MB),
    )?;
    let threads = options
        .threads
        .unwrap_or_else(storage::chunked::default_thread_count);
    let backend = options
        .backend
        .unwrap_or(storage::chunked::ChunkedBackend::ChunkedRawZstd);
    let outer_threads = if backend == storage::chunked::ChunkedBackend::ZstdMtExperimental {
        1
    } else {
        threads
    };
    storage::chunked::ChunkedCompressOptions::new_with_max_in_flight(
        chunk_size_bytes,
        threads,
        options
            .max_in_flight_chunks
            .unwrap_or_else(|| storage::chunked::default_max_in_flight_chunks(outer_threads)),
        options.adaptive_level,
        options.profile,
    )
    .and_then(|chunk_options| chunk_options.with_backend(backend))
}

fn read_prefix_buffered(
    path: &std::path::Path,
    max_bytes: Option<u64>,
    phase: &'static str,
    events: &mut dyn FnMut(BenchmarkEvent),
) -> Result<Vec<u8>> {
    let mut reader = BufReader::with_capacity(IO_BUFFER_BYTES, File::open(path)?);
    let mut bytes = Vec::new();
    let mut buffer = vec![0u8; IO_BUFFER_BYTES];
    let mut reporter = BenchmarkProgressReporter::new(phase, max_bytes, events);
    loop {
        let remaining = max_bytes
            .map(|limit| limit.saturating_sub(bytes.len() as u64))
            .unwrap_or(u64::MAX);
        if remaining == 0 {
            break;
        }
        let requested = buffer.len().min(remaining as usize);
        let read = reader.read(&mut buffer[..requested])?;
        if read == 0 {
            break;
        }
        bytes.extend_from_slice(&buffer[..read]);
        reporter.add_bytes(read as u64);
    }
    reporter.finish();
    Ok(bytes)
}

struct BenchmarkProgressReporter<'a> {
    phase: &'static str,
    total_bytes: Option<u64>,
    processed_bytes: u64,
    started: Instant,
    last_reported_bytes: u64,
    events: &'a mut dyn FnMut(BenchmarkEvent),
}

impl<'a> BenchmarkProgressReporter<'a> {
    fn new(
        phase: &'static str,
        total_bytes: Option<u64>,
        events: &'a mut dyn FnMut(BenchmarkEvent),
    ) -> Self {
        Self {
            phase,
            total_bytes,
            processed_bytes: 0,
            started: Instant::now(),
            last_reported_bytes: 0,
            events,
        }
    }

    fn add_bytes(&mut self, bytes: u64) {
        self.processed_bytes = self.processed_bytes.saturating_add(bytes);
        if self
            .processed_bytes
            .saturating_sub(self.last_reported_bytes)
            >= PROGRESS_INTERVAL_BYTES
        {
            self.report(BenchmarkEventState::Advanced);
            self.last_reported_bytes = self.processed_bytes;
        }
    }

    fn finish(&mut self) {
        self.report(BenchmarkEventState::Completed);
    }

    fn report(&mut self, state: BenchmarkEventState) {
        (self.events)(BenchmarkEvent::Phase {
            name: self.phase,
            state,
            processed_bytes: self.processed_bytes,
            total_bytes: self.total_bytes,
            started: self.started,
        });
    }
}

fn elapsed_ms(started: Instant) -> u64 {
    duration_ms(started.elapsed())
}

fn duration_ms(duration: Duration) -> u64 {
    duration.as_millis() as u64
}

fn mb_per_second(bytes: usize, duration: Duration) -> f64 {
    mb_per_second_u64(bytes as u64, duration)
}

fn mb_per_second_u64(bytes: u64, duration: Duration) -> f64 {
    let seconds = duration.as_secs_f64();
    if seconds <= 0.0 {
        0.0
    } else {
        bytes as f64 / 1_048_576.0 / seconds
    }
}
