use std::error::Error;
use std::fmt;
use std::fs::{self, File};
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use datapack::application::{
    self, AgainstStatusV1, ArchiveModeV1, CheckStatusV1, CodecBackendV1, CompressRequest,
    CompressionBackend, CompressionFormat, DecompressRequest, V1CompressionOptions,
    V2CompressionOptions, ValidateRequest,
};
use datapack::generation::{self, Profile};
use serde::Serialize;
use sha2::{Digest, Sha256};

const REPORT_SCHEMA_VERSION: u32 = 1;
const MIB: f64 = 1_048_576.0;
const HASH_BUFFER_BYTES: usize = 64 * 1024;

pub type HarnessResult<T> = Result<T, Box<dyn Error>>;

#[derive(Debug)]
struct HarnessError(String);

impl fmt::Display for HarnessError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Error for HarnessError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Preset {
    Smoke,
    Representative,
}

impl Preset {
    pub fn parse(value: &str) -> HarnessResult<Self> {
        match value {
            "smoke" => Ok(Self::Smoke),
            "representative" => Ok(Self::Representative),
            _ => Err(fail(format!(
                "unknown preset {value:?}; expected smoke or representative"
            ))),
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Smoke => "smoke",
            Self::Representative => "representative",
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum ArchiveConfiguration {
    V1,
    V2 {
        chunk_size_bytes: usize,
        threads: usize,
        max_in_flight_chunks: usize,
    },
}

#[derive(Debug, Clone, Copy)]
pub struct ScenarioSpec {
    pub name: &'static str,
    profile: Profile,
    pub rows: u64,
    pub seed: u64,
    archive: ArchiveConfiguration,
    expected_mode: &'static str,
    expected_source_bytes: Option<u64>,
    expected_source_sha256: Option<&'static str>,
}

impl ScenarioSpec {
    fn format(self) -> CompressionFormat {
        match self.archive {
            ArchiveConfiguration::V1 => CompressionFormat::V1(V1CompressionOptions::default()),
            ArchiveConfiguration::V2 {
                chunk_size_bytes,
                threads,
                max_in_flight_chunks,
            } => {
                let mut options = V2CompressionOptions::default();
                options.chunk_size_bytes = chunk_size_bytes;
                options.threads = threads;
                options.max_in_flight_chunks = max_in_flight_chunks;
                options.backend = CompressionBackend::ChunkedRawZstd;
                options.adaptive_level = false;
                options.max_memory_bytes = Some(
                    u64::try_from(chunk_size_bytes)
                        .unwrap_or(u64::MAX)
                        .saturating_mul(u64::try_from(max_in_flight_chunks).unwrap_or(u64::MAX)),
                );
                CompressionFormat::V2(options)
            }
        }
    }

    const fn expected_version(self) -> u16 {
        match self.archive {
            ArchiveConfiguration::V1 => 1,
            ArchiveConfiguration::V2 { .. } => 2,
        }
    }

    const fn chunk_configuration(self) -> (Option<usize>, Option<usize>, Option<usize>) {
        match self.archive {
            ArchiveConfiguration::V1 => (None, None, None),
            ArchiveConfiguration::V2 {
                chunk_size_bytes,
                threads,
                max_in_flight_chunks,
            } => (
                Some(chunk_size_bytes),
                Some(threads),
                Some(max_in_flight_chunks),
            ),
        }
    }
}

pub fn scenarios(preset: Preset) -> Vec<ScenarioSpec> {
    match preset {
        Preset::Smoke => vec![
            ScenarioSpec {
                name: "repetitive_structured_v1",
                profile: Profile::Repetitive,
                rows: 2_000,
                seed: 42,
                archive: ArchiveConfiguration::V1,
                expected_mode: "csv_columnar_dictionary",
                expected_source_bytes: Some(108_898),
                expected_source_sha256: Some(
                    "f924ecbbc173b70bc4705e31dd82ec74d7a5f962979c960c6f1c2f95dc32719f",
                ),
            },
            ScenarioSpec {
                name: "realistic_structured_v1",
                profile: Profile::Realistic,
                rows: 1_500,
                seed: 2_026,
                archive: ArchiveConfiguration::V1,
                expected_mode: "csv_columnar_dictionary",
                expected_source_bytes: Some(232_198),
                expected_source_sha256: Some(
                    "6d164c5e9a4be38a68473320f1cea69377ea2d4aed6d49c024e3d346b8c1bee5",
                ),
            },
            ScenarioSpec {
                name: "high_cardinality_v1",
                profile: Profile::HighCardinality,
                rows: 1_000,
                seed: 7,
                archive: ArchiveConfiguration::V1,
                expected_mode: "raw_zstd",
                expected_source_bytes: Some(296_167),
                expected_source_sha256: Some(
                    "e82769c7f01e6b30615b988ff661f0e257147d09c4c514ff9abd2f97b7811e46",
                ),
            },
            ScenarioSpec {
                name: "random_low_redundancy_v1",
                profile: Profile::Random,
                rows: 1_000,
                seed: 99,
                archive: ArchiveConfiguration::V1,
                expected_mode: "raw_zstd",
                expected_source_bytes: Some(136_555),
                expected_source_sha256: Some(
                    "7e5be3d5b1b265d2101f25c859d8eecb1994fe883c2d63445537bbe3d0d66a12",
                ),
            },
            ScenarioSpec {
                name: "realistic_v2_multichunk",
                profile: Profile::Realistic,
                rows: 4_000,
                seed: 20_260_316,
                archive: ArchiveConfiguration::V2 {
                    chunk_size_bytes: 32 * 1024,
                    threads: 2,
                    max_in_flight_chunks: 2,
                },
                expected_mode: "chunked_raw_zstd",
                expected_source_bytes: Some(619_260),
                expected_source_sha256: Some(
                    "d4b05af47106cc4f5301730019bb9188d4ea5ddb2e5c9f9cf4612b95fcc0a4bb",
                ),
            },
        ],
        Preset::Representative => vec![
            ScenarioSpec {
                name: "repetitive_structured_v1",
                profile: Profile::Repetitive,
                rows: 50_000,
                seed: 42,
                archive: ArchiveConfiguration::V1,
                expected_mode: "csv_columnar_dictionary",
                expected_source_bytes: None,
                expected_source_sha256: None,
            },
            ScenarioSpec {
                name: "realistic_structured_v1",
                profile: Profile::Realistic,
                rows: 25_000,
                seed: 2_026,
                archive: ArchiveConfiguration::V1,
                expected_mode: "csv_columnar_dictionary",
                expected_source_bytes: None,
                expected_source_sha256: None,
            },
            ScenarioSpec {
                name: "high_cardinality_v1",
                profile: Profile::HighCardinality,
                rows: 15_000,
                seed: 7,
                archive: ArchiveConfiguration::V1,
                expected_mode: "raw_zstd",
                expected_source_bytes: None,
                expected_source_sha256: None,
            },
            ScenarioSpec {
                name: "random_low_redundancy_v1",
                profile: Profile::Random,
                rows: 25_000,
                seed: 99,
                archive: ArchiveConfiguration::V1,
                expected_mode: "raw_zstd",
                expected_source_bytes: None,
                expected_source_sha256: None,
            },
            ScenarioSpec {
                name: "realistic_v2_multichunk",
                profile: Profile::Realistic,
                rows: 75_000,
                seed: 20_260_316,
                archive: ArchiveConfiguration::V2 {
                    chunk_size_bytes: 1024 * 1024,
                    threads: 4,
                    max_in_flight_chunks: 8,
                },
                expected_mode: "chunked_raw_zstd",
                expected_source_bytes: None,
                expected_source_sha256: None,
            },
        ],
    }
}

#[derive(Debug, Serialize)]
pub struct PerformanceObservationReportV1 {
    pub schema_version: u32,
    pub report_type: &'static str,
    pub evidence_classification: EvidenceClassificationV1,
    pub environment: EnvironmentMetadataV1,
    pub methodology: MethodologyV1,
    pub scenarios: Vec<ScenarioObservationV1>,
}

#[derive(Debug, Serialize)]
pub struct EvidenceClassificationV1 {
    pub correctness_regression: &'static str,
    pub wall_clock_performance: &'static str,
    pub timing_failure_thresholds: bool,
}

#[derive(Debug, Serialize)]
pub struct MethodologyV1 {
    pub preset: &'static str,
    pub run_count: usize,
    pub warmup_policy: &'static str,
    pub timing_boundary: &'static str,
    pub aggregation: &'static str,
    pub cache_caveat: &'static str,
    pub system_load_caveat: &'static str,
}

#[derive(Debug, Serialize)]
pub struct EnvironmentMetadataV1 {
    pub timestamp_unix_seconds_utc: u64,
    pub git_commit: Option<String>,
    pub git_dirty: Option<bool>,
    pub rust_version: Option<String>,
    pub cargo_version: Option<String>,
    pub build_profile: &'static str,
    pub optimized_build: bool,
    pub os: String,
    pub kernel: Option<String>,
    pub architecture: String,
    pub cpu_model: Option<String>,
    pub logical_cpu_count: Option<usize>,
    pub memory_total_bytes: Option<u64>,
    pub memory_available_bytes: Option<u64>,
    pub wsl_detected: bool,
    pub wsl_distribution: Option<String>,
    pub workspace_path: String,
    pub workspace_path_class: String,
    pub report_output_path: Option<String>,
    pub report_output_path_class: Option<String>,
    pub gpu_information: Option<String>,
    pub gpu_used_by_datapack: bool,
}

#[derive(Debug, Serialize)]
pub struct ScenarioObservationV1 {
    pub scenario: &'static str,
    pub generator_profile: &'static str,
    pub generator_seed: u64,
    pub rows: u64,
    pub source_path_class: String,
    pub archive_path_class: String,
    pub source_size_bytes: u64,
    pub source_sha256: String,
    pub archive_version: u16,
    pub selected_mode: &'static str,
    pub backend: &'static str,
    pub chunk_size_bytes: Option<usize>,
    pub threads: Option<usize>,
    pub max_in_flight_chunks: Option<usize>,
    pub chunk_count: Option<u64>,
    pub archive_size_bytes: u64,
    pub compression_ratio: f64,
    pub archive_sha256_per_run: Vec<String>,
    pub archive_size_stable_across_runs: Option<bool>,
    pub archive_sha256_stable_across_runs: Option<bool>,
    pub compression: TimingObservationV1,
    pub decompression: TimingObservationV1,
    pub correctness: CorrectnessEvidenceV1,
}

#[derive(Debug, Serialize)]
pub struct TimingObservationV1 {
    pub evidence_classification: &'static str,
    pub elapsed_ms_samples: Vec<f64>,
    pub median_elapsed_ms: f64,
    pub median_throughput_mib_per_second: Option<f64>,
}

#[derive(Debug, Serialize)]
pub struct CorrectnessEvidenceV1 {
    pub evidence_classification: &'static str,
    pub expected_archive_version: u16,
    pub expected_selected_mode: &'static str,
    pub restored_size_bytes: u64,
    pub restored_sha256: String,
    pub byte_exact_match: bool,
    pub source_sha256_match: bool,
    pub validation: ValidationEvidenceV1,
}

#[derive(Debug, Serialize)]
pub struct ValidationEvidenceV1 {
    pub valid: bool,
    pub against_original: &'static str,
    pub header: &'static str,
    pub metadata: &'static str,
    pub payload_structure: &'static str,
    pub decompression: &'static str,
    pub restored_length: &'static str,
    pub chunk_table: &'static str,
    pub per_chunk_sha256: &'static str,
    pub global_sha256: &'static str,
    pub trailing_data: &'static str,
}

pub fn run_suite(
    preset: Preset,
    runs: usize,
    workspace: &Path,
    report_output: Option<&Path>,
) -> HarnessResult<PerformanceObservationReportV1> {
    if runs == 0 || runs > 25 {
        return Err(fail("runs must be between 1 and 25"));
    }
    fs::create_dir_all(workspace)?;
    let environment = capture_environment(workspace, report_output);
    let wsl = environment.wsl_detected;
    let os = environment.os.clone();
    let mut observations = Vec::new();
    for scenario in scenarios(preset) {
        observations.push(run_scenario(scenario, runs, workspace, wsl, &os)?);
    }

    Ok(PerformanceObservationReportV1 {
        schema_version: REPORT_SCHEMA_VERSION,
        report_type: "performance_regression_observation",
        evidence_classification: EvidenceClassificationV1 {
            correctness_regression: "deterministic_ci_gate",
            wall_clock_performance: "observational_only",
            timing_failure_thresholds: false,
        },
        environment,
        methodology: MethodologyV1 {
            preset: preset.as_str(),
            run_count: runs,
            warmup_policy: "none; every run is reported",
            timing_boundary: "Application API file-to-file call wall clock",
            aggregation: "median",
            cache_caveat: "No cache flush is attempted; later runs may benefit from operating-system caches.",
            system_load_caveat: "System load is not controlled; timings must be interpreted with the captured environment and run context.",
        },
        scenarios: observations,
    })
}

fn run_scenario(
    scenario: ScenarioSpec,
    runs: usize,
    workspace: &Path,
    wsl: bool,
    os: &str,
) -> HarnessResult<ScenarioObservationV1> {
    let source = workspace.join(format!("{}.csv", scenario.name));
    generation::generate_to_path(
        scenario.profile,
        &source,
        scenario.rows,
        Some(scenario.seed),
    )?;
    let source_size = fs::metadata(&source)?.len();
    let source_sha256 = sha256_path(&source)?;
    check_recorded_source_identity(scenario, source_size, &source_sha256)?;

    let mut compression_samples = Vec::with_capacity(runs);
    let mut decompression_samples = Vec::with_capacity(runs);
    let mut archive_hashes = Vec::with_capacity(runs);
    let mut archive_sizes = Vec::with_capacity(runs);
    let mut final_correctness = None;
    let mut final_version = None;
    let mut final_mode = None;
    let mut final_backend = None;
    let mut final_chunk_count = None;

    for run_index in 0..runs {
        let archive = workspace.join(format!("{}-{run_index}.dpack", scenario.name));
        let restored = workspace.join(format!("{}-{run_index}.restored", scenario.name));

        let mut compress_request = CompressRequest::new(&source, &archive);
        compress_request.format = scenario.format();
        let compression_started = Instant::now();
        let compression = application::compress(compress_request)?;
        let compression_elapsed = compression_started.elapsed();
        compression_samples.push(compression_elapsed);

        ensure_equal(
            "archive version",
            scenario.expected_version(),
            compression.archive_version,
        )?;
        let selected_mode = archive_mode_label(compression.selected_mode);
        ensure_equal("selected mode", scenario.expected_mode, selected_mode)?;
        let backend = codec_backend_label(compression.backend);

        let decompression_started = Instant::now();
        let decompression = application::decompress(DecompressRequest::new(&archive, &restored))?;
        let decompression_elapsed = decompression_started.elapsed();
        decompression_samples.push(decompression_elapsed);

        ensure_equal(
            "decompression archive version",
            compression.archive_version,
            decompression.archive_version,
        )?;
        ensure_equal(
            "decompression selected mode",
            selected_mode,
            archive_mode_label(decompression.selected_mode),
        )?;

        let restored_size = fs::metadata(&restored)?.len();
        let restored_sha256 = sha256_path(&restored)?;
        let byte_exact = files_equal(&source, &restored)?;
        if restored_size != source_size || restored_sha256 != source_sha256 || !byte_exact {
            return Err(fail(format!(
                "{} did not restore byte-exact source identity",
                scenario.name
            )));
        }

        let mut validate_request = ValidateRequest::new(&archive);
        validate_request.against = Some(source.clone());
        let validation = application::validate(validate_request)?;
        let validation_evidence = validation_evidence(&validation);
        validate_required_checks(scenario, &validation_evidence)?;

        let archive_size = fs::metadata(&archive)?.len();
        archive_sizes.push(archive_size);
        archive_hashes.push(sha256_path(&archive)?);
        final_version = Some(compression.archive_version);
        final_mode = Some(selected_mode);
        final_backend = Some(backend);
        final_chunk_count = validation.archive.chunk_count;
        final_correctness = Some(CorrectnessEvidenceV1 {
            evidence_classification: "deterministic_ci_gate",
            expected_archive_version: scenario.expected_version(),
            expected_selected_mode: scenario.expected_mode,
            restored_size_bytes: restored_size,
            restored_sha256,
            byte_exact_match: byte_exact,
            source_sha256_match: true,
            validation: validation_evidence,
        });
    }

    let archive_size_stable = all_equal(&archive_sizes);
    let archive_hash_stable = all_equal(&archive_hashes);
    if !archive_size_stable || !archive_hash_stable {
        return Err(fail(format!(
            "{} did not produce stable archive bytes across runs",
            scenario.name
        )));
    }

    let archive_size = *archive_sizes
        .last()
        .ok_or_else(|| fail("scenario did not produce an archive"))?;
    let chunk_count = final_chunk_count;
    if let ArchiveConfiguration::V2 {
        chunk_size_bytes, ..
    } = scenario.archive
    {
        let expected_chunks = source_size
            .checked_add(u64::try_from(chunk_size_bytes)? - 1)
            .ok_or_else(|| fail("v2 expected chunk count overflow"))?
            / u64::try_from(chunk_size_bytes)?;
        if expected_chunks < 2 {
            return Err(fail(format!(
                "{} did not exercise multiple v2 chunks",
                scenario.name
            )));
        }
        ensure_equal("v2 chunk count", Some(expected_chunks), chunk_count)?;
    }

    let (chunk_size_bytes, threads, max_in_flight_chunks) = scenario.chunk_configuration();
    let stability = (runs > 1).then_some(true);
    Ok(ScenarioObservationV1 {
        scenario: scenario.name,
        generator_profile: profile_label(scenario.profile),
        generator_seed: scenario.seed,
        rows: scenario.rows,
        source_path_class: classify_path(&source, wsl, os),
        archive_path_class: classify_path(workspace, wsl, os),
        source_size_bytes: source_size,
        source_sha256,
        archive_version: final_version.ok_or_else(|| fail("missing archive version"))?,
        selected_mode: final_mode.ok_or_else(|| fail("missing selected mode"))?,
        backend: final_backend.ok_or_else(|| fail("missing backend"))?,
        chunk_size_bytes,
        threads,
        max_in_flight_chunks,
        chunk_count,
        archive_size_bytes: archive_size,
        compression_ratio: if archive_size == 0 {
            0.0
        } else {
            source_size as f64 / archive_size as f64
        },
        archive_sha256_per_run: archive_hashes,
        archive_size_stable_across_runs: stability,
        archive_sha256_stable_across_runs: stability,
        compression: timing_observation(source_size, compression_samples),
        decompression: timing_observation(source_size, decompression_samples),
        correctness: final_correctness.ok_or_else(|| fail("missing correctness evidence"))?,
    })
}

fn check_recorded_source_identity(
    scenario: ScenarioSpec,
    actual_bytes: u64,
    actual_sha256: &str,
) -> HarnessResult<()> {
    if let Some(expected) = scenario.expected_source_bytes {
        ensure_equal("recorded source size", expected, actual_bytes)?;
    }
    if let Some(expected) = scenario.expected_source_sha256 {
        ensure_equal("recorded source SHA-256", expected, actual_sha256)?;
    }
    Ok(())
}

fn validate_required_checks(
    scenario: ScenarioSpec,
    validation: &ValidationEvidenceV1,
) -> HarnessResult<()> {
    if !validation.valid || validation.against_original != "matched" {
        return Err(fail(format!(
            "{} failed validation against its generated source",
            scenario.name
        )));
    }
    for (name, status) in [
        ("header", validation.header),
        ("metadata", validation.metadata),
        ("payload_structure", validation.payload_structure),
        ("decompression", validation.decompression),
        ("restored_length", validation.restored_length),
    ] {
        ensure_equal(name, "passed", status)?;
    }
    match scenario.archive {
        ArchiveConfiguration::V1 => {
            ensure_equal("chunk_table", "not_applicable", validation.chunk_table)?;
            for (name, status) in [
                ("per_chunk_sha256", validation.per_chunk_sha256),
                ("global_sha256", validation.global_sha256),
                ("trailing_data", validation.trailing_data),
            ] {
                ensure_equal(name, "not_available", status)?;
            }
        }
        ArchiveConfiguration::V2 { .. } => {
            for (name, status) in [
                ("chunk_table", validation.chunk_table),
                ("per_chunk_sha256", validation.per_chunk_sha256),
                ("global_sha256", validation.global_sha256),
                ("trailing_data", validation.trailing_data),
            ] {
                ensure_equal(name, "passed", status)?;
            }
        }
    }
    Ok(())
}

fn validation_evidence(report: &application::ValidationReportV1) -> ValidationEvidenceV1 {
    ValidationEvidenceV1 {
        valid: report.valid,
        against_original: against_status_label(report.against.status),
        header: check_status_label(report.checks.header),
        metadata: check_status_label(report.checks.metadata),
        payload_structure: check_status_label(report.checks.payload_structure),
        decompression: check_status_label(report.checks.decompression),
        restored_length: check_status_label(report.checks.restored_length),
        chunk_table: check_status_label(report.checks.chunk_table),
        per_chunk_sha256: check_status_label(report.checks.per_chunk_sha256),
        global_sha256: check_status_label(report.checks.global_sha256),
        trailing_data: check_status_label(report.checks.trailing_data),
    }
}

fn timing_observation(input_bytes: u64, samples: Vec<Duration>) -> TimingObservationV1 {
    let median = median_duration(&samples);
    let seconds = median.as_secs_f64();
    TimingObservationV1 {
        evidence_classification: "observational_only",
        elapsed_ms_samples: samples
            .iter()
            .map(|sample| sample.as_secs_f64() * 1_000.0)
            .collect(),
        median_elapsed_ms: seconds * 1_000.0,
        median_throughput_mib_per_second: if input_bytes == 0 || seconds == 0.0 {
            None
        } else {
            Some(input_bytes as f64 / MIB / seconds)
        },
    }
}

pub fn median_duration(samples: &[Duration]) -> Duration {
    if samples.is_empty() {
        return Duration::ZERO;
    }
    let mut sorted = samples.to_vec();
    sorted.sort_unstable();
    let middle = sorted.len() / 2;
    if sorted.len() % 2 == 1 {
        sorted[middle]
    } else {
        let lower = sorted[middle - 1];
        let upper = sorted[middle];
        upper
            .checked_sub(lower)
            .and_then(|span| lower.checked_add(span / 2))
            .unwrap_or(lower)
    }
}

fn all_equal<T: PartialEq>(values: &[T]) -> bool {
    values
        .first()
        .map(|first| values.iter().all(|value| value == first))
        .unwrap_or(true)
}

fn files_equal(left: &Path, right: &Path) -> HarnessResult<bool> {
    let mut left_reader = BufReader::new(File::open(left)?);
    let mut right_reader = BufReader::new(File::open(right)?);
    let mut left_buffer = [0_u8; HASH_BUFFER_BYTES];
    let mut right_buffer = [0_u8; HASH_BUFFER_BYTES];
    loop {
        let left_read = left_reader.read(&mut left_buffer)?;
        let right_read = right_reader.read(&mut right_buffer)?;
        if left_read != right_read || left_buffer[..left_read] != right_buffer[..right_read] {
            return Ok(false);
        }
        if left_read == 0 {
            return Ok(true);
        }
    }
}

pub fn sha256_path(path: &Path) -> HarnessResult<String> {
    let mut reader = BufReader::new(File::open(path)?);
    let mut buffer = [0_u8; HASH_BUFFER_BYTES];
    let mut hasher = Sha256::new();
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn capture_environment(workspace: &Path, report_output: Option<&Path>) -> EnvironmentMetadataV1 {
    let os = std::env::consts::OS.to_string();
    let wsl = detect_wsl();
    let (memory_total_bytes, memory_available_bytes) = linux_memory();
    EnvironmentMetadataV1 {
        timestamp_unix_seconds_utc: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        git_commit: command_output("git", &["rev-parse", "HEAD"]),
        git_dirty: git_dirty(),
        rust_version: command_output("rustc", &["--version"]),
        cargo_version: command_output("cargo", &["--version"]),
        build_profile: if cfg!(debug_assertions) {
            "debug"
        } else {
            "bench (optimized)"
        },
        optimized_build: !cfg!(debug_assertions),
        os: os.clone(),
        kernel: command_output("uname", &["-srmo"]),
        architecture: std::env::consts::ARCH.to_string(),
        cpu_model: cpu_model(),
        logical_cpu_count: std::thread::available_parallelism().ok().map(usize::from),
        memory_total_bytes,
        memory_available_bytes,
        wsl_detected: wsl,
        wsl_distribution: std::env::var("WSL_DISTRO_NAME").ok(),
        workspace_path: display_absolute(workspace),
        workspace_path_class: classify_path(workspace, wsl, &os),
        report_output_path: report_output.map(display_absolute),
        report_output_path_class: report_output.map(|path| classify_path(path, wsl, &os)),
        gpu_information: std::env::var("DATAPACK_PERF_GPU_INFO").ok(),
        gpu_used_by_datapack: false,
    }
}

pub fn classify_path(path: &Path, wsl: bool, os: &str) -> String {
    classify_rendered_path(&display_absolute(path), wsl, os)
}

pub fn classify_rendered_path(path: &str, wsl: bool, os: &str) -> String {
    let normalized = path.replace('\\', "/");
    if wsl && is_wsl_windows_mount(&normalized) {
        "wsl_windows_backed_filesystem".to_string()
    } else if wsl {
        "wsl_linux_native_filesystem".to_string()
    } else if os == "windows" {
        "native_windows_filesystem".to_string()
    } else if os == "linux" {
        "native_linux_filesystem".to_string()
    } else {
        format!("{os}_filesystem_unclassified")
    }
}

fn is_wsl_windows_mount(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() >= 6
        && path.starts_with("/mnt/")
        && bytes[5].is_ascii_alphabetic()
        && (bytes.len() == 6 || bytes.get(6) == Some(&b'/'))
}

fn display_absolute(path: &Path) -> String {
    if path.is_absolute() {
        path.display().to_string()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
            .display()
            .to_string()
    }
}

fn detect_wsl() -> bool {
    std::env::var_os("WSL_INTEROP").is_some()
        || std::env::var_os("WSL_DISTRO_NAME").is_some()
        || fs::read_to_string("/proc/version")
            .map(|value| value.to_ascii_lowercase().contains("microsoft"))
            .unwrap_or(false)
}

fn git_dirty() -> Option<bool> {
    let output = Command::new("git")
        .args(["status", "--porcelain", "--untracked-files=normal"])
        .output()
        .ok()?;
    output.status.success().then_some(!output.stdout.is_empty())
}

fn command_output(command: &str, arguments: &[&str]) -> Option<String> {
    let output = Command::new(command).args(arguments).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8(output.stdout).ok()?;
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

fn cpu_model() -> Option<String> {
    if let Ok(cpuinfo) = fs::read_to_string("/proc/cpuinfo") {
        for line in cpuinfo.lines() {
            if let Some((key, value)) = line.split_once(':') {
                if matches!(key.trim(), "model name" | "Hardware") {
                    let trimmed = value.trim();
                    if !trimmed.is_empty() {
                        return Some(trimmed.to_string());
                    }
                }
            }
        }
    }
    std::env::var("PROCESSOR_IDENTIFIER").ok()
}

fn linux_memory() -> (Option<u64>, Option<u64>) {
    let Ok(meminfo) = fs::read_to_string("/proc/meminfo") else {
        return (None, None);
    };
    (
        meminfo_bytes(&meminfo, "MemTotal"),
        meminfo_bytes(&meminfo, "MemAvailable"),
    )
}

fn meminfo_bytes(meminfo: &str, wanted: &str) -> Option<u64> {
    meminfo.lines().find_map(|line| {
        let (key, value) = line.split_once(':')?;
        if key != wanted {
            return None;
        }
        value
            .split_whitespace()
            .next()?
            .parse::<u64>()
            .ok()?
            .checked_mul(1024)
    })
}

fn profile_label(profile: Profile) -> &'static str {
    match profile {
        Profile::Repetitive => "repetitive",
        Profile::Realistic => "realistic",
        Profile::HighCardinality => "high_cardinality",
        Profile::Random => "random",
    }
}

fn archive_mode_label(mode: ArchiveModeV1) -> &'static str {
    match mode {
        ArchiveModeV1::RawZstd => "raw_zstd",
        ArchiveModeV1::CsvColumnarDictionary => "csv_columnar_dictionary",
        ArchiveModeV1::ChunkedRawZstd => "chunked_raw_zstd",
        _ => "unknown_future_mode",
    }
}

fn codec_backend_label(backend: CodecBackendV1) -> &'static str {
    backend.as_str()
}

fn check_status_label(status: CheckStatusV1) -> &'static str {
    match status {
        CheckStatusV1::Passed => "passed",
        CheckStatusV1::Failed => "failed",
        CheckStatusV1::NotAvailable => "not_available",
        CheckStatusV1::NotApplicable => "not_applicable",
        CheckStatusV1::NotCompleted => "not_completed",
        _ => "unknown_future_status",
    }
}

fn against_status_label(status: AgainstStatusV1) -> &'static str {
    match status {
        AgainstStatusV1::NotRequested => "not_requested",
        AgainstStatusV1::Matched => "matched",
        AgainstStatusV1::Mismatched => "mismatched",
        AgainstStatusV1::NotCompleted => "not_completed",
        _ => "unknown_future_status",
    }
}

fn ensure_equal<T>(label: &str, expected: T, actual: T) -> HarnessResult<()>
where
    T: PartialEq + fmt::Debug,
{
    if expected == actual {
        Ok(())
    } else {
        Err(fail(format!(
            "{label} mismatch: expected {expected:?}, got {actual:?}"
        )))
    }
}

fn fail(message: impl Into<String>) -> Box<dyn Error> {
    Box::new(HarnessError(message.into()))
}
