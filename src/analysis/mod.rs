use std::path::Path;

use crate::formats::{csv, txt};
use crate::metadata::{DpackMetadata, FileType};

mod accumulator;
mod delimited_engine;
mod engine;
mod model;
mod report;

pub(crate) const STRUCTURED_ANALYSIS_UNAVAILABLE_CODE: &str = "STRUCTURED_ANALYSIS_UNAVAILABLE";

pub(crate) use delimited_engine::{
    analyze_cli_path, comma_structured_compression_eligible_path,
    structured_compression_eligible_bytes,
};
#[cfg(test)]
pub(crate) use engine::{analyze_path, AnalysisEngine, AnalysisLimits, SampleConfig};
pub(crate) use engine::{analyze_path_with_scope, DatasetAnalysis, PlanDisposition};
pub(crate) use model::{
    AnalysisParser, AnalysisStopReason, CardinalityEstimate, DatasetFacts, DelimitedFormat,
    CARDINALITY_LOWER_BOUND,
};
pub(crate) use report::{
    analysis_stop_reason_code, archive_reason_code, build_report_v1, column_reason_code,
};
pub use report::{
    AnalysisLimitV1, AnalysisReportV1, ArchiveModeV1 as AnalysisArchiveModeV1,
    ColumnNameStatusReportV1, ColumnPlannerReportV1, ColumnReportV1,
    ColumnStrategyV1 as AnalysisColumnStrategyV1, CompletenessV1, DatasetReportV1,
    DiagnosticSeverityV1, DiagnosticV1, EstimateF64ReportV1, EstimateKindV1, EstimateU64ReportV1,
    ParserReportV1, PlannerReportV1, PolicyReportV1, ReasonReportV1, SamplingReportV1,
    SamplingScopeV1, ValueLengthReportV1,
};

pub fn analyze_bytes(path: &Path, bytes: &[u8]) -> DpackMetadata {
    let file_type = FileType::from_path(path);
    let mut metadata = DpackMetadata::new(file_type, bytes.len() as u64);

    match file_type {
        FileType::Csv => metadata.csv = Some(csv::analyze(bytes)),
        FileType::Txt | FileType::Unknown => metadata.txt = Some(txt::analyze(bytes)),
    }

    metadata
}
