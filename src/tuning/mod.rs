//! Experimental, hardware-local tuning for the v2 chunked RawZstd backend.
//!
//! The tuning engine deliberately recommends configurations without changing
//! DataPack's defaults. Each archive is produced through the normal v2 storage
//! API, and round trips use the normal verified decompressor unless the caller
//! explicitly requests a partial, non-validating run.

use std::collections::BTreeMap;
use std::fmt;
use std::fs::{File, OpenOptions};
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};

use crate::error::{DatapackError, Result};
use crate::storage::chunked::{
    self, ChunkedBackend, ChunkedCompressOptions, ChunkedDecompressOptions, CHUNKED_VERSION,
};

const BYTES_PER_MB: u64 = 1024 * 1024;
const REPORT_HEADERS: [&str; 32] = [
    "timestamp",
    "datapack_version",
    "input_path",
    "input_size_bytes",
    "measured_input_size_bytes",
    "input_sampled",
    "max_input_mb",
    "backend",
    "archive_version",
    "mode",
    "chunk_size_mb",
    "chunk_size_bytes",
    "threads",
    "max_in_flight_chunks",
    "adaptive_level",
    "zstd_level_strategy",
    "runs",
    "run_index",
    "output_size_bytes",
    "compression_ratio",
    "compression_time_ms",
    "compression_mb_per_sec",
    "decompression_time_ms",
    "decompression_mb_per_sec",
    "total_time_ms",
    "sha256_match",
    "validation_status",
    "benchmark_scope",
    "peak_memory_estimate_mb",
    "temp_path_used",
    "notes",
    "error",
];
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Backends understood by the tuning engine.
///
/// The native-zstd option remains experimental; both backends emit the same
/// compatible v2 container and are reported distinctly for comparison.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TuneBackend {
    #[default]
    ChunkedRawZstd,
    ZstdMtExperimental,
}

impl TuneBackend {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ChunkedRawZstd => "chunked-raw-zstd",
            Self::ZstdMtExperimental => "zstd-mt-experimental",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        value.parse()
    }
}

impl fmt::Display for TuneBackend {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for TuneBackend {
    type Err = DatapackError;

    fn from_str(value: &str) -> Result<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "chunked-raw-zstd" => Ok(Self::ChunkedRawZstd),
            "zstd-mt-experimental" => Ok(Self::ZstdMtExperimental),
            other => Err(DatapackError::InvalidFormat(format!(
                "unsupported tune backend '{other}'; supported backends: chunked-raw-zstd, zstd-mt-experimental"
            ))),
        }
    }
}

/// Options for one tuning grid.
#[derive(Debug, Clone)]
pub struct TuneOptions {
    /// Explicit CSV report destination. When omitted, a collision-safe name is
    /// selected beside the input file.
    pub output: Option<PathBuf>,
    pub chunk_sizes_mb: Vec<u64>,
    pub threads: Vec<usize>,
    pub runs: usize,
    pub max_input_mb: Option<u64>,
    pub skip_roundtrip: bool,
    pub no_hash: bool,
    pub adaptive_level: bool,
    pub keep_temp: bool,
    pub profile: bool,
    pub max_in_flight_chunks: Option<usize>,
    pub force: bool,
    pub backend: TuneBackend,
}

impl Default for TuneOptions {
    fn default() -> Self {
        let max_threads = chunked::default_thread_count();
        let mut threads = vec![1, 2, 4, 8, max_threads];
        threads.sort_unstable();
        threads.dedup();
        Self {
            output: None,
            chunk_sizes_mb: vec![32, 64, 128, 256],
            threads,
            runs: 1,
            max_input_mb: None,
            skip_roundtrip: false,
            no_hash: false,
            adaptive_level: false,
            keep_temp: false,
            profile: false,
            max_in_flight_chunks: None,
            force: false,
            backend: TuneBackend::ChunkedRawZstd,
        }
    }
}

/// A median configuration selected from successful rows in the tuning report.
#[derive(Debug, Clone)]
pub struct TuneRecommendation {
    pub backend: TuneBackend,
    pub chunk_size_mb: u64,
    pub chunk_size_bytes: u64,
    pub threads: usize,
    pub max_in_flight_chunks: usize,
    pub compression_mb_per_sec: f64,
    pub compression_ratio: f64,
    pub output_size_bytes: u64,
    pub peak_memory_estimate_mb: f64,
    /// Populated for the balanced recommendation. Throughput and ratio winners
    /// use `None` because their selection is based directly on one metric.
    pub balanced_score: Option<f64>,
}

