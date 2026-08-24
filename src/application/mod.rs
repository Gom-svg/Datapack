//! Path-based application services independent of command-line presentation.
//!
//! Requests and versioned results are owned values. Service operations do not
//! read terminal state or write to standard output/error. Optional observers
//! receive synchronous, privacy-safe progress facts on the calling thread.

mod benchmark;
mod compress;
mod decompress;
mod io;
mod model;
mod progress;
mod read_ops;
mod validation;

pub use crate::analysis::{
    AnalysisArchiveModeV1, AnalysisColumnStrategyV1, AnalysisLimitV1, AnalysisReportV1,
    ColumnNameStatusReportV1, ColumnPlannerReportV1, ColumnReportV1, CompletenessV1,
    DatasetReportV1, DiagnosticSeverityV1, DiagnosticV1, EstimateF64ReportV1, EstimateKindV1,
    EstimateU64ReportV1, ParserReportV1, PlannerReportV1, PolicyReportV1, ReasonReportV1,
    SamplingReportV1, SamplingScopeV1, ValueLengthReportV1,
};
pub use crate::archive_validation::{
    AgainstReportV1, AgainstStatusV1, ArchiveFormatV1, ArchiveReportV1, CheckStatusV1,
    PayloadModeV1, ValidationChecksV1, ValidationDiagnosticV1, ValidationReportV1,
    ValidationSeverityV1,
};
pub use crate::comparison::{
    ComparisonLimitationV1, ComparisonMethodologyV1, ComparisonReportV1, ComparisonScopeV1,
    ComparisonWinnersV1, CompetitorReportV1, CompetitorValidationV1, TimingReportV1,
};
pub(crate) use model::CompressionNotice;
pub use model::{
    AnalyzeRequest, ArchiveModeV1, BenchmarkArtifactsV1, BenchmarkProfileV1, BenchmarkReportV1,
    BenchmarkRequest, BenchmarkScopeV1, BenchmarkValidationStatusV1, ChunkedBenchmarkReportV1,
    CodecBackendV1, CompareMode, CompareRequest, CompressRequest, CompressionBackend,
    CompressionFormat, CompressionMode, CompressionResultV1, DecompressRequest,
    DecompressionResultV1, OperationDiagnosticV1, OperationProfileV1, V1CompressionOptions,
    V2CompressionOptions, ValidateRequest, DEFAULT_MAX_DICTIONARY_MB,
    DEFAULT_MAX_DICTIONARY_VALUES, DEFAULT_SAMPLE_MB, DEFAULT_VALIDATION_MEMORY_BYTES,
};
pub use progress::{OperationKind, ProgressEvent, ProgressObserver, ProgressPhase, ProgressState};

pub fn analyze(request: AnalyzeRequest) -> crate::error::Result<AnalysisReportV1> {
    read_ops::analyze(request)
}

pub fn benchmark(request: BenchmarkRequest) -> crate::error::Result<BenchmarkReportV1> {
    benchmark::benchmark(request)
}

pub fn benchmark_with_progress(
    request: BenchmarkRequest,
    observer: &mut dyn ProgressObserver,
) -> crate::error::Result<BenchmarkReportV1> {
    benchmark::benchmark_with_progress(request, observer)
}

pub fn analyze_with_progress(
    request: AnalyzeRequest,
    observer: &mut dyn ProgressObserver,
) -> crate::error::Result<AnalysisReportV1> {
    read_ops::analyze_with_progress(request, observer)
}

pub(crate) use read_ops::analyze_for_cli_with_progress;

pub fn validate(request: ValidateRequest) -> crate::error::Result<ValidationReportV1> {
    read_ops::validate(request)
}

pub fn validate_with_progress(
    request: ValidateRequest,
    observer: &mut dyn ProgressObserver,
) -> crate::error::Result<ValidationReportV1> {
    read_ops::validate_with_progress(request, observer)
}

pub fn compare(request: CompareRequest) -> crate::error::Result<ComparisonReportV1> {
    read_ops::compare(request)
}

pub fn compare_with_progress(
    request: CompareRequest,
    observer: &mut dyn ProgressObserver,
) -> crate::error::Result<ComparisonReportV1> {
    read_ops::compare_with_progress(request, observer)
}

pub fn compress(request: CompressRequest) -> crate::error::Result<CompressionResultV1> {
    compress::compress(request)
}

pub fn compress_with_progress(
    request: CompressRequest,
    observer: &mut dyn ProgressObserver,
) -> crate::error::Result<CompressionResultV1> {
    compress::compress_with_progress(request, observer)
}

pub fn decompress(request: DecompressRequest) -> crate::error::Result<DecompressionResultV1> {
    decompress::decompress(request)
}

pub fn decompress_with_progress(
    request: DecompressRequest,
    observer: &mut dyn ProgressObserver,
) -> crate::error::Result<DecompressionResultV1> {
    decompress::decompress_with_progress(request, observer)
}
