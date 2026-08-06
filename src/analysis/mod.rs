use std::path::Path;

use crate::formats::{csv, txt};
use crate::metadata::{DpackMetadata, FileType};

mod accumulator;
mod engine;
mod model;
mod report;

pub(crate) use engine::{analyze_path, DatasetAnalysis};
#[cfg(test)]
pub(crate) use engine::{AnalysisEngine, SampleConfig};
#[cfg(test)]
pub(crate) use model::AnalysisStopReason;
pub(crate) use model::{CardinalityEstimate, DatasetFacts, CARDINALITY_LOWER_BOUND};
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