/// Result returned to a CLI or another caller after a tuning grid completes.
#[derive(Debug, Clone)]
pub struct TuneSummary {
    pub report_path: PathBuf,
    pub input_size_bytes: u64,
    pub measured_input_size_bytes: u64,
    pub input_sampled: bool,
    pub rows_written: usize,
    pub successful_runs: usize,
    pub failed_runs: usize,
    pub best_throughput: Option<TuneRecommendation>,
    pub best_ratio: Option<TuneRecommendation>,
    pub balanced: Option<TuneRecommendation>,
    /// Paths intentionally preserved by `--keep-temp`. Empty for normal runs.
    pub temp_paths: Vec<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct ConfigKey {
    backend: TuneBackend,
    chunk_size_mb: u64,
    chunk_size_bytes: u64,
    threads: usize,
    max_in_flight_chunks: usize,
}

#[derive(Debug, Clone, Copy)]
struct SuccessfulRun {
    compression_mb_per_sec: f64,
    compression_ratio: f64,
    output_size_bytes: u64,
    peak_memory_estimate_mb: f64,
}

#[derive(Debug, Clone)]
struct Aggregate {
    key: ConfigKey,
    compression_mb_per_sec: f64,
    compression_ratio: f64,
    output_size_bytes: u64,
    peak_memory_estimate_mb: f64,
}

#[derive(Debug)]
struct TuneRow {
    timestamp: u64,
    input_path: String,
    input_size_bytes: u64,
    measured_input_size_bytes: u64,
    input_sampled: bool,
    max_input_mb: Option<u64>,
    key: ConfigKey,
    adaptive_level: bool,
    runs: usize,
    run_index: usize,
    output_size_bytes: Option<u64>,
    compression_ratio: Option<f64>,
    compression_time_ms: Option<u128>,
    compression_mb_per_sec: Option<f64>,
    decompression_time_ms: Option<u128>,
    decompression_mb_per_sec: Option<f64>,
    total_time_ms: u128,
    sha256_match: Option<bool>,
    validation_status: &'static str,
    benchmark_scope: &'static str,
    peak_memory_estimate_mb: f64,
    temp_path_used: String,
    notes: String,
    error: Option<String>,
}

impl TuneRow {
    fn csv_values(&self) -> Vec<String> {
        vec![
            self.timestamp.to_string(),
            env!("CARGO_PKG_VERSION").to_string(),
            self.input_path.clone(),
            self.input_size_bytes.to_string(),
            self.measured_input_size_bytes.to_string(),
            self.input_sampled.to_string(),
            option_u64(self.max_input_mb),
            self.key.backend.as_str().to_string(),
            CHUNKED_VERSION.to_string(),
            "RawZstd".to_string(),
            self.key.chunk_size_mb.to_string(),
            self.key.chunk_size_bytes.to_string(),
            self.key.threads.to_string(),
            self.key.max_in_flight_chunks.to_string(),
            self.adaptive_level.to_string(),
            if self.adaptive_level {
                "adaptive".to_string()
            } else {
                "fixed-level-3".to_string()
            },
            self.runs.to_string(),
            self.run_index.to_string(),
            option_u64(self.output_size_bytes),
            option_f64(self.compression_ratio),
            option_u128(self.compression_time_ms),
            option_f64(self.compression_mb_per_sec),
            option_u128(self.decompression_time_ms),
            option_f64(self.decompression_mb_per_sec),
            self.total_time_ms.to_string(),
            self.sha256_match
                .map(|value| value.to_string())
                .unwrap_or_default(),
            self.validation_status.to_string(),
            self.benchmark_scope.to_string(),
            format!("{:.2}", self.peak_memory_estimate_mb),
            self.temp_path_used.clone(),
            self.notes.clone(),
            self.error.clone().unwrap_or_default(),
        ]
    }
}

/// Executes an experimental v2 RawZstd tuning grid and writes a CSV report.
///
/// Invalid grid options fail before any benchmark is run. Failures belonging to
/// an individual configuration are instead recorded in that row, and the rest
/// of the grid continues.
pub fn tune(input: &Path, mut options: TuneOptions) -> Result<TuneSummary> {
    let input_metadata = validate_request(input, &options)?;
    normalize_grid(&mut options);

    let report_path = resolve_report_path(input, options.output.as_deref(), options.force)?;
    let temp_parent = report_path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut artifacts = TempArtifacts::new(options.keep_temp);
    let input_size_bytes = input_metadata.len();
    let sample_limit = options
        .max_input_mb
        .map(mb_to_bytes)
        .transpose()?
        .unwrap_or(input_size_bytes);
    let input_sampled = sample_limit < input_size_bytes;
    let measured_input_size_bytes = input_size_bytes.min(sample_limit);
    let measured_path = if input_sampled {
        let suffix = input
            .extension()
            .and_then(|extension| extension.to_str())
            .map(|extension| format!(".sample.{extension}"))
            .unwrap_or_else(|| ".sample".to_string());
        let path = artifacts.reserve(temp_parent, "input", &suffix)?;
        copy_prefix(input, &path, measured_input_size_bytes)?;
        path
    } else {
        input.to_path_buf()
    };

    let report_file = open_report(&report_path, options.force)?;
    let mut report = BufWriter::new(report_file);
    write_csv_record(&mut report, REPORT_HEADERS.iter().copied())?;
    report.flush()?;

    // Hash the measured source once. Each successfully restored output is
    // independently hashed and compared with this digest.
    let source_hash = if !options.skip_roundtrip && !options.no_hash {
        Some(hash_file(&measured_path)?)
    } else {
        None
    };
    let benchmark_scope = benchmark_scope(input_sampled, &options);
    let base_notes = base_notes(input_sampled, &options);
    let mut successful_by_config: BTreeMap<ConfigKey, Vec<SuccessfulRun>> = BTreeMap::new();
    let mut rows_written = 0usize;
    let mut successful_runs = 0usize;
    let mut failed_runs = 0usize;

    for &chunk_size_mb in &options.chunk_sizes_mb {
        let chunk_size_bytes_usize = chunked::chunk_size_mb_to_bytes(chunk_size_mb)?;
        let chunk_size_bytes = chunk_size_bytes_usize as u64;
        for &threads in &options.threads {
            let max_in_flight_chunks =
                options
                    .max_in_flight_chunks
                    .unwrap_or_else(|| match options.backend {
                        TuneBackend::ChunkedRawZstd => {
                            chunked::default_max_in_flight_chunks(threads)
                        }
                        TuneBackend::ZstdMtExperimental => {
                            // Native zstd owns the requested workers internally;
                            // the outer chunk pipeline intentionally has one worker.
                            chunked::default_max_in_flight_chunks(1)
                        }
                    });
            let key = ConfigKey {
                backend: options.backend,
                chunk_size_mb,
                chunk_size_bytes,
                threads,
                max_in_flight_chunks,
            };
            let peak_memory_estimate_mb =
                peak_memory_estimate_mb(chunk_size_mb, threads, max_in_flight_chunks);

            for run_index in 1..=options.runs {
                let archive_path = artifacts.reserve(temp_parent, "archive", ".dpack")?;
                let restored_path = if options.skip_roundtrip {
                    None
                } else {
                    Some(artifacts.reserve(temp_parent, "restored", ".bin")?)
                };
                let temp_path_used = describe_temp_paths(
                    input_sampled.then_some(measured_path.as_path()),
                    &archive_path,
                    restored_path.as_deref(),
                );
                let started = Instant::now();
                let mut row = TuneRow {
                    timestamp: unix_seconds(),
                    input_path: input.to_string_lossy().into_owned(),
                    input_size_bytes,
                    measured_input_size_bytes,
                    input_sampled,
                    max_input_mb: options.max_input_mb,
                    key,
                    adaptive_level: options.adaptive_level,
                    runs: options.runs,
                    run_index,
                    output_size_bytes: None,
                    compression_ratio: None,
                    compression_time_ms: None,
                    compression_mb_per_sec: None,
                    decompression_time_ms: None,
                    decompression_mb_per_sec: None,
                    total_time_ms: 0,
                    sha256_match: None,
                    validation_status: if options.skip_roundtrip || options.no_hash {
                        "not_validated"
                    } else if input_sampled {
                        "partially_validated"
                    } else {
                        "validated"
                    },
                    benchmark_scope,
                    peak_memory_estimate_mb,
                    temp_path_used,
                    notes: base_notes.clone(),
                    error: None,
                };

                if let Err(error) = execute_run(
                    &measured_path,
                    &archive_path,
                    restored_path.as_deref(),
                    source_hash.as_ref(),
                    &options,
                    &mut row,
                ) {
                    row.validation_status = "not_validated";
                    if !row.notes.is_empty() {
                        row.notes.push(';');
                    }
                    row.notes.push_str("run_failed");
                    row.error = Some(error.to_string());
                }
                row.total_time_ms = started.elapsed().as_millis();

                if row.error.is_none() {
                    successful_runs += 1;
                    successful_by_config
                        .entry(key)
                        .or_default()
                        .push(SuccessfulRun {
                            compression_mb_per_sec: row.compression_mb_per_sec.unwrap_or(0.0),
                            compression_ratio: row.compression_ratio.unwrap_or(0.0),
                            output_size_bytes: row.output_size_bytes.unwrap_or(0),
                            peak_memory_estimate_mb,
                        });
                } else {
                    failed_runs += 1;
                }

                write_csv_record(&mut report, row.csv_values())?;
                report.flush()?;
                rows_written += 1;

                // A full grid may produce many input-sized restored files. Do
                // not retain one run's outputs while later configurations run
                // unless the caller explicitly requested `--keep-temp`.
                artifacts.cleanup(&archive_path);
                if let Some(restored_path) = restored_path.as_deref() {
                    artifacts.cleanup(restored_path);
                }
            }
        }
    }
    report.flush()?;

    let aggregates = aggregate_runs(successful_by_config);
    let (best_throughput, best_ratio, balanced) = select_recommendations(&aggregates);
    let temp_paths = if options.keep_temp {
        artifacts.paths.clone()
    } else {
        Vec::new()
    };

    Ok(TuneSummary {
        report_path,
        input_size_bytes,
        measured_input_size_bytes,
        input_sampled,
        rows_written,
        successful_runs,
        failed_runs,
        best_throughput,
        best_ratio,
        balanced,
        temp_paths,
    })
}

fn execute_run(
    measured_path: &Path,
    archive_path: &Path,
    restored_path: Option<&Path>,
    source_hash: Option<&[u8; 32]>,
    options: &TuneOptions,
    row: &mut TuneRow,
) -> Result<()> {
    let mut compress_options = ChunkedCompressOptions::new_with_max_in_flight(
        usize::try_from(row.key.chunk_size_bytes).map_err(|_| {
            DatapackError::InvalidFormat("tune chunk size exceeds platform capacity".to_string())
        })?,
        row.key.threads,
        row.key.max_in_flight_chunks,
        options.adaptive_level,
        options.profile,
    )?
    .with_backend(match options.backend {
        TuneBackend::ChunkedRawZstd => ChunkedBackend::ChunkedRawZstd,
        TuneBackend::ZstdMtExperimental => ChunkedBackend::ZstdMtExperimental,
    })?;
    compress_options.force = true;
    compress_options.keep_temp = options.keep_temp;
    let compression_started = Instant::now();
    let compression_stats =
        chunked::encode_raw_zstd_chunked_file(measured_path, archive_path, compress_options)?;
    let compression_elapsed = compression_started.elapsed();
    if compression_stats.original_size_bytes != row.measured_input_size_bytes {
        return Err(DatapackError::InvalidFormat(format!(
            "measured input size changed from {} to {} bytes during tuning",
            row.measured_input_size_bytes, compression_stats.original_size_bytes
        )));
    }
    row.compression_time_ms = Some(compression_elapsed.as_millis());
    row.output_size_bytes = Some(compression_stats.archive_size_bytes);
    row.compression_ratio = Some(ratio(
        compression_stats.original_size_bytes,
        compression_stats.archive_size_bytes,
    ));
    row.compression_mb_per_sec = Some(mb_per_second(
        compression_stats.original_size_bytes,
        compression_elapsed.as_secs_f64(),
    ));

    if let Some(restored_path) = restored_path {
        let decompression_started = Instant::now();
        let decompression_stats = chunked::decode_raw_zstd_chunked_file(
            archive_path,
            restored_path,
            ChunkedDecompressOptions {
                verify: !options.no_hash,
                force: true,
                keep_temp: options.keep_temp,
                ..ChunkedDecompressOptions::default()
            },
        )?;
        let decompression_elapsed = decompression_started.elapsed();
        row.decompression_time_ms = Some(decompression_elapsed.as_millis());
        row.decompression_mb_per_sec = Some(mb_per_second(
            decompression_stats.original_size_bytes,
            decompression_elapsed.as_secs_f64(),
        ));

        if let Some(source_hash) = source_hash {
            let restored_hash = hash_file(restored_path)?;
            let matches = source_hash == &restored_hash;
            row.sha256_match = Some(matches);
            if !matches {
                return Err(DatapackError::InvalidFormat(
                    "tune roundtrip SHA256 mismatch".to_string(),
                ));
            }
        }
    }
    Ok(())
}

fn validate_request(input: &Path, options: &TuneOptions) -> Result<std::fs::Metadata> {
    let metadata = std::fs::metadata(input)
        .map_err(|error| DatapackError::AnalyzeRead(format!("{} ({error})", input.display())))?;
    if !metadata.is_file() {
        return Err(DatapackError::AnalyzeRead(format!(
            "{} is not a regular file",
            input.display()
        )));
    }
    File::open(input)
        .map_err(|error| DatapackError::AnalyzeRead(format!("{} ({error})", input.display())))?;
    if options.chunk_sizes_mb.is_empty() {
        return Err(invalid("--chunk-sizes-mb must contain at least one value"));
    }
    for &chunk_size_mb in &options.chunk_sizes_mb {
        chunked::chunk_size_mb_to_bytes(chunk_size_mb)?;
    }
    if options.threads.is_empty() {
        return Err(invalid("--threads-list must contain at least one value"));
    }
    if options.threads.contains(&0) {
        return Err(invalid("--threads-list values must be greater than zero"));
    }
    if options.runs == 0 {
        return Err(invalid("--runs must be greater than zero"));
    }
    if options.max_input_mb == Some(0) {
        return Err(invalid("--max-input-mb must be greater than zero"));
    }
    if let Some(max_input_mb) = options.max_input_mb {
        mb_to_bytes(max_input_mb)?;
    }
    if options.max_in_flight_chunks == Some(0) {
        return Err(invalid("--max-in-flight-chunks must be greater than zero"));
    }
    if let Some(output) = options.output.as_deref() {
        ensure_distinct(input, output, "tune report output must differ from input")?;
        if output.exists() && !options.force {
            return Err(DatapackError::OutputNotWritable(format!(
                "tune report already exists: {}; pass --force to overwrite it",
                output.display()
            )));
        }
    }
    Ok(metadata)
}

fn normalize_grid(options: &mut TuneOptions) {
    options.chunk_sizes_mb.sort_unstable();
    options.chunk_sizes_mb.dedup();
    options.threads.sort_unstable();
    options.threads.dedup();
}

fn resolve_report_path(input: &Path, output: Option<&Path>, force: bool) -> Result<PathBuf> {
    if let Some(output) = output {
        if output.exists() && !force {
            return Err(DatapackError::OutputNotWritable(format!(
                "tune report already exists: {}; pass --force to overwrite it",
                output.display()
            )));
        }
        validate_parent(output)?;
        return Ok(output.to_path_buf());
    }

    let parent = input
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let stem = input
        .file_stem()
        .and_then(|stem| stem.to_str())
        .filter(|stem| !stem.is_empty())
        .unwrap_or("datapack");
    let timestamp = unix_seconds();
    for suffix in 0u32..10_000 {
        let filename = if suffix == 0 {
            format!("{stem}-tune-{timestamp}.csv")
        } else {
            format!("{stem}-tune-{timestamp}-{suffix}.csv")
        };
        let candidate = parent.join(filename);
        if !candidate.exists() {
            return Ok(candidate);
        }
    }
    Err(DatapackError::OutputNotWritable(format!(
        "could not choose a collision-safe tune report beside {}",
        input.display()
    )))
}

fn validate_parent(path: &Path) -> Result<()> {
    let parent = path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    if !parent.is_dir() {
        return Err(DatapackError::OutputNotWritable(format!(
            "output directory does not exist: {}",
            parent.display()
        )));
    }
    Ok(())
}

fn open_report(path: &Path, force: bool) -> Result<File> {
    validate_parent(path)?;
    let mut options = OpenOptions::new();
    options.write(true).create(true);
    if force {
        options.truncate(true);
    } else {
        options.create_new(true);
    }
    options
        .open(path)
        .map_err(|error| DatapackError::OutputNotWritable(format!("{} ({error})", path.display())))
}

fn benchmark_scope(input_sampled: bool, options: &TuneOptions) -> &'static str {
    if input_sampled {
        "sampled"
    } else if options.skip_roundtrip || options.no_hash {
        "partial"
    } else {
        "full"
    }
}

