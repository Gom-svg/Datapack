use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};

use crate::application;
use crate::error::Result;
use crate::generation::{self, Profile};
use crate::storage;
use crate::tuning;

mod advisor;
mod analysis_json;
mod analysis_report;
mod benchmark;
mod chunked_options;
mod compare;
mod compress;
mod decompress;
mod profile;
mod progress;
mod tune;
mod validate;
mod validation;

const DEFAULT_SAMPLE_MB: u64 = 64;
const DEFAULT_MAX_DICTIONARY_VALUES: u64 = 65_535;
const DEFAULT_MAX_DICTIONARY_MB: u64 = 64;
const DEFAULT_VALIDATE_MAX_MEMORY_MB: u64 = 512;

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
        /// Emit a versioned machine-readable JSON report instead of legacy text.
        #[arg(long)]
        json: bool,
        /// Pretty-print JSON output. Requires --json.
        #[arg(long, requires = "json")]
        pretty: bool,
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
        #[arg(long, default_value_t = DEFAULT_MAX_DICTIONARY_VALUES)]
        max_dictionary_values: u64,
        /// Maximum estimated in-memory dictionary size per column.
        #[arg(long, default_value_t = DEFAULT_MAX_DICTIONARY_MB)]
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
    /// Validate an archive without creating restored output.
    Validate {
        archive: PathBuf,
        /// Compare the verified restored identity with an original source file.
        #[arg(long)]
        against: Option<PathBuf>,
        /// Emit a versioned machine-readable JSON report.
        #[arg(long)]
        json: bool,
        /// Pretty-print JSON output. Requires --json.
        #[arg(long, requires = "json")]
        pretty: bool,
        /// Refuse archives declaring an output larger than N MiB.
        #[arg(long)]
        max_output_mb: Option<u64>,
        /// Refuse v2 archives declaring more than N chunks.
        #[arg(long)]
        max_chunks: Option<u64>,
        /// Bound validation working memory in MiB.
        #[arg(long, default_value_t = DEFAULT_VALIDATE_MAX_MEMORY_MB)]
        max_memory_mb: u64,
    },
    /// Compare DataPack with standalone zstd using factual measurements.
    Compare {
        input: PathBuf,
        /// Comparison scope: bounded Quick mode or complete Full mode.
        #[arg(long, value_enum, default_value_t = CompareModeArg::Quick)]
        mode: CompareModeArg,
        /// Number of timing runs, from 1 to 25.
        #[arg(long, default_value_t = 3)]
        runs: usize,
        /// Compare at most the first N MiB in Quick mode.
        #[arg(long)]
        max_input_mb: Option<u64>,
        /// Emit a versioned machine-readable JSON report.
        #[arg(long)]
        json: bool,
        /// Pretty-print JSON output. Requires --json.
        #[arg(long, requires = "json")]
        pretty: bool,
    },
    /// Provide deterministic advice from existing analysis facts and policy.
    Advisor {
        input: PathBuf,
        /// Maximum sample size in MB, from 1 to 2048.
        #[arg(long, default_value_t = DEFAULT_SAMPLE_MB)]
        sample_mb: u64,
        /// Emit a versioned machine-readable JSON report.
        #[arg(long)]
        json: bool,
        /// Pretty-print JSON output. Requires --json.
        #[arg(long, requires = "json")]
        pretty: bool,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum CompareModeArg {
    Quick,
    Full,
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
            json,
            pretty,
        } => analyze_command(input, plan, sample_mb, json, pretty),
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
        } => compress::run(
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
        } => decompress::run(
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
        Command::Validate {
            archive,
            against,
            json,
            pretty,
            max_output_mb,
            max_chunks,
            max_memory_mb,
        } => validate::run(
            archive,
            ValidateOptions {
                against,
                json,
                pretty,
                max_output_mb,
                max_chunks,
                max_memory_mb,
            },
        ),
        Command::Compare {
            input,
            mode,
            runs,
            max_input_mb,
            json,
            pretty,
        } => compare::run(
            application::CompareRequest {
                input,
                mode: match mode {
                    CompareModeArg::Quick => application::CompareMode::Quick,
                    CompareModeArg::Full => application::CompareMode::Full,
                },
                runs,
                max_input_mb,
            },
            json,
            pretty,
        ),
        Command::Advisor {
            input,
            sample_mb,
            json,
            pretty,
        } => advisor::run(input, sample_mb, json, pretty),
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
        } => tune::run(
            input,
            tuning::TuneOptions {
                output,
                chunk_sizes_mb: tune::parse_tune_chunk_sizes(&chunk_sizes_mb)?,
                threads: tune::parse_tune_threads(&threads_list)?,
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
        } => benchmark::run(
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

#[derive(Debug, Clone)]
struct ValidateOptions {
    against: Option<PathBuf>,
    json: bool,
    pretty: bool,
    max_output_mb: Option<u64>,
    max_chunks: Option<u64>,
    max_memory_mb: u64,
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

fn analyze_command(
    input: PathBuf,
    include_plan: bool,
    sample_mb: u64,
    json: bool,
    pretty: bool,
) -> Result<()> {
    let mut observer = progress::TerminalProgressObserver::new();
    let analysis = application::analyze_for_cli_with_progress(
        application::AnalyzeRequest { input, sample_mb },
        &mut observer,
    )?;
    if json {
        analysis_json::print_analysis_v1(&analysis, pretty)
    } else {
        analysis_report::print_planning_analysis(&analysis, include_plan);
        Ok(())
    }
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
