use std::fs::File;
use std::io::{BufReader, BufWriter, Cursor, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use clap::{Parser, Subcommand, ValueEnum};
use sha2::{Digest, Sha256};

use crate::analysis::{self, DatasetAnalysis};
use crate::compression::zstd_backend;
use crate::error::{DatapackError, Result};
use crate::generation::{self, Profile};
use crate::metadata::{DpackMetadata, PayloadKind};
use crate::planning::{self, ArchiveMode};
use crate::storage;
use crate::tuning;

mod analysis_report;

const DEFAULT_SAMPLE_MB: u64 = 64;
const IO_BUFFER_BYTES: usize = 256 * 1024;
const PROGRESS_INTERVAL: Duration = Duration::from_secs(5);
const LARGE_BENCHMARK_WARNING_BYTES: u64 = 1024 * 1024 * 1024;

#[derive(Debug, Parser)]
#[command(name = "datapack")]
#[command(version)]
#[command(about = "Lossless flat-data compression for CSV, TXT, logs, and database exports")]
pub struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Analyze {
        input: PathBuf,
        /// Print the full preflight compression plan.
        #[arg(long)]
        plan: bool,
        /// Maximum sample size in MB, from 1 to 2048.
        #[arg(long, default_value_t = DEFAULT_SAMPLE_MB)]
        sample_mb: u64,
    },
    Compress {
        input: PathBuf,
        output: PathBuf,
        /// Compression planning mode.
        #[arg(long, value_enum, default_value_t = CompressMode::Fast)]
        mode: CompressMode,
        /// Maximum sample size in MB, from 1 to 2048.
        #[arg(long, default_value_t = DEFAULT_SAMPLE_MB)]
        sample_mb: u64,
        /// Always compare RawZstd and CSV columnar outputs and keep the smaller archive.
        #[arg(long)]
        verify_best: bool,
        /// Maximum in-memory dictionary entries per column.
        #[arg(long, default_value_t = 65_535)]
        max_dictionary_values: u64,
        /// Maximum estimated in-memory dictionary size per column.
        #[arg(long, default_value_t = 64)]
        max_dictionary_mb: u64,
        /// Write a v2 chunked RawZstd archive.
        #[arg(long)]
        chunked: bool,
        /// Chunk size in MB for v2 chunked RawZstd archives. Supplying this enables chunked mode.
        #[arg(long)]
        chunk_size_mb: Option<u64>,
        /// Worker count for v2 chunked RawZstd compression. Supplying this enables chunked mode.
        #[arg(long)]
        threads: Option<usize>,
        /// Bound chunks held across reader, workers, and ordered writer.
        #[arg(long)]
        max_in_flight_chunks: Option<usize>,
        /// Compression backend. Native zstd MT is experimental and still writes compatible v2 chunks.
        #[arg(long, value_enum)]
        backend: Option<ChunkedBackendArg>,
        /// Select zstd level per chunk with a deterministic byte-sample heuristic.
        #[arg(long)]
        adaptive_level: bool,
        /// Emit compression timing and throughput diagnostics to stderr.
        #[arg(long)]
        profile: bool,
        /// Replace an existing regular output file. Existing outputs are protected by default.
        #[arg(long)]
        force: bool,
        /// Preserve a sibling .partial file when compression fails.
        #[arg(long)]
        keep_temp: bool,
        /// Refuse chunked compression when max-in-flight * chunk-size exceeds N MiB.
        #[arg(long)]
        max_memory_mb: Option<u64>,
    },
    Decompress {
        input: PathBuf,
        output: PathBuf,
        /// UNSAFE: skip v2 per-chunk and global SHA256 verification.
        #[arg(long)]
        no_verify: bool,
        /// Emit decompression timing and throughput diagnostics to stderr.
        #[arg(long)]
        profile: bool,
        /// Refuse archives declaring an output larger than N MiB. No limit by default.
        #[arg(long)]
        max_output_mb: Option<u64>,
        /// Refuse v2 archives declaring more than N chunks. No user limit by default.
        #[arg(long)]
        max_chunks: Option<u64>,
        /// Refuse archives whose approximate lower-bound memory estimate exceeds N MiB.
        #[arg(long)]
        max_memory_mb: Option<u64>,
        /// Replace an existing regular output file. Existing outputs are protected by default.
        #[arg(long)]
        force: bool,
        /// Preserve a sibling .partial file when decompression fails.
        #[arg(long)]
        keep_temp: bool,
    },
    /// Generate deterministic fictitious CSV benchmark data.
    GenerateTestData {
        /// Profile: repetitive, realistic, high-cardinality, or random.
        profile: String,
        /// Output CSV path.
        output: PathBuf,
        /// Number of data rows to generate, from 1 to 5,000,000.
        #[arg(long, default_value = "10000", allow_hyphen_values = true)]
        rows: String,
        /// Optional deterministic seed.
        #[arg(long)]
        seed: Option<u64>,
    },
    /// Experimentally tune v2 chunk/thread settings for this hardware and input.
    Tune {
        input: PathBuf,
        /// CSV report path. Defaults to a collision-safe name beside the input.
        #[arg(long)]
        output: Option<PathBuf>,
        /// Comma-separated chunk sizes in MiB.
        #[arg(long, default_value = "32,64,128,256")]
        chunk_sizes_mb: String,
        /// Comma-separated worker counts; `max` means logical CPU count.
        #[arg(long, default_value = "1,2,4,8,max")]
        threads_list: String,
        /// Number of runs for every grid configuration.
        #[arg(long, default_value_t = 1)]
        runs: usize,
        /// Tune only the first N MiB and mark results sampled/partial.
        #[arg(long)]
        max_input_mb: Option<u64>,
        /// Skip decompression; the report is explicitly non-validating.
        #[arg(long)]
        skip_roundtrip: bool,
        /// Skip decompression hashes; the report is explicitly non-validating.
        #[arg(long)]
        no_hash: bool,
        /// Select zstd level per chunk with the deterministic heuristic.
        #[arg(long)]
        adaptive_level: bool,
        /// Preserve tuning samples, archives, and restored outputs.
        #[arg(long)]
        keep_temp: bool,
        /// Emit pipeline progress and diagnostics while tuning.
        #[arg(long)]
        profile: bool,
        /// V2 backend to tune. Native zstd MT remains experimental.
        #[arg(long, value_enum, default_value_t = ChunkedBackendArg::ChunkedRawZstd)]
        backend: ChunkedBackendArg,
        /// Override the bounded number of chunks held by the pipeline.
        #[arg(long)]
        max_in_flight_chunks: Option<usize>,
        /// Allow replacement of an existing explicit report path.
        #[arg(long)]
        force: bool,
    },
    /// Benchmark DataPack against zstd-only compression.
    Benchmark {
        input: PathBuf,
        /// Emit machine-readable JSON.
        #[arg(long)]
        json: bool,
        /// Keep temporary benchmark artifacts for debugging.
        #[arg(long)]
        keep_temp: bool,
        /// Run a single benchmark iteration.
        #[arg(long)]
        quick: bool,
        /// Number of benchmark timing runs.
        #[arg(long, default_value_t = 3)]
        runs: usize,
        /// Emit detailed timing diagnostics to stderr.
        #[arg(long)]
        profile: bool,
        /// Include v2 chunked RawZstd in benchmark output.
        #[arg(long)]
        chunked: bool,
        /// Chunk size in MB for the v2 chunked benchmark.
        #[arg(long)]
        chunk_size_mb: Option<u64>,
        /// Worker count for the v2 chunked benchmark.
        #[arg(long)]
        threads: Option<usize>,
        /// Bound chunks held across the v2 benchmark pipeline.
        #[arg(long)]
        max_in_flight_chunks: Option<usize>,
        /// V2 compression backend to benchmark. Native zstd MT is experimental.
        #[arg(long, value_enum)]
        backend: Option<ChunkedBackendArg>,
        /// Use deterministic per-chunk zstd level selection in the v2 benchmark.
        #[arg(long)]
        adaptive_level: bool,
        /// Skip the standalone zstd comparison. DataPack validation still runs unless disabled separately.
        #[arg(long)]
        no_zstd_baseline: bool,
        /// Skip decompression and round-trip validation. This makes the benchmark non-validating.
        #[arg(long, visible_alias = "skip-full-roundtrip")]
        no_roundtrip: bool,
        /// Skip SHA256 validation. Decompression may still be timed, but identity is not validated.
        #[arg(long)]
        no_hash: bool,
        /// Run planning only; do not compress, decompress, or hash input data.
        #[arg(long)]
        estimate_only: bool,
        /// Benchmark only the first N MiB. Results describe the prefix, not the full input.
        #[arg(long)]
        max_input_mb: Option<u64>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum CompressMode {
    Fast,
    Best,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum ChunkedBackendArg {
    ChunkedRawZstd,
    ZstdMtExperimental,
}

impl ChunkedBackendArg {
    fn storage_backend(self) -> storage::chunked::ChunkedBackend {
        match self {
            Self::ChunkedRawZstd => storage::chunked::ChunkedBackend::ChunkedRawZstd,
            Self::ZstdMtExperimental => storage::chunked::ChunkedBackend::ZstdMtExperimental,
        }
    }

    fn tuning_backend(self) -> tuning::TuneBackend {
        match self {
            Self::ChunkedRawZstd => tuning::TuneBackend::ChunkedRawZstd,
            Self::ZstdMtExperimental => tuning::TuneBackend::ZstdMtExperimental,
        }
    }
}

pub fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Command::Analyze {
            input,
            plan,
            sample_mb,
        } => analyze_command(input, plan, sample_mb),
        Command::Compress {
            input,
            output,
            mode,
            sample_mb,
            verify_best,
            max_dictionary_values,
            max_dictionary_mb,
            chunked,
            chunk_size_mb,
            threads,
            max_in_flight_chunks,
            backend,
            adaptive_level,
            profile,
            force,
            keep_temp,
            max_memory_mb,
        } => compress_command(
            input,
            output,
            CompressOptions {
                mode,
                sample_mb,
                verify_best,
                max_dictionary_values,
                max_dictionary_mb,
                chunked,
                chunk_size_mb,
                threads,
                max_in_flight_chunks,
                backend,
                adaptive_level,
                profile,
                force,
                keep_temp,
                max_memory_mb,
            },
        ),
        Command::Decompress {
            input,
            output,
            no_verify,
            profile,
            max_output_mb,
            max_chunks,
            max_memory_mb,
            force,
            keep_temp,
        } => decompress_command(
            input,
            output,
            DecompressOptions {
                no_verify,
                profile,
                max_output_mb,
                max_chunks,
                max_memory_mb,
                force,
                keep_temp,
            },
        ),
        Command::GenerateTestData {
            profile,
            output,
            rows,
            seed,
        } => generate_test_data_command(profile, output, rows, seed),
        Command::Tune {
            input,
            output,
            chunk_sizes_mb,
            threads_list,
            runs,
            max_input_mb,
            skip_roundtrip,
            no_hash,
            adaptive_level,
            keep_temp,
            profile,
            backend,
            max_in_flight_chunks,
            force,
        } => tune_command(
            input,
            tuning::TuneOptions {
                output,
                chunk_sizes_mb: parse_tune_chunk_sizes(&chunk_sizes_mb)?,
                threads: parse_tune_threads(&threads_list)?,
                runs,
                max_input_mb,
                skip_roundtrip,
                no_hash,
                adaptive_level,
                keep_temp,
                profile,
                max_in_flight_chunks,
                force,
                backend: backend.tuning_backend(),
            },
        ),
        Command::Benchmark {
            input,
            json,
            keep_temp,
            quick,
            runs,
            profile,
            chunked,
            chunk_size_mb,
            threads,
            max_in_flight_chunks,
            backend,
            adaptive_level,
            no_zstd_baseline,
            no_roundtrip,
            no_hash,
            estimate_only,
            max_input_mb,
        } => benchmark_command(
            input,
            BenchmarkOptions {
                json,
                keep_temp,
                quick,
                runs,
                profile,
                chunked,
                chunk_size_mb,
                threads,
                max_in_flight_chunks,
                backend,
                adaptive_level,
                no_zstd_baseline,
                no_roundtrip,
                no_hash,
                estimate_only,
                max_input_mb,
            },
        ),
    }
}

#[derive(Debug, Clone, Copy)]
struct CompressOptions {
    mode: CompressMode,
    sample_mb: u64,
    verify_best: bool,
    max_dictionary_values: u64,
    max_dictionary_mb: u64,
    chunked: bool,
    chunk_size_mb: Option<u64>,
    threads: Option<usize>,
    max_in_flight_chunks: Option<usize>,
    backend: Option<ChunkedBackendArg>,
    adaptive_level: bool,
    profile: bool,
    force: bool,
    keep_temp: bool,
    max_memory_mb: Option<u64>,
}

#[derive(Debug, Clone, Copy)]
struct DecompressOptions {
    no_verify: bool,
    profile: bool,
    max_output_mb: Option<u64>,
    max_chunks: Option<u64>,
    max_memory_mb: Option<u64>,
    force: bool,
    keep_temp: bool,
}

impl CompressOptions {
    fn uses_chunked(self) -> bool {
        self.chunked
            || self.chunk_size_mb.is_some()
            || self.threads.is_some()
            || self.max_in_flight_chunks.is_some()
            || self.backend.is_some()
            || self.adaptive_level
            || self.max_memory_mb.is_some()
    }
}

#[derive(Debug, Clone, Copy)]
struct BenchmarkOptions {
    json: bool,
    keep_temp: bool,
    quick: bool,
    runs: usize,
    profile: bool,
    chunked: bool,
    chunk_size_mb: Option<u64>,
    threads: Option<usize>,
    max_in_flight_chunks: Option<usize>,
    backend: Option<ChunkedBackendArg>,
    adaptive_level: bool,
    no_zstd_baseline: bool,
    no_roundtrip: bool,
    no_hash: bool,
    estimate_only: bool,
    max_input_mb: Option<u64>,
}

impl BenchmarkOptions {
    fn uses_chunked(self) -> bool {
        self.chunked
            || self.chunk_size_mb.is_some()
            || self.threads.is_some()
            || self.max_in_flight_chunks.is_some()
            || self.backend.is_some()
            || self.adaptive_level
    }
}

struct DirectProfile<'a> {
    operation: &'a str,
    archive_version: u16,
    mode: &'a str,
    backend: &'a str,
    verify_enabled: Option<bool>,
    input_size_bytes: u64,
    output_size_bytes: u64,
    planning_ms: Option<u64>,
    read_ms: Option<u64>,
    transform_ms: u64,
    write_ms: Option<u64>,
    total_ms: u64,
    throughput_mb_per_sec: f64,
}