fn base_notes(input_sampled: bool, options: &TuneOptions) -> String {
    let mut notes = vec!["peak_memory_estimate_is_approximate"];
    if input_sampled {
        notes.push("prefix_sample");
    }
    if options.skip_roundtrip {
        notes.push("roundtrip_skipped_non_validating");
    }
    if options.no_hash {
        notes.push("hash_verification_skipped_non_validating");
    }
    if options.keep_temp {
        notes.push("temporary_files_preserved");
    }
    notes.join(";")
}

fn copy_prefix(input: &Path, output: &Path, limit: u64) -> Result<()> {
    let mut reader = BufReader::new(File::open(input)?).take(limit);
    let mut writer = BufWriter::new(OpenOptions::new().write(true).truncate(true).open(output)?);
    let copied = std::io::copy(&mut reader, &mut writer)?;
    writer.flush()?;
    if copied != limit {
        return Err(DatapackError::InvalidFormat(format!(
            "input ended after {copied} bytes while creating a {limit}-byte tune sample"
        )));
    }
    Ok(())
}

fn hash_file(path: &Path) -> Result<[u8; 32]> {
    let mut reader = BufReader::new(File::open(path)?);
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1024 * 1024];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hasher.finalize().into())
}

fn mb_to_bytes(mb: u64) -> Result<u64> {
    mb.checked_mul(BYTES_PER_MB)
        .ok_or_else(|| invalid("MB value exceeds the supported byte range"))
}

