use std::path::PathBuf;

use serde::Serialize;

use crate::storage;

pub const DEFAULT_SAMPLE_MB: u64 = 64;
pub const DEFAULT_MAX_DICTIONARY_VALUES: u64 = 65_535;
pub const DEFAULT_MAX_DICTIONARY_MB: u64 = 64;
pub const DEFAULT_VALIDATION_MEMORY_BYTES: u64 = 512 * 1024 * 1024;

#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct AnalyzeRequest {
    pub input: PathBuf,
    pub sample_mb: u64,
}

impl AnalyzeRequest {
    pub fn new(input: impl Into<PathBuf>) -> Self {
        Self {
            input: input.into(),
            sample_mb: DEFAULT_SAMPLE_MB,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CompressionMode {
    Fast,
    Best,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CompressionBackend {
    ChunkedRawZstd,
    ZstdMtExperimental,
}

impl CompressionBackend {
    pub(crate) const fn storage_backend(self) -> storage::chunked::ChunkedBackend {
        match self {
            Self::ChunkedRawZstd => storage::chunked::ChunkedBackend::ChunkedRawZstd,
            Self::ZstdMtExperimental => storage::chunked::ChunkedBackend::ZstdMtExperimental,
        }
    }
}

/// Codec/backend fact reported by compression and decompression operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum CodecBackendV1 {
    Zstd,
    RawZstdStreaming,
    ChunkedRawZstd,
    ZstdMtExperimental,
    V2RawZstdFrame,
}

impl CodecBackendV1 {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Zstd => "zstd",
            Self::RawZstdStreaming => "raw-zstd-streaming",
            Self::ChunkedRawZstd => "chunked-raw-zstd",
            Self::ZstdMtExperimental => "zstd-mt-experimental",
            Self::V2RawZstdFrame => "v2-raw-zstd-frame",
        }
    }
}

#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct V1CompressionOptions {
    pub mode: CompressionMode,
    pub sample_mb: u64,
    pub verify_best: bool,
    pub max_dictionary_values: u64,
    pub max_dictionary_mb: u64,
}

impl Default for V1CompressionOptions {
    fn default() -> Self {
        Self {
            mode: CompressionMode::Fast,
            sample_mb: DEFAULT_SAMPLE_MB,
            verify_best: false,
            max_dictionary_values: DEFAULT_MAX_DICTIONARY_VALUES,
            max_dictionary_mb: DEFAULT_MAX_DICTIONARY_MB,
        }
    }
}

#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct V2CompressionOptions {
    pub chunk_size_bytes: usize,
    pub threads: usize,
    pub max_in_flight_chunks: usize,
    pub backend: CompressionBackend,
    pub adaptive_level: bool,
    pub max_memory_bytes: Option<u64>,
}

impl Default for V2CompressionOptions {
    fn default() -> Self {
        let threads = storage::chunked::default_thread_count();
        Self {
            chunk_size_bytes: (storage::chunked::DEFAULT_CHUNK_SIZE_MB * 1024 * 1024) as usize,
            threads,
            max_in_flight_chunks: storage::chunked::default_max_in_flight_chunks(threads),
            backend: CompressionBackend::ChunkedRawZstd,
            adaptive_level: false,
            max_memory_bytes: None,
        }
    }
}

#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum CompressionFormat {
    V1(V1CompressionOptions),
    V2(V2CompressionOptions),
}

#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct CompressRequest {
    pub input: PathBuf,
    pub output: PathBuf,
    pub format: CompressionFormat,
    pub overwrite: bool,
    pub keep_partial: bool,
}

impl CompressRequest {
    pub fn new(input: impl Into<PathBuf>, output: impl Into<PathBuf>) -> Self {
        Self {
            input: input.into(),
            output: output.into(),
            format: CompressionFormat::V1(V1CompressionOptions::default()),
            overwrite: false,
            keep_partial: false,
        }
    }
}

#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct DecompressRequest {
    pub archive: PathBuf,
    pub output: PathBuf,
    pub verify: bool,
    pub max_output_bytes: Option<u64>,
    pub max_chunks: Option<u64>,
    pub max_memory_bytes: Option<u64>,
    pub overwrite: bool,
    pub keep_partial: bool,
}