fn analyze_command(input: PathBuf, include_plan: bool, sample_mb: u64) -> Result<()> {
    let analysis = analysis::analyze_path(&input, sample_mb)?;
    analysis_report::print_planning_analysis(&analysis, include_plan);
    Ok(())
}

fn compress_command(input: PathBuf, output: PathBuf, options: CompressOptions) -> Result<()> {
    validate_input_output_paths(&input, &output)?;
    validate_output_overwrite_policy(&output, options.force)?;
    if let Some(chunk_size_mb) = options.chunk_size_mb {
        validate_chunk_size(chunk_size_mb)?;
    }
    if let Some(threads) = options.threads {
        validate_thread_count(threads)?;
    }
    if let Some(max_in_flight) = options.max_in_flight_chunks {
        validate_max_in_flight_chunks(max_in_flight)?;
    }
    if let Some(backend) = options.backend {
        validate_backend_support(backend.storage_backend().as_str())?;
    }
    if let Some(max_memory_mb) = options.max_memory_mb {
        validate_nonzero_megabyte_limit("--max-memory-mb", max_memory_mb)?;
    }
    let output_existed = output.exists();
    compress_command_inner(&input, &output, options)
        .map_err(|error| operation_failed("compression", &input, &output, output_existed, error))
}

fn compress_command_inner(input: &Path, output: &Path, options: CompressOptions) -> Result<()> {
    let total_started = Instant::now();
    if options.uses_chunked() {
        let input_size = std::fs::metadata(input)?.len();
        let mut chunk_options = build_chunked_compress_options(
            options.chunk_size_mb,
            options.threads,
            options.max_in_flight_chunks,
            options.backend,
            options.adaptive_level,
            options.profile,
        )?;
        validate_compression_memory_limit(&chunk_options, options.max_memory_mb)?;
        chunk_options.force = options.force;
        chunk_options.keep_temp = options.keep_temp;
        let transform_started = Instant::now();
        let stats = storage::chunked::encode_raw_zstd_chunked_file(input, output, chunk_options)?;
        let transform_elapsed = transform_started.elapsed();
        if options.profile {
            print_chunked_profile(
                "compress",
                &stats,
                input_size,
                stats.archive_size_bytes,
                elapsed_ms(total_started),
                mb_per_second_u64(input_size, transform_elapsed),
            );
        }
        return Ok(());
    }

    let phase_started = Instant::now();
    let analysis = analysis::analyze_path(input, options.sample_mb)?;
    let planning_ms = elapsed_ms(phase_started);
    let mut plan = analysis.plan.clone();
    for column in planning::apply_dictionary_limits(
        &mut plan,
        &analysis.columns,
        options.max_dictionary_values,
        options.max_dictionary_mb,
    ) {
        eprintln!("Column '{column}' exceeded dictionary limit; switching to Plain.");
    }
    progress_phase("planning", 0, None, phase_started);

    let compares_candidates = options.verify_best
        || (options.mode == CompressMode::Best && plan.estimated_savings_percent < 15.0);
    if plan.archive_mode == ArchiveMode::RawZstd && !compares_candidates {
        let input_size = std::fs::metadata(input)?.len();
        let file = File::open(input)?;
        let reader = BufReader::with_capacity(IO_BUFFER_BYTES, file);
        let mut progress_reader =
            ProgressReader::new(reader, "raw-zstd-compress", Some(input_size));
        let (mut temp_output, temp_file) =
            storage::output::TempOutput::create(output, options.keep_temp)?;
        let mut writer = BufWriter::with_capacity(IO_BUFFER_BYTES, temp_file);
        let transform_started = Instant::now();
        storage::write_raw_zstd_archive_stream(
            input,
            input_size,
            &mut progress_reader,
            &mut writer,
        )?;
        writer.flush()?;
        let output_size = writer.get_ref().metadata()?.len();
        drop(writer);
        temp_output.commit(output, options.force)?;
        progress_reader.finish();
        let transform_elapsed = transform_started.elapsed();
        if options.profile {
            print_direct_profile(&DirectProfile {
                operation: "compress",
                archive_version: 1,
                mode: ArchiveMode::RawZstd.as_str(),
                backend: "raw-zstd-streaming",
                verify_enabled: None,
                input_size_bytes: input_size,
                output_size_bytes: output_size,
                planning_ms: Some(planning_ms),
                read_ms: None,
                transform_ms: duration_ms(transform_elapsed),
                write_ms: None,
                total_ms: elapsed_ms(total_started),
                throughput_mb_per_sec: mb_per_second_u64(input_size, transform_elapsed),
            });
        }
        return Ok(());
    }

    let read_started = Instant::now();
    let bytes = read_all_buffered_progress(input, "read input")?;
    let read_ms = elapsed_ms(read_started);
    let phase_started = Instant::now();
    let (archive, selected_mode) = if options.verify_best {
        let (archive, selected, saved, columnar_error) = encode_best_archive(input, &bytes)?;
        eprintln!(
            "verify-best: selected {}, saved {saved} bytes over alternative.",
            selected.as_str()
        );
        if let Some(error) = columnar_error {
            eprintln!("columnar_candidate_error = {error:?}");
        }
        (archive, selected)
    } else if options.mode == CompressMode::Best && plan.estimated_savings_percent < 15.0 {
        let (archive, selected, _, _) = encode_best_archive(input, &bytes)?;
        (archive, selected)
    } else {
        let archive = encode_for_plan(input, &bytes, plan.archive_mode)?;
        let metadata = storage::read_v1_archive_header(&mut Cursor::new(&archive))?;
        (archive, archive_mode_for_payload(&metadata.payload_kind))
    };
    let transform_elapsed = phase_started.elapsed();
    progress_phase(
        "encode+compress",
        bytes.len() as u64,
        Some(bytes.len() as u64),
        phase_started,
    );

    let (mut temp_output, temp_file) =
        storage::output::TempOutput::create(output, options.keep_temp)?;
    let mut writer = BufWriter::with_capacity(IO_BUFFER_BYTES, temp_file);
    let phase_started = Instant::now();
    writer.write_all(&archive)?;
    writer.flush()?;
    drop(writer);
    temp_output.commit(output, options.force)?;
    let write_ms = elapsed_ms(phase_started);
    progress_phase(
        "write archive",
        archive.len() as u64,
        Some(archive.len() as u64),
        phase_started,
    );
    if options.profile {
        print_direct_profile(&DirectProfile {
            operation: "compress",
            archive_version: 1,
            mode: selected_mode.as_str(),
            backend: "zstd",
            verify_enabled: None,
            input_size_bytes: bytes.len() as u64,
            output_size_bytes: archive.len() as u64,
            planning_ms: Some(planning_ms),
            read_ms: Some(read_ms),
            transform_ms: duration_ms(transform_elapsed),
            write_ms: Some(write_ms),
            total_ms: elapsed_ms(total_started),
            throughput_mb_per_sec: mb_per_second_u64(bytes.len() as u64, transform_elapsed),
        });
    }
    Ok(())
}