fn ratio(input_size: u64, output_size: u64) -> f64 {
    if output_size == 0 {
        0.0
    } else {
        input_size as f64 / output_size as f64
    }
}

fn mb_per_second(bytes: u64, elapsed_seconds: f64) -> f64 {
    bytes as f64 / BYTES_PER_MB as f64 / elapsed_seconds.max(1e-9)
}

/// A conservative approximation: one buffer for every outstanding chunk plus
/// a transient compression-output buffer for every active worker.
fn peak_memory_estimate_mb(chunk_size_mb: u64, threads: usize, max_in_flight_chunks: usize) -> f64 {
    let active_workers = threads.min(max_in_flight_chunks);
    chunk_size_mb as f64 * (max_in_flight_chunks.saturating_add(active_workers)) as f64
}

fn aggregate_runs(runs: BTreeMap<ConfigKey, Vec<SuccessfulRun>>) -> Vec<Aggregate> {
    runs.into_iter()
        .filter_map(|(key, runs)| {
            if runs.is_empty() {
                return None;
            }
            Some(Aggregate {
                key,
                compression_mb_per_sec: median_f64(
                    runs.iter().map(|run| run.compression_mb_per_sec).collect(),
                ),
                compression_ratio: median_f64(
                    runs.iter().map(|run| run.compression_ratio).collect(),
                ),
                output_size_bytes: median_u64(
                    runs.iter().map(|run| run.output_size_bytes).collect(),
                ),
                peak_memory_estimate_mb: median_f64(
                    runs.iter().map(|run| run.peak_memory_estimate_mb).collect(),
                ),
            })
        })
        .collect()
}

