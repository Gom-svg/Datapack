use std::path::Path;

use crate::formats::{csv, txt};
use crate::metadata::{DpackMetadata, FileType};

pub fn analyze_bytes(path: &Path, bytes: &[u8]) -> DpackMetadata {
    let file_type = FileType::from_path(path);
    let mut metadata = DpackMetadata::new(file_type, bytes.len() as u64);

    match file_type {
        FileType::Csv => metadata.csv = Some(csv::analyze(bytes)),
        FileType::Txt | FileType::Unknown => metadata.txt = Some(txt::analyze(bytes)),
    }

    metadata
}