fn decompress_command(input: PathBuf, output: PathBuf, options: DecompressOptions) -> Result<()> {
    validate_input_output_paths(&input, &output)?;
    validate_output_overwrite_policy(&output, options.force)?;
    if let Some(max_memory_mb) = options.max_memory_mb {
        validate_nonzero_megabyte_limit("--max-memory-mb", max_memory_mb)?;
    }
    let output_existed = output.exists();
    decompress_command_inner(&input, &output, options)
        .map_err(|error| operation_failed("decompression", &input, &output, output_existed, error))
}

fn decompress_command_inner(input: &Path, output: &Path, options: DecompressOptions) -> Result<()> {
    let total_started = Instant::now();
    let archive_size = std::fs::metadata(input)?.len();
    let version =
        storage::archive_version_from_path(input).map_err(|error| DatapackError::ArchiveParse {
            path: input.to_path_buf(),
            reason: error.to_string(),
        })?;
    if version == storage::chunked::CHUNKED_VERSION {
        let transform_started = Instant::now();
        let stats = storage::chunked::decode_raw_zstd_chunked_file(
            input,
            output,
            storage::chunked::ChunkedDecompressOptions {
                verify: !options.no_verify,
                max_output_bytes: optional_megabytes_to_bytes(
                    "--max-output-mb",
                    options.max_output_mb,
                )?,
                max_chunks: options.max_chunks,
                max_memory_bytes: optional_megabytes_to_bytes(
                    "--max-memory-mb",
                    options.max_memory_mb,
                )?,
                force: options.force,
                keep_temp: options.keep_temp,
            },
        )?;
        let transform_elapsed = transform_started.elapsed();
        if options.profile {
            print_chunked_profile(
                "decompress",
                &stats,
                archive_size,
                stats.original_size_bytes,
                elapsed_ms(total_started),
                mb_per_second_u64(stats.original_size_bytes, transform_elapsed),
            );
        }
        return Ok(());
    }

    let file = File::open(input)?;
    let mut reader = BufReader::with_capacity(IO_BUFFER_BYTES, file);
    let metadata = storage::read_v1_archive_header(&mut reader).map_err(|error| {
        DatapackError::ArchiveParse {
            path: input.to_path_buf(),
            reason: error.to_string(),
        }
    })?;
    validate_v1_decompression_limits(&metadata, archive_size, options)?;
    if matches!(
        metadata.payload_kind,
        PayloadKind::RawZstd | PayloadKind::Plain | PayloadKind::Dictionary
    ) {
        let (mut temp_output, temp_file) =
            storage::output::TempOutput::create(output, options.keep_temp)?;
        let mut output_writer = BufWriter::with_capacity(IO_BUFFER_BYTES, temp_file);
        let mut progress_writer = ProgressWriter::new(
            &mut output_writer,
            "raw-zstd-decompress",
            Some(metadata.original_size),
        );
        let transform_started = Instant::now();
        let restored_size =
            storage::restore_raw_zstd_stream(&metadata, &mut reader, &mut progress_writer)?;
        progress_writer.flush()?;
        progress_writer.finish();
        drop(progress_writer);
        drop(output_writer);
        temp_output.commit(output, options.force)?;
        let transform_elapsed = transform_started.elapsed();
        if options.profile {
            print_direct_profile(&DirectProfile {
                operation: "decompress",
                archive_version: version,
                mode: archive_mode_for_payload(&metadata.payload_kind).as_str(),
                backend: "raw-zstd-streaming",
                verify_enabled: None,
                input_size_bytes: archive_size,
                output_size_bytes: restored_size,
                planning_ms: None,
                read_ms: None,
                transform_ms: duration_ms(transform_elapsed),
                write_ms: None,
                total_ms: elapsed_ms(total_started),
                throughput_mb_per_sec: mb_per_second_u64(restored_size, transform_elapsed),
            });
        }
        return Ok(());
    }

    let read_started = Instant::now();
    let archive_bytes = read_all_buffered_progress(input, "read archive")?;
    let read_ms = elapsed_ms(read_started);
    let archive =
        storage::decode_archive(&archive_bytes).map_err(|error| DatapackError::ArchiveParse {
            path: input.to_path_buf(),
            reason: error.to_string(),
        })?;

    let phase_started = Instant::now();
    let restored = storage::restore_archive(&archive)?;
    let transform_elapsed = phase_started.elapsed();
    progress_phase(
        "decompress+decode",
        archive.payload.len() as u64,
        Some(archive.payload.len() as u64),
        phase_started,
    );

    let phase_started = Instant::now();
    let (mut temp_output, temp_file) =
        storage::output::TempOutput::create(output, options.keep_temp)?;
    let mut writer = BufWriter::with_capacity(IO_BUFFER_BYTES, temp_file);
    writer.write_all(&restored)?;
    writer.flush()?;
    drop(writer);
    temp_output.commit(output, options.force)?;
    let write_ms = elapsed_ms(phase_started);
    progress_phase(
        "write restored",
        restored.len() as u64,
        Some(restored.len() as u64),
        phase_started,
    );
    if options.profile {
        print_direct_profile(&DirectProfile {
            operation: "decompress",
            archive_version: version,
            mode: archive_mode_for_payload(&archive.metadata.payload_kind).as_str(),
            backend: "zstd",
            verify_enabled: None,
            input_size_bytes: archive_size,
            output_size_bytes: restored.len() as u64,
            planning_ms: None,
            read_ms: Some(read_ms),
            transform_ms: duration_ms(transform_elapsed),
            write_ms: Some(write_ms),
            total_ms: elapsed_ms(total_started),
            throughput_mb_per_sec: mb_per_second_u64(restored.len() as u64, transform_elapsed),
        });
    }
    Ok(())
}