fn select_recommendations(
    aggregates: &[Aggregate],
) -> (
    Option<TuneRecommendation>,
    Option<TuneRecommendation>,
    Option<TuneRecommendation>,
) {
    if aggregates.is_empty() {
        return (None, None, None);
    }
    let best_throughput = aggregates
        .iter()
        .max_by(|left, right| {
            left.compression_mb_per_sec
                .total_cmp(&right.compression_mb_per_sec)
                .then_with(|| right.key.chunk_size_mb.cmp(&left.key.chunk_size_mb))
                .then_with(|| right.key.threads.cmp(&left.key.threads))
        })
        .map(|aggregate| recommendation(aggregate, None));
    let best_ratio = aggregates
        .iter()
        .max_by(|left, right| {
            left.compression_ratio
                .total_cmp(&right.compression_ratio)
                .then_with(|| right.key.chunk_size_mb.cmp(&left.key.chunk_size_mb))
                .then_with(|| right.key.threads.cmp(&left.key.threads))
        })
        .map(|aggregate| recommendation(aggregate, None));

    let throughput_min = aggregates
        .iter()
        .map(|aggregate| aggregate.compression_mb_per_sec)
        .reduce(f64::min)
        .unwrap_or(0.0);
    let throughput_max = aggregates
        .iter()
        .map(|aggregate| aggregate.compression_mb_per_sec)
        .reduce(f64::max)
        .unwrap_or(0.0);
    let ratio_min = aggregates
        .iter()
        .map(|aggregate| aggregate.compression_ratio)
        .reduce(f64::min)
        .unwrap_or(0.0);
    let ratio_max = aggregates
        .iter()
        .map(|aggregate| aggregate.compression_ratio)
        .reduce(f64::max)
        .unwrap_or(0.0);
    let memory_min = aggregates
        .iter()
        .map(|aggregate| aggregate.peak_memory_estimate_mb)
        .reduce(f64::min)
        .unwrap_or(0.0);
    let memory_max = aggregates
        .iter()
        .map(|aggregate| aggregate.peak_memory_estimate_mb)
        .reduce(f64::max)
        .unwrap_or(0.0);

    let balanced = aggregates
        .iter()
        .map(|aggregate| {
            let throughput = normalize(
                aggregate.compression_mb_per_sec,
                throughput_min,
                throughput_max,
            );
            let ratio = normalize(aggregate.compression_ratio, ratio_min, ratio_max);
            let memory = normalize(aggregate.peak_memory_estimate_mb, memory_min, memory_max);
            // Speed is the primary purpose of this phase, while ratio remains
            // material and memory receives a deliberately modest penalty.
            let score = 0.55 * throughput + 0.45 * ratio - 0.08 * memory;
            (aggregate, score)
        })
        .max_by(|(left, left_score), (right, right_score)| {
            left_score
                .total_cmp(right_score)
                .then_with(|| {
                    left.compression_mb_per_sec
                        .total_cmp(&right.compression_mb_per_sec)
                })
                .then_with(|| right.key.chunk_size_mb.cmp(&left.key.chunk_size_mb))
        })
        .map(|(aggregate, score)| recommendation(aggregate, Some(score)));

    (best_throughput, best_ratio, balanced)
}