impl DecompressRequest {
    pub fn new(archive: impl Into<PathBuf>, output: impl Into<PathBuf>) -> Self {
        Self {
            archive: archive.into(),
            output: output.into(),
            verify: true,
            max_output_bytes: None,
            max_chunks: None,
            max_memory_bytes: None,
            overwrite: false,
            keep_partial: false,
        }
    }
}

#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct ValidateRequest {
    pub archive: PathBuf,
    pub against: Option<PathBuf>,
    pub max_output_bytes: Option<u64>,
    pub max_chunks: Option<u64>,
    pub max_memory_bytes: u64,
}

impl ValidateRequest {
    pub fn new(archive: impl Into<PathBuf>) -> Self {
        Self {
            archive: archive.into(),
            against: None,
            max_output_bytes: None,
            max_chunks: None,
            max_memory_bytes: DEFAULT_VALIDATION_MEMORY_BYTES,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CompareMode {
    Quick,
    Full,
}

#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct CompareRequest {
    pub input: PathBuf,
    pub mode: CompareMode,
    pub runs: usize,
    pub max_input_mb: Option<u64>,
}

impl CompareRequest {
    pub fn new(input: impl Into<PathBuf>) -> Self {
        Self {
            input: input.into(),
            mode: CompareMode::Quick,
            runs: 3,
            max_input_mb: None,
        }
    }
}

#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct BenchmarkRequest {
    pub input: PathBuf,
    pub quick: bool,
    pub runs: usize,
    pub include_zstd_baseline: bool,
    pub roundtrip: bool,
    pub hash: bool,
    pub estimate_only: bool,
    pub max_input_mb: Option<u64>,
    pub chunked: Option<V2CompressionOptions>,
    pub keep_artifacts: bool,
}

impl BenchmarkRequest {
    pub fn new(input: impl Into<PathBuf>) -> Self {
        Self {
            input: input.into(),
            quick: false,
            runs: 3,
            include_zstd_baseline: true,
            roundtrip: true,
            hash: true,
            estimate_only: false,
            max_input_mb: None,
            chunked: None,
            keep_artifacts: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ArchiveModeV1 {
    RawZstd,
    CsvColumnarDictionary,
    ChunkedRawZstd,
}

#[derive(Debug, Clone, Serialize)]
#[non_exhaustive]
pub struct OperationDiagnosticV1 {
    pub code: String,
    pub message: String,
    pub column_index: Option<usize>,
}

#[derive(Debug, Clone, Default, Serialize)]
#[non_exhaustive]
pub struct OperationProfileV1 {
    pub planning_ms: Option<u64>,
    pub read_ms: Option<u64>,
    pub transform_ms: u64,
    pub write_ms: Option<u64>,
    pub total_ms: u64,
    pub throughput_mib_per_second: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
#[non_exhaustive]
pub struct CompressionResultV1 {
    pub schema_version: u32,
    pub report_type: &'static str,
    pub archive_version: u16,
    pub selected_mode: ArchiveModeV1,
    pub input_size_bytes: u64,
    pub archive_size_bytes: u64,
    pub backend: CodecBackendV1,
    pub diagnostics: Vec<OperationDiagnosticV1>,
    pub profile: OperationProfileV1,
    #[serde(skip)]
    pub(crate) cli_notices: Vec<CompressionNotice>,
    #[serde(skip)]
    pub(crate) chunked_stats: Option<storage::chunked::ChunkedStats>,
}

#[derive(Debug, Clone, Serialize)]
#[non_exhaustive]
pub struct DecompressionResultV1 {
    pub schema_version: u32,
    pub report_type: &'static str,
    pub archive_version: u16,
    pub selected_mode: ArchiveModeV1,
    pub archive_size_bytes: u64,
    pub restored_size_bytes: u64,
    pub verified: Option<bool>,
    pub backend: CodecBackendV1,
    pub diagnostics: Vec<OperationDiagnosticV1>,
    pub profile: OperationProfileV1,
    #[serde(skip)]
    pub(crate) chunked_stats: Option<storage::chunked::ChunkedStats>,
}

#[derive(Debug, Clone)]
pub(crate) enum CompressionNotice {
    StructuredAnalysisUnavailable {
        reason: String,
    },
    AnalysisLimited {
        limitation_codes: Vec<String>,
    },
    DictionaryLimitApplied {
        column_name: String,
    },
    VerifyBest {
        selected_mode: String,
        saved_bytes: usize,
    },
    ColumnarCandidateError {
        error: String,
    },
}

#[derive(Debug, Clone, Serialize)]
#[non_exhaustive]
pub struct ChunkedBenchmarkReportV1 {
    pub backend: String,
    pub chunk_size_mb: f64,
    pub threads: usize,
    pub max_in_flight_chunks: usize,
    pub size_bytes: u64,
    pub compression_ratio: f64,
    pub compression_time_ms: f64,
    pub compression_mib_per_second: f64,
    pub decompression_time_ms: Option<f64>,
    pub decompression_mib_per_second: Option<f64>,
    pub roundtrip_sha256_match: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum BenchmarkScopeV1 {
    EstimateOnly,
    Full,
    Partial,
    Sampled,
}

impl BenchmarkScopeV1 {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::EstimateOnly => "estimate_only",
            Self::Full => "full",
            Self::Partial => "partial",
            Self::Sampled => "sampled",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum BenchmarkValidationStatusV1 {
    NotValidated,
    PartiallyValidated,
    Validated,
}

impl BenchmarkValidationStatusV1 {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NotValidated => "not_validated",
            Self::PartiallyValidated => "partially_validated",
            Self::Validated => "validated",
        }
    }
}

#[derive(Debug, Clone, Default, Serialize)]
#[non_exhaustive]
pub struct BenchmarkArtifactsV1 {
    pub datapack: Option<PathBuf>,
    pub restored: Option<PathBuf>,
    pub zstd: Option<PathBuf>,
    pub chunked: Option<PathBuf>,
    pub chunked_restored: Option<PathBuf>,
    pub chunked_sample: Option<PathBuf>,
}

#[derive(Debug, Clone, Default, Serialize)]
#[non_exhaustive]
pub struct BenchmarkProfileV1 {
    pub planning_ms: Option<u64>,
    pub read_input_ms: Option<u64>,
    pub zstd_only_ms: Option<u64>,
    pub datapack_compress_ms: Option<u64>,
    pub datapack_decompress_ms: Option<u64>,
    pub hash_ms: Option<u64>,
    pub archive_write_ms: Option<u64>,
    pub archive_read_ms: Option<u64>,
    pub total_elapsed_ms: Option<u64>,
    pub compression_mib_per_second: Option<f64>,
    pub decompression_mib_per_second: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
#[non_exhaustive]
pub struct BenchmarkReportV1 {
    pub schema_version: u32,
    pub report_type: &'static str,
    pub estimate_only: bool,
    pub source_size_bytes: u64,
    pub measured_input_size_bytes: u64,
    pub scope: BenchmarkScopeV1,
    pub validation_status: BenchmarkValidationStatusV1,
    pub input_sampled: bool,
    pub zstd_baseline_performed: bool,
    pub roundtrip_performed: bool,
    pub hash_performed: bool,
    pub estimated_mode: ArchiveModeV1,
    pub selected_mode: Option<ArchiveModeV1>,
    pub plan_was_correct: Option<bool>,
    pub peak_memory_estimate_mb: Option<f32>,
    pub planning_time_ms: u64,
    pub runs_used: usize,
    pub datapack_size_bytes: Option<u64>,
    pub zstd_size_bytes: Option<u64>,
    pub compression_ratio: Option<f64>,
    pub zstd_ratio: Option<f64>,
    pub compression_time_ms: Option<f64>,
    pub compression_mib_per_second: Option<f64>,
    pub decompression_time_ms: Option<f64>,
    pub decompression_mib_per_second: Option<f64>,
    pub zstd_compression_time_ms: Option<f64>,
    pub zstd_compression_mib_per_second: Option<f64>,
    pub roundtrip_sha256_match: Option<bool>,
    pub datapack_beats_zstd: Option<bool>,
    pub columnar_candidate_error: Option<String>,
    pub total_elapsed_time_ms: u64,
    pub partial_reasons: Vec<String>,
    pub chunked: Option<ChunkedBenchmarkReportV1>,
    pub artifacts: BenchmarkArtifactsV1,
    pub profile: BenchmarkProfileV1,
}