fn generate_test_data_command(
    profile: String,
    output: PathBuf,
    rows: String,
    seed: Option<u64>,
) -> Result<()> {
    let profile = Profile::parse(&profile)?;
    let rows = rows.parse::<u64>().unwrap_or(0);
    generation::generate_to_path(profile, &output, rows, seed)
}

fn parse_tune_chunk_sizes(value: &str) -> Result<Vec<u64>> {
    let values = parse_comma_list(value, "--chunk-sizes-mb")?;
    values
        .into_iter()
        .map(|value| {
            let parsed = value.parse::<u64>().map_err(|_| {
                DatapackError::InvalidFormat(format!(
                    "invalid --chunk-sizes-mb value '{value}'; expected positive MiB integers"
                ))
            })?;
            storage::chunked::chunk_size_mb_to_bytes(parsed)?;
            Ok(parsed)
        })
        .collect()
}

fn parse_tune_threads(value: &str) -> Result<Vec<usize>> {
    let values = parse_comma_list(value, "--threads-list")?;
    values
        .into_iter()
        .map(|value| {
            if value.eq_ignore_ascii_case("max") {
                return Ok(storage::chunked::default_thread_count());
            }
            let parsed = value.parse::<usize>().map_err(|_| {
                DatapackError::InvalidFormat(format!(
                    "invalid --threads-list value '{value}'; expected positive integers or max"
                ))
            })?;
            if parsed == 0 {
                return Err(DatapackError::InvalidFormat(
                    "--threads-list values must be greater than zero".to_string(),
                ));
            }
            Ok(parsed)
        })
        .collect()
}

