use crate::analysis::AnalysisReportV1;
use crate::archive_validation::{self, ValidationOptions, ValidationReportV1};
use crate::comparison::{self, CompareOptions, ComparisonMode, ComparisonReportV1};
use crate::error::{DatapackError, Result};

use super::control::{uncontrolled, OperationContext, OperationControl, OperationResult};
use super::model::{AnalyzeRequest, CompareMode, CompareRequest, ValidateRequest};
use super::progress::{OperationKind, ProgressObserver, ProgressPhase};

pub(super) fn analyze(request: AnalyzeRequest) -> Result<AnalysisReportV1> {
    let mut context = OperationContext::silent(OperationKind::Analyze);
    uncontrolled(analyze_inner(request, &mut context))
}

pub(super) fn analyze_with_progress(
    request: AnalyzeRequest,
    observer: &mut dyn ProgressObserver,
) -> Result<AnalysisReportV1> {
    let mut context = OperationContext::observed(OperationKind::Analyze, observer);
    uncontrolled(analyze_inner(request, &mut context))
}

pub(super) fn analyze_with_control(
    request: AnalyzeRequest,
    control: OperationControl<'_>,
) -> OperationResult<AnalysisReportV1> {
    let mut context = OperationContext::controlled(OperationKind::Analyze, control);
    analyze_inner(request, &mut context)
}

fn analyze_inner(
    request: AnalyzeRequest,
    context: &mut OperationContext<'_>,
) -> OperationResult<AnalysisReportV1> {
    let analysis = analyze_dataset(request, context)?;
    let report = crate::analysis::build_report_v1(&analysis)?;
    context.checkpoint()?;
    context.succeeded();
    Ok(report)
}

fn analyze_dataset(
    request: AnalyzeRequest,
    context: &mut OperationContext<'_>,
) -> OperationResult<crate::analysis::DatasetAnalysis> {
    context.started(ProgressPhase::Analyzing, None)?;
    let analysis = crate::analysis::analyze_cli_path(&request.input, request.sample_mb)?;
    let sample_budget = analysis
        .facts
        .coverage
        .scope_size_bytes
        .min(analysis.facts.coverage.max_bytes);
    context.completed(
        ProgressPhase::Analyzing,
        analysis.facts.coverage.bytes_analyzed,
        Some(sample_budget),
    )?;
    Ok(analysis)
}

pub(crate) fn analyze_for_cli_with_progress(
    request: AnalyzeRequest,
    observer: &mut dyn ProgressObserver,
) -> Result<crate::analysis::DatasetAnalysis> {
    let mut context = OperationContext::observed(OperationKind::Analyze, observer);
    uncontrolled((|| {
        let analysis = analyze_dataset(request, &mut context)?;
        context.checkpoint()?;
        context.succeeded();
        Ok(analysis)
    })())
}

pub(super) fn validate(request: ValidateRequest) -> Result<ValidationReportV1> {
    let mut context = OperationContext::silent(OperationKind::Validate);
    uncontrolled(validate_inner(request, &mut context))
}

pub(super) fn validate_with_progress(
    request: ValidateRequest,
    observer: &mut dyn ProgressObserver,
) -> Result<ValidationReportV1> {
    let mut context = OperationContext::observed(OperationKind::Validate, observer);
    uncontrolled(validate_inner(request, &mut context))
}

pub(super) fn validate_with_control(
    request: ValidateRequest,
    control: OperationControl<'_>,
) -> OperationResult<ValidationReportV1> {
    let mut context = OperationContext::controlled(OperationKind::Validate, control);
    validate_inner(request, &mut context)
}

fn validate_inner(
    request: ValidateRequest,
    context: &mut OperationContext<'_>,
) -> OperationResult<ValidationReportV1> {
    context.checkpoint()?;
    if request.max_memory_bytes == 0 {
        return Err(DatapackError::InvalidFormat(
            "max_memory_bytes must be greater than zero".to_string(),
        )
        .into());
    }
    if request.max_output_bytes == Some(0) {
        return Err(DatapackError::InvalidFormat(
            "max_output_bytes must be greater than zero".to_string(),
        )
        .into());
    }
    if request.max_chunks == Some(0) {
        return Err(DatapackError::InvalidFormat(
            "max_chunks must be greater than zero".to_string(),
        )
        .into());
    }

    context.started(ProgressPhase::Validating, None)?;
    let cancellation = context.cancellation().cloned();
    let report = archive_validation::validate_path_with_control(
        &request.archive,
        request.against.as_deref(),
        ValidationOptions {
            max_output_bytes: request.max_output_bytes,
            max_chunks: request.max_chunks,
            max_memory_bytes: request.max_memory_bytes,
        },
        cancellation.as_ref(),
    )?;
    context.completed(
        ProgressPhase::Validating,
        report.archive.archive_size_bytes,
        Some(report.archive.archive_size_bytes),
    )?;
    context.succeeded();
    Ok(report)
}

pub(super) fn compare(request: CompareRequest) -> Result<ComparisonReportV1> {
    let mut context = OperationContext::silent(OperationKind::Compare);
    uncontrolled(compare_inner(request, &mut context))
}

pub(super) fn compare_with_progress(
    request: CompareRequest,
    observer: &mut dyn ProgressObserver,
) -> Result<ComparisonReportV1> {
    let mut context = OperationContext::observed(OperationKind::Compare, observer);
    uncontrolled(compare_inner(request, &mut context))
}

pub(super) fn compare_with_control(
    request: CompareRequest,
    control: OperationControl<'_>,
) -> OperationResult<ComparisonReportV1> {
    let mut context = OperationContext::controlled(OperationKind::Compare, control);
    compare_inner(request, &mut context)
}

fn compare_inner(
    request: CompareRequest,
    context: &mut OperationContext<'_>,
) -> OperationResult<ComparisonReportV1> {
    context.started(ProgressPhase::Comparing, None)?;
    let cancellation = context.cancellation().cloned();
    let report = comparison::compare_path_with_control(
        &request.input,
        CompareOptions {
            mode: match request.mode {
                CompareMode::Quick => ComparisonMode::Quick,
                CompareMode::Full => ComparisonMode::Full,
            },
            runs: request.runs,
            max_input_mb: request.max_input_mb,
        },
        cancellation.as_ref(),
    )?;
    context.completed(
        ProgressPhase::Comparing,
        report.scope.compared_size_bytes,
        Some(report.scope.compared_size_bytes),
    )?;
    context.succeeded();
    Ok(report)
}
