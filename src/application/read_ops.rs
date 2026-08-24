use crate::analysis::AnalysisReportV1;
use crate::archive_validation::{self, ValidationOptions, ValidationReportV1};
use crate::comparison::{self, CompareOptions, ComparisonMode, ComparisonReportV1};
use crate::error::{DatapackError, Result};

use super::model::{AnalyzeRequest, CompareMode, CompareRequest, ValidateRequest};
use super::progress::{OperationKind, ProgressEmitter, ProgressObserver, ProgressPhase};

pub(super) fn analyze(request: AnalyzeRequest) -> Result<AnalysisReportV1> {
    let mut emitter = ProgressEmitter::silent(OperationKind::Analyze);
    analyze_inner(request, &mut emitter)
}

pub(super) fn analyze_with_progress(
    request: AnalyzeRequest,
    observer: &mut dyn ProgressObserver,
) -> Result<AnalysisReportV1> {
    let mut emitter = ProgressEmitter::observed(OperationKind::Analyze, observer);
    analyze_inner(request, &mut emitter)
}

fn analyze_inner(
    request: AnalyzeRequest,
    emitter: &mut ProgressEmitter<'_>,
) -> Result<AnalysisReportV1> {
    let analysis = analyze_dataset(request, emitter)?;
    let report = crate::analysis::build_report_v1(&analysis)?;
    emitter.succeeded();
    Ok(report)
}

fn analyze_dataset(
    request: AnalyzeRequest,
    emitter: &mut ProgressEmitter<'_>,
) -> Result<crate::analysis::DatasetAnalysis> {
    emitter.started(ProgressPhase::Analyzing, None);
    let analysis = crate::analysis::analyze_cli_path(&request.input, request.sample_mb)?;
    let sample_budget = analysis
        .facts
        .coverage
        .scope_size_bytes
        .min(analysis.facts.coverage.max_bytes);
    emitter.completed(
        ProgressPhase::Analyzing,
        analysis.facts.coverage.bytes_analyzed,
        Some(sample_budget),
    );
    Ok(analysis)
}

pub(crate) fn analyze_for_cli_with_progress(
    request: AnalyzeRequest,
    observer: &mut dyn ProgressObserver,
) -> Result<crate::analysis::DatasetAnalysis> {
    let mut emitter = ProgressEmitter::observed(OperationKind::Analyze, observer);
    let analysis = analyze_dataset(request, &mut emitter)?;
    emitter.succeeded();
    Ok(analysis)
}

pub(super) fn validate(request: ValidateRequest) -> Result<ValidationReportV1> {
    let mut emitter = ProgressEmitter::silent(OperationKind::Validate);
    validate_inner(request, &mut emitter)
}

pub(super) fn validate_with_progress(
    request: ValidateRequest,
    observer: &mut dyn ProgressObserver,
) -> Result<ValidationReportV1> {
    let mut emitter = ProgressEmitter::observed(OperationKind::Validate, observer);
    validate_inner(request, &mut emitter)
}

fn validate_inner(
    request: ValidateRequest,
    emitter: &mut ProgressEmitter<'_>,
) -> Result<ValidationReportV1> {
    if request.max_memory_bytes == 0 {
        return Err(DatapackError::InvalidFormat(
            "max_memory_bytes must be greater than zero".to_string(),
        ));
    }
    if request.max_output_bytes == Some(0) {
        return Err(DatapackError::InvalidFormat(
            "max_output_bytes must be greater than zero".to_string(),
        ));
    }
    if request.max_chunks == Some(0) {
        return Err(DatapackError::InvalidFormat(
            "max_chunks must be greater than zero".to_string(),
        ));
    }

    emitter.started(ProgressPhase::Validating, None);
    let report = archive_validation::validate_path(
        &request.archive,
        request.against.as_deref(),
        ValidationOptions {
            max_output_bytes: request.max_output_bytes,
            max_chunks: request.max_chunks,
            max_memory_bytes: request.max_memory_bytes,
        },
    )?;
    emitter.completed(
        ProgressPhase::Validating,
        report.archive.archive_size_bytes,
        Some(report.archive.archive_size_bytes),
    );
    emitter.succeeded();
    Ok(report)
}

pub(super) fn compare(request: CompareRequest) -> Result<ComparisonReportV1> {
    let mut emitter = ProgressEmitter::silent(OperationKind::Compare);
    compare_inner(request, &mut emitter)
}

pub(super) fn compare_with_progress(
    request: CompareRequest,
    observer: &mut dyn ProgressObserver,
) -> Result<ComparisonReportV1> {
    let mut emitter = ProgressEmitter::observed(OperationKind::Compare, observer);
    compare_inner(request, &mut emitter)
}

fn compare_inner(
    request: CompareRequest,
    emitter: &mut ProgressEmitter<'_>,
) -> Result<ComparisonReportV1> {
    emitter.started(ProgressPhase::Comparing, None);
    let report = comparison::compare_path(
        &request.input,
        CompareOptions {
            mode: match request.mode {
                CompareMode::Quick => ComparisonMode::Quick,
                CompareMode::Full => ComparisonMode::Full,
            },
            runs: request.runs,
            max_input_mb: request.max_input_mb,
        },
    )?;
    emitter.completed(
        ProgressPhase::Comparing,
        report.scope.compared_size_bytes,
        Some(report.scope.compared_size_bytes),
    );
    emitter.succeeded();
    Ok(report)
}