fn parse_comma_list<'a>(value: &'a str, flag: &str) -> Result<Vec<&'a str>> {
    let values: Vec<_> = value.split(',').map(str::trim).collect();
    if values.is_empty() || values.iter().any(|value| value.is_empty()) {
        return Err(DatapackError::InvalidFormat(format!(
            "{flag} must be a comma-separated list without empty values"
        )));
    }
    Ok(values)
}

fn tune_command(input: PathBuf, options: tuning::TuneOptions) -> Result<()> {
    eprintln!(
        "experimental tune: recommendations are hardware- and dataset-specific; defaults will not be changed automatically."
    );
    let summary = tuning::tune(&input, options)?;
    println!("Tune report: {}", summary.report_path.display());
    println!("Input size: {} bytes", summary.input_size_bytes);
    println!(
        "Measured input: {} bytes ({})",
        summary.measured_input_size_bytes,
        if summary.input_sampled {
            "sampled"
        } else {
            "full"
        }
    );
    println!(
        "Runs: {} successful, {} failed",
        summary.successful_runs, summary.failed_runs
    );
    print_tune_recommendation("Best throughput", summary.best_throughput.as_ref());
    print_tune_recommendation("Best compression ratio", summary.best_ratio.as_ref());
    print_tune_recommendation("Balanced", summary.balanced.as_ref());
    if !summary.temp_paths.is_empty() {
        println!("Preserved temporary files:");
        for path in &summary.temp_paths {
            println!("  {}", path.display());
        }
    }
    if summary.successful_runs == 0 {
        return Err(DatapackError::InvalidFormat(format!(
            "all tune runs failed; inspect {}",
            summary.report_path.display()
        )));
    }
    Ok(())
}

fn print_tune_recommendation(label: &str, recommendation: Option<&tuning::TuneRecommendation>) {
    println!("{label}:");
    match recommendation {
        Some(value) => {
            println!("  backend={}", value.backend.as_str());
            println!("  chunk_size_mb={}", value.chunk_size_mb);
            println!("  threads={}", value.threads);
            println!("  max_in_flight_chunks={}", value.max_in_flight_chunks);
            println!("  throughput={:.3} MB/s", value.compression_mb_per_sec);
            println!("  ratio={:.4}x", value.compression_ratio);
            println!(
                "  peak_memory_estimate_mb={:.1}",
                value.peak_memory_estimate_mb
            );
            if let Some(score) = value.balanced_score {
                println!("  balanced_score={score:.4}");
            }
        }
        None => println!("  unavailable (no successful runs)"),
    }
}

fn build_chunked_compress_options(
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

fn benchmark_partial_reasons(options: &BenchmarkOptions, input_sampled: bool) -> Vec<&'static str> {
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

fn benchmark_scope(options: &BenchmarkOptions, input_sampled: bool) -> &'static str {
    if input_sampled {
        "sampled"
    } else if benchmark_partial_reasons(options, false).is_empty() {
        "full"
    } else {
        "partial"
    }
}