fn recommendation(aggregate: &Aggregate, score: Option<f64>) -> TuneRecommendation {
    TuneRecommendation {
        backend: aggregate.key.backend,
        chunk_size_mb: aggregate.key.chunk_size_mb,
        chunk_size_bytes: aggregate.key.chunk_size_bytes,
        threads: aggregate.key.threads,
        max_in_flight_chunks: aggregate.key.max_in_flight_chunks,
        compression_mb_per_sec: aggregate.compression_mb_per_sec,
        compression_ratio: aggregate.compression_ratio,
        output_size_bytes: aggregate.output_size_bytes,
        peak_memory_estimate_mb: aggregate.peak_memory_estimate_mb,
        balanced_score: score,
    }
}

fn normalize(value: f64, minimum: f64, maximum: f64) -> f64 {
    if (maximum - minimum).abs() <= f64::EPSILON {
        1.0
    } else {
        (value - minimum) / (maximum - minimum)
    }
}

fn median_f64(mut values: Vec<f64>) -> f64 {
    values.sort_by(f64::total_cmp);
    let middle = values.len() / 2;
    if values.len() % 2 == 0 {
        (values[middle - 1] + values[middle]) / 2.0
    } else {
        values[middle]
    }
}

fn median_u64(mut values: Vec<u64>) -> u64 {
    values.sort_unstable();
    let middle = values.len() / 2;
    if values.len() % 2 == 0 {
        let left = values[middle - 1] as u128;
        let right = values[middle] as u128;
        ((left + right) / 2) as u64
    } else {
        values[middle]
    }
}

