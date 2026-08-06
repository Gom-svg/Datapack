use std::path::Path;

use crate::formats::{csv, txt};
use crate::metadata::{DpackMetadata, FileType};

mod accumulator;
mod delimited_engine;
mod engine;
mod model;
mod report;

pub(crate) use delimited_engine::{
    analyze_cli_path, comma_structured_compression_eligible_bytes,
    comma_structured_compression_eligible_path,
};
pub(crate) use engine::{analyze_path, analyze_path_with_scope, DatasetAnalysis};
#[cfg(test)]
pub(crate) use engine::{AnalysisEngine, AnalysisLimits, SampleConfig};
#[cfg(test)]
pub(crate) use model::AnalysisStopReason;
pub(crate) use model::{
    AnalysisParser, CardinalityEstimate, DatasetFacts, DelimitedFormat, CARDINALITY_LOWER_BOUND,
};
pub(crate) use report::build_report_v1;

pub fn analyze_bytes(path: &Path, bytes: &[u8]) -> DpackMetadata {
    let file_type = FileType::from_path(path);
    let mut metadata = DpackMetadata::new(file_type, bytes.len() as u64);

    match file_type {
        FileType::Csv => metadata.csv = Some(csv::analyze(bytes)),
        FileType::Txt | FileType::Unknown => metadata.txt = Some(txt::analyze(bytes)),
    }

    metadata
}