fn validation_status(
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

fn print_estimate_only_table(
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

fn print_estimate_only_json(
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

#[allow(clippy::too_many_arguments)]
fn benchmark_raw_zstd_streaming(
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
    input_path: &std::path::Path,
    output_path: &std::path::Path,
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
    input_path: &std::path::Path,
    output_path: &std::path::Path,
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

fn restore_v1_raw_path(
    input_path: &std::path::Path,
    output_path: &std::path::Path,
    phase: &str,
) -> Result<u64> {
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

fn sha256_path_prefix(path: &std::path::Path, input_size: u64, phase: &str) -> Result<String> {
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
    input_path: &std::path::Path,
    output_path: &std::path::Path,
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

fn benchmark_command(input: PathBuf, options: BenchmarkOptions) -> Result<()> {
    let total_started = Instant::now();
    let mut profile_timings = ProfileTimings::default();
    let source_size = std::fs::metadata(&input)?.len();
    warn_for_large_full_benchmark(source_size, &options);
    let runs_used = if options.quick {
        1
    } else {
        options.runs.max(1)
    };

    let phase_started = Instant::now();
    let analysis = analysis::analyze_path(&input, DEFAULT_SAMPLE_MB)?;
    profile_timings.planning_ms = Some(elapsed_ms(phase_started));
    progress_phase("planning", 0, None, phase_started);
    let estimated_mode = analysis.plan.archive_mode;
    let max_input_bytes = benchmark_max_input_bytes(options.max_input_mb)?;

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

    let benchmark_input_size = max_input_bytes
        .map(|limit| source_size.min(limit))
        .unwrap_or(source_size);
    let input_sampled = benchmark_input_size < source_size;

    if estimated_mode == ArchiveMode::RawZstd {
        return benchmark_raw_zstd_streaming(
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

fn encode_for_plan(input: &std::path::Path, bytes: &[u8], mode: ArchiveMode) -> Result<Vec<u8>> {
    Ok(encode_for_plan_detailed(input, bytes, mode)?.0)
}

fn encode_for_plan_detailed(
    input: &std::path::Path,
    bytes: &[u8],
    mode: ArchiveMode,
) -> Result<(Vec<u8>, ArchiveMode, Option<String>)> {
    match mode {
        ArchiveMode::RawZstd => Ok((
            storage::encode_raw_zstd_archive(input, bytes)?,
            ArchiveMode::RawZstd,
            None,
        )),
        ArchiveMode::CsvColumnarDictionary => {
            let (archive, error) =
                storage::encode_columnar_dictionary_archive_detailed(input, bytes)?;
            match archive {
                Some(archive) => Ok((archive, ArchiveMode::CsvColumnarDictionary, error)),
                None => Ok((
                    storage::encode_raw_zstd_archive(input, bytes)?,
                    ArchiveMode::RawZstd,
                    error,
                )),
            }
        }
    }
}

fn archive_mode_for_payload(payload_kind: &PayloadKind) -> ArchiveMode {
    match payload_kind {
        PayloadKind::CsvColumnarDictionary => ArchiveMode::CsvColumnarDictionary,
        PayloadKind::RawZstd | PayloadKind::Plain | PayloadKind::Dictionary => ArchiveMode::RawZstd,
    }
}

fn encode_best_archive(
    input: &std::path::Path,
    bytes: &[u8],
) -> Result<(Vec<u8>, ArchiveMode, usize, Option<String>)> {
    let raw = storage::encode_raw_zstd_archive(input, bytes)?;
    let (columnar, columnar_error) =
        storage::encode_columnar_dictionary_archive_detailed(input, bytes)?;
    if let Some(columnar) = columnar {
        if columnar.len() < raw.len() {
            let saved = raw.len() - columnar.len();
            return Ok((columnar, ArchiveMode::CsvColumnarDictionary, saved, None));
        }
        let saved = columnar.len().saturating_sub(raw.len());
        Ok((raw, ArchiveMode::RawZstd, saved, columnar_error))
    } else {
        Ok((raw, ArchiveMode::RawZstd, 0, columnar_error))
    }
}

fn read_all_buffered_progress(path: &std::path::Path, phase: &str) -> Result<Vec<u8>> {
    read_prefix_buffered_progress(path, phase, None)
}

fn read_prefix_buffered_progress(
    path: &std::path::Path,
    phase: &str,
    max_bytes: Option<u64>,
) -> Result<Vec<u8>> {
    let file_size = std::fs::metadata(path).ok().map(|metadata| metadata.len());
    let total = match (file_size, max_bytes) {
        (Some(file_size), Some(max_bytes)) => Some(file_size.min(max_bytes)),
        (Some(file_size), None) => Some(file_size),
        (None, max_bytes) => max_bytes,
    };
    let mut reader = BufReader::with_capacity(IO_BUFFER_BYTES, File::open(path)?);
    let mut bytes = Vec::new();
    let mut buffer = vec![0u8; IO_BUFFER_BYTES];
    let mut reporter = ProgressReporter::new(phase, total);
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

fn progress_phase(phase: &str, processed_bytes: u64, total_bytes: Option<u64>, started: Instant) {
    let elapsed = started.elapsed().as_secs_f64().max(0.001);
    let mb = processed_bytes as f64 / 1_048_576.0;
    let throughput = mb / elapsed;
    match total_bytes {
        Some(total) => {
            let percent = if total == 0 {
                100.0
            } else {
                (processed_bytes as f64 / total as f64 * 100.0).clamp(0.0, 100.0)
            };
            let bytes_per_second = processed_bytes as f64 / elapsed;
            let eta_seconds = if processed_bytes < total && bytes_per_second > 0.0 {
                (total - processed_bytes) as f64 / bytes_per_second
            } else {
                0.0
            };
            eprintln!(
                "phase={phase} rows=unknown mb={:.2}/{:.2} percent={:.1} elapsed={:.1}s eta={:.1}s throughput={:.2} MB/s",
                mb,
                total as f64 / 1_048_576.0,
                percent,
                elapsed,
                eta_seconds,
                throughput
            );
        }
        None => eprintln!(
            "phase={phase} rows=unknown mb={:.2} elapsed={:.1}s throughput={:.2} MB/s",
            mb, elapsed, throughput
        ),
    }
}

struct ProgressReporter {
    phase: String,
    total_bytes: Option<u64>,
    processed_bytes: u64,
    started: Instant,
    last_report: Instant,
}

impl ProgressReporter {
    fn new(phase: &str, total_bytes: Option<u64>) -> Self {
        Self {
            phase: phase.to_string(),
            total_bytes,
            processed_bytes: 0,
            started: Instant::now(),
            last_report: Instant::now(),
        }
    }

    fn add_bytes(&mut self, bytes: u64) {
        self.processed_bytes = self.processed_bytes.saturating_add(bytes);
        if self.last_report.elapsed() >= PROGRESS_INTERVAL {
            self.report();
            self.last_report = Instant::now();
        }
    }

    fn finish(&self) {
        self.report();
    }

    fn report(&self) {
        progress_phase(
            &self.phase,
            self.processed_bytes,
            self.total_bytes,
            self.started,
        );
    }
}

struct ProgressReader<R> {
    inner: R,
    reporter: ProgressReporter,
}

impl<R> ProgressReader<R> {
    fn new(inner: R, phase: &str, total_bytes: Option<u64>) -> Self {
        Self {
            inner,
            reporter: ProgressReporter::new(phase, total_bytes),
        }
    }

    fn finish(&self) {
        self.reporter.finish();
    }
}

impl<R: Read> Read for ProgressReader<R> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        let read = self.inner.read(buffer)?;
        self.reporter.add_bytes(read as u64);
        Ok(read)
    }
}

struct ProgressWriter<W> {
    inner: W,
    reporter: ProgressReporter,
}

impl<W> ProgressWriter<W> {
    fn new(inner: W, phase: &str, total_bytes: Option<u64>) -> Self {
        Self {
            inner,
            reporter: ProgressReporter::new(phase, total_bytes),
        }
    }

    fn finish(&self) {
        self.reporter.finish();
    }
}

impl<W: Write> Write for ProgressWriter<W> {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        let written = self.inner.write(buffer)?;
        self.reporter.add_bytes(written as u64);
        Ok(written)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

fn compression_ratio(original_size: usize, compressed_size: usize) -> f64 {
    if compressed_size == 0 {
        0.0
    } else {
        original_size as f64 / compressed_size as f64
    }
}

fn compression_ratio_u64(original_size: u64, compressed_size: u64) -> f64 {
    if compressed_size == 0 {
        0.0
    } else {
        original_size as f64 / compressed_size as f64
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
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

#[derive(Debug, Clone)]
struct BenchmarkMetrics {
    source_size_bytes: u64,
    measured_input_size_bytes: u64,
    original_size_bytes: u64,
    datapack_size_bytes: u64,
    zstd_only_size_bytes: u64,
    zstd_only_compression_time_ms: Option<f64>,
    zstd_only_compression_mb_per_second: Option<f64>,
    compression_ratio: f64,
    zstd_only_ratio: f64,
    selected_mode: ArchiveMode,
    estimated_mode: ArchiveMode,
    plan_was_correct: bool,
    peak_memory_estimate_mb: Option<f32>,
    planning_time_ms: u64,
    columnar_candidate_error: Option<String>,
    runs_used: usize,
    total_elapsed_time_ms: u64,
    compression_time_ms: f64,
    decompression_time_ms: f64,
    compression_input_mb_per_second: f64,
    decompression_input_mb_per_second: f64,
    roundtrip_sha256_match: bool,
    datapack_beats_zstd: bool,
    chunked_raw_zstd: Option<ChunkedBenchmarkMetrics>,
    input_sampled: bool,
    zstd_baseline_performed: bool,
    roundtrip_performed: bool,
    hash_performed: bool,
    benchmark_scope: String,
    validation_status: String,
    partial_reasons: String,
    no_roundtrip: bool,
    no_hash: bool,
}

#[derive(Debug, Clone)]
struct ChunkedBenchmarkMetrics {
    backend: String,
    chunk_size_mb: f64,
    threads: usize,
    max_in_flight_chunks: usize,
    size_bytes: u64,
    compression_ratio: f64,
    compression_time_ms: f64,
    compression_mb_per_second: f64,
    decompression_time_ms: Option<f64>,
    decompression_mb_per_second: Option<f64>,
    roundtrip_sha256_match: Option<bool>,
}

fn median_duration(values: &[Duration]) -> Duration {
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

fn elapsed_ms(started: Instant) -> u64 {
    duration_ms(started.elapsed())
}

fn duration_ms(duration: Duration) -> u64 {
    duration.as_millis() as u64
}

fn mb_per_second(bytes: usize, duration: Duration) -> f64 {
    let seconds = duration.as_secs_f64();
    if seconds <= 0.0 {
        0.0
    } else {
        bytes as f64 / 1_048_576.0 / seconds
    }
}

fn mb_per_second_u64(bytes: u64, duration: Duration) -> f64 {
    let seconds = duration.as_secs_f64();
    if seconds <= 0.0 {
        0.0
    } else {
        bytes as f64 / 1_048_576.0 / seconds
    }
}

fn ratio(original_size: u64, compressed_size: u64) -> f64 {
    if compressed_size == 0 {
        0.0
    } else {
        original_size as f64 / compressed_size as f64
    }
}

fn print_direct_profile(profile: &DirectProfile<'_>) {
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

fn print_chunked_profile(
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

fn validate_input_output_paths(input: &Path, output: &Path) -> Result<()> {
    let input_metadata = std::fs::metadata(input).map_err(|error| {
        DatapackError::InvalidFormat(format!(
            "input path '{}' was not found or is not readable: {error}",
            input.display()
        ))
    })?;
    if !input_metadata.is_file() {
        return Err(DatapackError::InvalidFormat(format!(
            "input path '{}' is not a regular file",
            input.display()
        )));
    }

    let input = normalized_cli_path(input)?;
    let output = normalized_cli_path(output)?;
    let same = if cfg!(windows) {
        input
            .to_string_lossy()
            .eq_ignore_ascii_case(&output.to_string_lossy())
    } else {
        input == output
    };
    if same {
        return Err(DatapackError::InvalidFormat(format!(
            "output path '{}' must differ from input/archive path '{}'",
            output.display(),
            input.display()
        )));
    }

    let parent = output
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let parent_metadata = std::fs::metadata(parent).map_err(|error| {
        DatapackError::InvalidFormat(format!(
            "output parent directory '{}' does not exist or is inaccessible: {error}",
            parent.display()
        ))
    })?;
    if !parent_metadata.is_dir() {
        return Err(DatapackError::InvalidFormat(format!(
            "output parent '{}' is not a directory",
            parent.display()
        )));
    }
    Ok(())
}

fn validate_chunk_size(mb: u64) -> Result<()> {
    storage::chunked::chunk_size_mb_to_bytes(mb).map(|_| ())
}

fn validate_thread_count(n: usize) -> Result<()> {
    if n == 0 || n > storage::chunked::MAX_THREAD_COUNT {
        return Err(DatapackError::InvalidFormat(format!(
            "--threads must be between 1 and {}",
            storage::chunked::MAX_THREAD_COUNT
        )));
    }
    Ok(())
}

fn validate_max_in_flight_chunks(n: usize) -> Result<()> {
    if n == 0 || n > storage::chunked::MAX_IN_FLIGHT_CHUNKS {
        return Err(DatapackError::InvalidFormat(format!(
            "--max-in-flight-chunks must be between 1 and {}",
            storage::chunked::MAX_IN_FLIGHT_CHUNKS
        )));
    }
    Ok(())
}

fn validate_backend_support(name: &str) -> Result<()> {
    if matches!(name, "chunked-raw-zstd" | "zstd-mt-experimental") {
        Ok(())
    } else {
        Err(DatapackError::InvalidFormat(format!(
            "--backend '{name}' is unsupported; expected chunked-raw-zstd or zstd-mt-experimental"
        )))
    }
}

fn validate_output_overwrite_policy(path: &Path, force: bool) -> Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) => {
            if !metadata.file_type().is_file() {
                return Err(DatapackError::InvalidFormat(format!(
                    "output path '{}' exists but is not a regular file",
                    path.display()
                )));
            }
            if !force {
                return Err(DatapackError::InvalidFormat(format!(
                    "output file '{}' already exists; pass --force to replace it",
                    path.display()
                )));
            }
            Ok(())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(DatapackError::InvalidFormat(format!(
            "could not inspect output path '{}': {error}",
            path.display()
        ))),
    }
}

fn validate_nonzero_megabyte_limit(flag: &str, mb: u64) -> Result<()> {
    if mb == 0 {
        return Err(DatapackError::InvalidFormat(format!(
            "{flag} must be greater than zero"
        )));
    }
    let _ = megabytes_to_bytes(flag, mb)?;
    Ok(())
}

fn optional_megabytes_to_bytes(flag: &str, value: Option<u64>) -> Result<Option<u64>> {
    value.map(|mb| megabytes_to_bytes(flag, mb)).transpose()
}

fn megabytes_to_bytes(flag: &str, mb: u64) -> Result<u64> {
    mb.checked_mul(1024 * 1024)
        .ok_or_else(|| DatapackError::InvalidFormat(format!("{flag} is too large")))
}

fn validate_compression_memory_limit(
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

fn validate_v1_decompression_limits(
    metadata: &DpackMetadata,
    archive_size: u64,
    options: DecompressOptions,
) -> Result<()> {
    if let Some(maximum) = optional_megabytes_to_bytes("--max-output-mb", options.max_output_mb)? {
        if metadata.original_size > maximum {
            return Err(DatapackError::InvalidFormat(format!(
                "archive declares {} output bytes, exceeding --max-output-mb limit of {maximum} bytes",
                metadata.original_size
            )));
        }
    }

    let Some(maximum) = optional_megabytes_to_bytes("--max-memory-mb", options.max_memory_mb)?
    else {
        return Ok(());
    };
    let estimate = if matches!(metadata.payload_kind, PayloadKind::CsvColumnarDictionary) {
        metadata
            .original_size
            .checked_mul(18)
            .and_then(|value| value.checked_add(archive_size))
            .ok_or_else(|| {
                DatapackError::InvalidFormat(
                    "v1 decompression memory estimate overflowed u64".to_string(),
                )
            })?
    } else {
        let io_bytes = u64::try_from(IO_BUFFER_BYTES)
            .ok()
            .and_then(|value| value.checked_mul(2))
            .ok_or_else(|| {
                DatapackError::InvalidFormat(
                    "v1 decompression memory estimate overflowed u64".to_string(),
                )
            })?;
        io_bytes.checked_add(16 * 1024 * 1024).ok_or_else(|| {
            DatapackError::InvalidFormat(
                "v1 decompression memory estimate overflowed u64".to_string(),
            )
        })?
    };
    if estimate > maximum {
        return Err(DatapackError::InvalidFormat(format!(
            "estimated decompression memory {estimate} bytes exceeds --max-memory-mb limit of {maximum} bytes"
        )));
    }
    Ok(())
}

fn operation_failed(
    operation: &'static str,
    input: &Path,
    output: &Path,
    output_existed: bool,
    error: DatapackError,
) -> DatapackError {
    DatapackError::OperationFailed {
        operation,
        input: input.to_path_buf(),
        output: output.to_path_buf(),
        reason: error.to_string(),
        output_status: if output_existed {
            "previous output preserved"
        } else {
            "no final output was committed"
        },
    }
}

fn normalized_cli_path(path: &Path) -> Result<PathBuf> {
    if path.exists() {
        return Ok(std::fs::canonicalize(path)?);
    }
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    let file_name = absolute.file_name().ok_or_else(|| {
        DatapackError::InvalidFormat(format!("invalid output path: {}", path.display()))
    })?;
    let parent = absolute.parent().unwrap_or_else(|| Path::new("."));
    let parent = if parent.exists() {
        std::fs::canonicalize(parent)?
    } else {
        parent.to_path_buf()
    };
    Ok(parent.join(file_name))
}

fn benchmark_temp_path(input: &std::path::Path) -> PathBuf {
    let mut path = benchmark_temp_root(input);
    let stem = input
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("datapack-benchmark");
    path.push(format!("{stem}-{}-benchmark.dpack", std::process::id()));
    path
}

fn benchmark_restore_path(input: &std::path::Path) -> PathBuf {
    let mut path = benchmark_temp_root(input);
    let stem = input
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("datapack-benchmark");
    path.push(format!(
        "{stem}-{}-benchmark-restored.tmp",
        std::process::id()
    ));
    path
}

fn benchmark_zstd_temp_path(input: &std::path::Path) -> PathBuf {
    let mut path = benchmark_temp_root(input);
    let stem = input
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("datapack-benchmark");
    path.push(format!(
        "{stem}-{}-benchmark-baseline.zst",
        std::process::id()
    ));
    path
}

fn benchmark_chunked_temp_path(input: &std::path::Path) -> PathBuf {
    let mut path = benchmark_temp_root(input);
    let stem = input
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("datapack-benchmark");
    path.push(format!(
        "{stem}-{}-benchmark-chunked.dpack",
        std::process::id()
    ));
    path
}

fn benchmark_chunked_restore_path(input: &std::path::Path) -> PathBuf {
    let mut path = benchmark_temp_root(input);
    let stem = input
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("datapack-benchmark");
    path.push(format!(
        "{stem}-{}-benchmark-chunked-restored.tmp",
        std::process::id()
    ));
    path
}

fn benchmark_chunked_sample_path(input: &std::path::Path) -> PathBuf {
    let mut path = benchmark_temp_root(input);
    let stem = input
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("datapack-benchmark");
    path.push(format!(
        "{stem}-{}-benchmark-prefix.tmp",
        std::process::id()
    ));
    path
}

fn benchmark_temp_root(input: &std::path::Path) -> PathBuf {
    std::env::var_os("DATAPACK_TEMP_DIR")
        .map(PathBuf::from)
        .or_else(|| input.parent().map(std::path::Path::to_path_buf))
        .unwrap_or_else(std::env::temp_dir)
}

struct BenchmarkTempFiles {
    paths: Vec<PathBuf>,
    keep: bool,
}

impl BenchmarkTempFiles {
    fn new(keep: bool, paths: Vec<PathBuf>) -> Self {
        Self { paths, keep }
    }
}

impl Drop for BenchmarkTempFiles {
    fn drop(&mut self) {
        if self.keep {
            return;
        }
        for path in &self.paths {
            let _ = std::fs::remove_file(path);
        }
    }
}

fn print_benchmark_table(metrics: &BenchmarkMetrics) {
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

fn print_benchmark_json(metrics: &BenchmarkMetrics) {
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

#[derive(Debug, Default)]
struct ProfileTimings {
    planning_ms: Option<u64>,
    read_input_ms: Option<u64>,
    zstd_only_ms: Option<u64>,
    datapack_compress_ms: Option<u64>,
    datapack_decompress_ms: Option<u64>,
    hash_ms: Option<u64>,
    archive_write_ms: Option<u64>,
    archive_read_ms: Option<u64>,
    total_elapsed_ms: Option<u64>,
    compression_mb_per_sec: Option<f64>,
    decompression_mb_per_sec: Option<f64>,
}

fn print_profile_timings(timings: &ProfileTimings) {
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

fn display_optional_u64(value: Option<u64>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "unavailable".to_string())
}

fn display_optional_f64(value: Option<f64>) -> String {
    value
        .map(|value| format!("{value:.3}"))
        .unwrap_or_else(|| "unavailable".to_string())
}

fn json_escape(value: &str) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn benchmark_options() -> BenchmarkOptions {
        BenchmarkOptions {
            json: false,
            keep_temp: false,
            quick: true,
            runs: 1,
            profile: false,
            chunked: false,
            chunk_size_mb: None,
            threads: None,
            max_in_flight_chunks: None,
            backend: None,
            adaptive_level: false,
            no_zstd_baseline: false,
            no_roundtrip: false,
            no_hash: false,
            estimate_only: false,
            max_input_mb: None,
        }
    }

    #[test]
    fn benchmark_scope_uses_canonical_values() {
        let options = benchmark_options();
        assert_eq!(benchmark_scope(&options, false), "full");
        assert_eq!(benchmark_scope(&options, true), "sampled");

        let mut partial = options;
        partial.no_zstd_baseline = true;
        assert_eq!(benchmark_scope(&partial, false), "partial");
        assert_eq!(benchmark_scope(&partial, true), "sampled");
    }

    #[test]
    fn benchmark_partial_reasons_are_unique() {
        let mut options = benchmark_options();
        options.no_zstd_baseline = true;
        options.no_roundtrip = true;
        options.no_hash = true;
        let reasons = benchmark_partial_reasons(&options, true);
        let mut unique = reasons.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(reasons.len(), unique.len());
    }

    #[test]
    fn benchmark_validation_status_is_canonical_and_mismatch_is_error() {
        assert_eq!(
            validation_status(true, false, Some("same"), Some("same")).unwrap(),
            "validated"
        );
        assert_eq!(
            validation_status(true, true, Some("same"), Some("same")).unwrap(),
            "partially_validated"
        );
        assert_eq!(
            validation_status(false, false, None, None).unwrap(),
            "not_validated"
        );
        let error = validation_status(true, false, Some("restored"), Some("original"))
            .expect_err("a SHA256 mismatch must fail the benchmark");
        assert!(error.to_string().contains("SHA256 validation failed"));
    }

    #[test]
    fn json_escape_covers_control_characters() {
        assert_eq!(
            json_escape("quote=\" slash=\\ line=\n tab=\t \u{0001}"),
            "quote=\\\" slash=\\\\ line=\\n tab=\\t \\u0001"
        );
    }

    #[test]
    fn benchmark_temp_guard_removes_files_on_drop() {
        let directory = tempfile::tempdir().unwrap();
        let first = directory.path().join("first.tmp");
        let second = directory.path().join("second.tmp");
        std::fs::write(&first, b"first").unwrap();
        std::fs::write(&second, b"second").unwrap();

        {
            let _guard = BenchmarkTempFiles::new(false, vec![first.clone(), second.clone()]);
        }

        assert!(!first.exists());
        assert!(!second.exists());
    }

    #[test]
    fn benchmark_temp_guard_honors_keep_temp() {
        let directory = tempfile::tempdir().unwrap();
        let artifact = directory.path().join("kept.tmp");
        std::fs::write(&artifact, b"kept").unwrap();

        {
            let _guard = BenchmarkTempFiles::new(true, vec![artifact.clone()]);
        }

        assert!(artifact.exists());
    }
}