fn write_csv_record<W, I, S>(writer: &mut W, values: I) -> Result<()>
where
    W: Write,
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut first = true;
    for value in values {
        if !first {
            writer.write_all(b",")?;
        }
        first = false;
        write_csv_field(writer, value.as_ref())?;
    }
    writer.write_all(b"\n")?;
    Ok(())
}

fn write_csv_field<W: Write>(writer: &mut W, value: &str) -> Result<()> {
    if value
        .as_bytes()
        .iter()
        .any(|byte| matches!(byte, b',' | b'"' | b'\r' | b'\n'))
    {
        writer.write_all(b"\"")?;
        for byte in value.as_bytes() {
            if *byte == b'"' {
                writer.write_all(b"\"\"")?;
            } else {
                writer.write_all(std::slice::from_ref(byte))?;
            }
        }
        writer.write_all(b"\"")?;
    } else {
        writer.write_all(value.as_bytes())?;
    }
    Ok(())
}

fn option_u64(value: Option<u64>) -> String {
    value.map(|value| value.to_string()).unwrap_or_default()
}

fn option_u128(value: Option<u128>) -> String {
    value.map(|value| value.to_string()).unwrap_or_default()
}

fn option_f64(value: Option<f64>) -> String {
    value.map(|value| format!("{value:.6}")).unwrap_or_default()
}

fn unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn unique_stamp() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}

fn describe_temp_paths(sample: Option<&Path>, archive: &Path, restored: Option<&Path>) -> String {
    let mut paths = Vec::new();
    if let Some(sample) = sample {
        paths.push(format!("sample={}", sample.display()));
    }
    paths.push(format!("archive={}", archive.display()));
    if let Some(restored) = restored {
        paths.push(format!("restored={}", restored.display()));
    }
    paths.join(";")
}

fn ensure_distinct(input: &Path, output: &Path, message: &str) -> Result<()> {
    let input = normalize_path(input)?;
    let output = normalize_path(output)?;
    let same = if cfg!(windows) {
        input
            .to_string_lossy()
            .eq_ignore_ascii_case(&output.to_string_lossy())
    } else {
        input == output
    };
    if same {
        return Err(invalid(message));
    }
    Ok(())
}

fn normalize_path(path: &Path) -> Result<PathBuf> {
    if path.exists() {
        return Ok(std::fs::canonicalize(path)?);
    }
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    let file_name = absolute
        .file_name()
        .ok_or_else(|| invalid(format!("invalid output path: {}", path.display())))?;
    let parent = absolute.parent().unwrap_or_else(|| Path::new("."));
    let parent = if parent.exists() {
        std::fs::canonicalize(parent)?
    } else {
        parent.to_path_buf()
    };
    Ok(parent.join(file_name))
}

fn invalid(message: impl Into<String>) -> DatapackError {
    DatapackError::InvalidFormat(message.into())
}

struct TempArtifacts {
    keep: bool,
    paths: Vec<PathBuf>,
}

impl TempArtifacts {
    fn new(keep: bool) -> Self {
        Self {
            keep,
            paths: Vec::new(),
        }
    }

    fn reserve(&mut self, parent: &Path, label: &str, suffix: &str) -> Result<PathBuf> {
        for _ in 0..1000 {
            let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let path = parent.join(format!(
                ".datapack-tune-{}-{}-{sequence}-{label}{suffix}",
                std::process::id(),
                unique_stamp()
            ));
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(file) => {
                    drop(file);
                    self.paths.push(path.clone());
                    return Ok(path);
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error.into()),
            }
        }
        Err(DatapackError::OutputNotWritable(format!(
            "could not reserve a unique tuning temporary file in {}",
            parent.display()
        )))
    }

    fn cleanup(&mut self, path: &Path) {
        if self.keep {
            return;
        }
        match std::fs::remove_file(path) {
            Ok(()) => self.paths.retain(|candidate| candidate != path),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                self.paths.retain(|candidate| candidate != path);
            }
            // Retain the path so Drop gets one more cleanup attempt.
            Err(_) => {}
        }
    }
}

