use std::path::PathBuf;
use thiserror::Error;

pub type Result<T> = std::result::Result<T, DatapackError>;

#[derive(Debug, Error)]
pub enum DatapackError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("metadata serialization error: {0}")]
    Bincode(#[from] Box<bincode::ErrorKind>),

    #[error("invalid dpack file: {0}")]
    InvalidFormat(String),

    #[error("invalid profile name: {0}")]
    InvalidProfile(String),

    #[error("output path not writable: {0}")]
    OutputNotWritable(String),

    #[error("--rows value out of range: {0}")]
    RowsOutOfRange(u64),

    #[error("file not found or not readable: {0}")]
    AnalyzeRead(String),

    #[error("file is not valid CSV: {0}")]
    InvalidCsv(String),

    #[error("archive parse error for '{path}': {reason}")]
    ArchiveParse { path: PathBuf, reason: String },

    #[error("{operation} failed for input '{input}' and output '{output}': {reason}; output status: {output_status}")]
    OperationFailed {
        operation: &'static str,
        input: PathBuf,
        output: PathBuf,
        reason: String,
        output_status: &'static str,
    },
}

impl DatapackError {
    pub fn exit_code(&self) -> i32 {
        match self {
            Self::InvalidProfile(_) => 1,
            Self::OutputNotWritable(_) => 2,
            Self::RowsOutOfRange(_) => 3,
            Self::AnalyzeRead(_) => 1,
            Self::InvalidCsv(_) => 2,
            _ => 1,
        }
    }
}
