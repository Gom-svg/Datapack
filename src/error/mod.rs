use std::path::PathBuf;

use serde::Serialize;
use thiserror::Error;

pub type Result<T> = std::result::Result<T, DatapackError>;

/// Stable, presentation-neutral category for a DataPack failure.
///
/// Categories are intentionally broader than individual [`DatapackError`]
/// variants. Use [`DatapackError::code`] when the specific stable identity is
/// needed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ErrorCategory {
    Io,
    Format,
    Configuration,
    Analysis,
    Output,
    Operation,
    Cancellation,
}

impl ErrorCategory {
    /// Returns the stable snake-case category identifier.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Io => "io",
            Self::Format => "format",
            Self::Configuration => "configuration",
            Self::Analysis => "analysis",
            Self::Output => "output",
            Self::Operation => "operation",
            Self::Cancellation => "cancellation",
        }
    }
}

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
    /// Returns a stable broad category without parsing the display message.
    pub const fn category(&self) -> ErrorCategory {
        match self {
            Self::Io(_) => ErrorCategory::Io,
            Self::Bincode(_) | Self::InvalidFormat(_) | Self::ArchiveParse { .. } => {
                ErrorCategory::Format
            }
            Self::InvalidProfile(_) | Self::RowsOutOfRange(_) => ErrorCategory::Configuration,
            Self::AnalyzeRead(_) => ErrorCategory::Analysis,
            Self::InvalidCsv(_) => ErrorCategory::Format,
            Self::OutputNotWritable(_) => ErrorCategory::Output,
            Self::OperationFailed { .. } => ErrorCategory::Operation,
        }
    }

    /// Returns the stable snake-case identity of this error variant.
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Io(_) => "io_error",
            Self::Bincode(_) => "metadata_serialization_error",
            Self::InvalidFormat(_) => "invalid_format",
            Self::InvalidProfile(_) => "invalid_profile",
            Self::OutputNotWritable(_) => "output_not_writable",
            Self::RowsOutOfRange(_) => "rows_out_of_range",
            Self::AnalyzeRead(_) => "analysis_read",
            Self::InvalidCsv(_) => "invalid_csv",
            Self::ArchiveParse { .. } => "archive_parse",
            Self::OperationFailed { .. } => "operation_failed",
        }
    }

    /// Returns the established CLI process exit status for this failure.
    ///
    /// The stable error code, not the process status, carries machine-readable
    /// error identity. Historical nonzero status values remain unchanged.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_categories_and_codes_are_stable_and_message_independent() {
        let cases = vec![
            (
                DatapackError::Io(std::io::Error::other("first message")),
                ErrorCategory::Io,
                "io_error",
            ),
            (
                DatapackError::Bincode(Box::new(bincode::ErrorKind::Custom(
                    "first message".to_string(),
                ))),
                ErrorCategory::Format,
                "metadata_serialization_error",
            ),
            (
                DatapackError::InvalidFormat("first message".to_string()),
                ErrorCategory::Format,
                "invalid_format",
            ),
            (
                DatapackError::InvalidProfile("first profile".to_string()),
                ErrorCategory::Configuration,
                "invalid_profile",
            ),
            (
                DatapackError::AnalyzeRead("private path".to_string()),
                ErrorCategory::Analysis,
                "analysis_read",
            ),
            (
                DatapackError::RowsOutOfRange(0),
                ErrorCategory::Configuration,
                "rows_out_of_range",
            ),
            (
                DatapackError::InvalidCsv("first CSV reason".to_string()),
                ErrorCategory::Format,
                "invalid_csv",
            ),
            (
                DatapackError::OutputNotWritable("first output".to_string()),
                ErrorCategory::Output,
                "output_not_writable",
            ),
            (
                DatapackError::ArchiveParse {
                    path: PathBuf::from("archive.dpack"),
                    reason: "first reason".to_string(),
                },
                ErrorCategory::Format,
                "archive_parse",
            ),
            (
                DatapackError::OperationFailed {
                    operation: "compress",
                    input: PathBuf::from("input"),
                    output: PathBuf::from("output"),
                    reason: "reason".to_string(),
                    output_status: "no final output was committed",
                },
                ErrorCategory::Operation,
                "operation_failed",
            ),
        ];

        for (error, category, code) in cases {
            assert_eq!(error.category(), category);
            assert_eq!(error.category().as_str(), category.as_str());
            assert_eq!(error.code(), code);
        }

        assert_eq!(
            DatapackError::InvalidFormat("different message".to_string()).code(),
            "invalid_format"
        );
    }

    #[test]
    fn error_category_serialization_uses_adapter_identifiers() {
        let cases = [
            (ErrorCategory::Io, "io"),
            (ErrorCategory::Format, "format"),
            (ErrorCategory::Configuration, "configuration"),
            (ErrorCategory::Analysis, "analysis"),
            (ErrorCategory::Output, "output"),
            (ErrorCategory::Operation, "operation"),
            (ErrorCategory::Cancellation, "cancellation"),
        ];

        for (category, identifier) in cases {
            assert_eq!(category.as_str(), identifier);
            assert_eq!(
                serde_json::to_string(&category).unwrap(),
                format!("\"{identifier}\"")
            );
        }
    }

    #[test]
    fn historical_exit_statuses_remain_unchanged() {
        assert_eq!(
            DatapackError::InvalidProfile("x".to_string()).exit_code(),
            1
        );
        assert_eq!(DatapackError::InvalidCsv("x".to_string()).exit_code(), 2);
        assert_eq!(DatapackError::RowsOutOfRange(0).exit_code(), 3);
    }
}