impl Drop for TempArtifacts {
    fn drop(&mut self) {
        if self.keep {
            return;
        }
        for path in self.paths.iter().rev() {
            let _ = std::fs::remove_file(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_fields_follow_rfc4180_escaping_rules() {
        let mut output = Vec::new();
        write_csv_record(
            &mut output,
            ["plain", "has,comma", "has\"quote", "line\nbreak"],
        )
        .unwrap();
        assert_eq!(
            String::from_utf8(output).unwrap(),
            "plain,\"has,comma\",\"has\"\"quote\",\"line\nbreak\"\n"
        );
    }

    #[test]
    fn median_helpers_are_stable_for_even_and_odd_counts() {
        assert_eq!(median_f64(vec![9.0, 1.0, 5.0]), 5.0);
        assert_eq!(median_f64(vec![8.0, 2.0]), 5.0);
        assert_eq!(median_u64(vec![9, 1, 5]), 5);
        assert_eq!(median_u64(vec![8, 2]), 5);
    }

    #[test]
    fn backend_parser_rejects_unknown_values() {
        assert_eq!(
            TuneBackend::parse("chunked-raw-zstd").unwrap(),
            TuneBackend::ChunkedRawZstd
        );
        assert_eq!(
            TuneBackend::parse("zstd-mt-experimental").unwrap(),
            TuneBackend::ZstdMtExperimental
        );
        assert!(TuneBackend::parse("zstd-mt").is_err());
    }

    #[test]
    fn small_tuning_grid_writes_a_validated_report_and_cleans_outputs() {
        let directory = tempfile::tempdir().unwrap();
        let input = directory.path().join("tiny.csv");
        let report = directory.path().join("tiny-tune.csv");
        let bytes: Vec<u8> = b"id,category,value\r\n1,alpha,42\r\n"
            .iter()
            .copied()
            .cycle()
            .take(32 * 1024)
            .collect();
        std::fs::write(&input, &bytes).unwrap();

        let summary = tune(
            &input,
            TuneOptions {
                output: Some(report.clone()),
                chunk_sizes_mb: vec![1],
                threads: vec![1],
                runs: 1,
                max_in_flight_chunks: Some(2),
                ..TuneOptions::default()
            },
        )
        .unwrap();

        assert_eq!(summary.rows_written, 1);
        assert_eq!(summary.successful_runs, 1);
        assert_eq!(summary.failed_runs, 0);
        assert!(summary.best_throughput.is_some());
        assert!(summary.temp_paths.is_empty());
        let csv = std::fs::read_to_string(&report).unwrap();
        assert!(csv.starts_with("timestamp,datapack_version,input_path,"));
        assert!(csv.contains(",true,validated,full,"));

        let mut remaining: Vec<_> = std::fs::read_dir(directory.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        remaining.sort();
        assert_eq!(
            remaining.len(),
            2,
            "unexpected temporary files: {remaining:?}"
        );
    }

    #[test]
    fn native_mt_tuning_uses_one_outer_worker_default_and_can_keep_outputs() {
        let directory = tempfile::tempdir().unwrap();
        let input = directory.path().join("tiny.csv");
        let report = directory.path().join("tiny-mt-tune.csv");
        let bytes: Vec<u8> = b"id,value\n1,repeated-value\n"
            .iter()
            .copied()
            .cycle()
            .take(32 * 1024)
            .collect();
        std::fs::write(&input, bytes).unwrap();

        let summary = tune(
            &input,
            TuneOptions {
                output: Some(report),
                chunk_sizes_mb: vec![1],
                threads: vec![2],
                runs: 1,
                keep_temp: true,
                backend: TuneBackend::ZstdMtExperimental,
                ..TuneOptions::default()
            },
        )
        .unwrap();

        let recommendation = summary.best_throughput.unwrap();
        assert_eq!(recommendation.backend, TuneBackend::ZstdMtExperimental);
        assert_eq!(recommendation.threads, 2);
        assert_eq!(recommendation.max_in_flight_chunks, 4);
        assert_eq!(summary.temp_paths.len(), 2);
        assert!(summary.temp_paths.iter().all(|path| path.exists()));
    }

    #[test]
    fn prefix_sample_is_marked_partially_validated() {
        let directory = tempfile::tempdir().unwrap();
        let input = directory.path().join("sample-source.bin");
        let report = directory.path().join("sample-tune.csv");
        let bytes: Vec<u8> = b"structured-prefix-data\n"
            .iter()
            .copied()
            .cycle()
            .take(BYTES_PER_MB as usize + 4096)
            .collect();
        std::fs::write(&input, bytes).unwrap();

        let summary = tune(
            &input,
            TuneOptions {
                output: Some(report.clone()),
                chunk_sizes_mb: vec![1],
                threads: vec![1],
                runs: 1,
                max_input_mb: Some(1),
                ..TuneOptions::default()
            },
        )
        .unwrap();

        assert!(summary.input_sampled);
        assert_eq!(summary.measured_input_size_bytes, BYTES_PER_MB);
        let csv = std::fs::read_to_string(report).unwrap();
        assert!(csv.contains(",true,partially_validated,sampled,"));
    }

    #[test]
    fn skipped_roundtrip_is_explicitly_non_validating() {
        let directory = tempfile::tempdir().unwrap();
        let input = directory.path().join("partial.bin");
        let report = directory.path().join("partial-tune.csv");
        std::fs::write(&input, b"small repeated repeated input").unwrap();

        let summary = tune(
            &input,
            TuneOptions {
                output: Some(report.clone()),
                chunk_sizes_mb: vec![1],
                threads: vec![1],
                runs: 1,
                skip_roundtrip: true,
                ..TuneOptions::default()
            },
        )
        .unwrap();

        assert_eq!(summary.successful_runs, 1);
        let csv = std::fs::read_to_string(report).unwrap();
        assert!(csv.contains(",not_validated,partial,"));
        assert!(csv.contains("roundtrip_skipped_non_validating"));
    }
}
