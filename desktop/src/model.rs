use std::path::{Path, PathBuf};

use datapack::application::{self as api, ProgressEvent, ProgressPhase};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    Data,
    Archive,
}

#[derive(Debug, Clone)]
pub struct SelectedFile {
    pub path: PathBuf,
    pub bytes: u64,
    pub kind: FileKind,
}

impl SelectedFile {
    /// Selection only examines metadata. Even a multi-GiB file is never previewed.
    pub fn inspect(path: PathBuf, kind: FileKind) -> std::io::Result<Self> {
        let metadata = std::fs::metadata(&path)?;
        if !metadata.is_file() {
            return Err(std::io::Error::other(
                "Choose a regular file, not a folder or device.",
            ));
        }
        Ok(Self {
            path,
            bytes: metadata.len(),
            kind,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operation {
    Select,
    Analyze,
    Compress,
    Validate,
    Decompress,
}

impl Operation {
    pub fn label(self) -> &'static str {
        match self {
            Self::Select => "Selecting file",
            Self::Analyze => "Analyzing",
            Self::Compress => "Compressing",
            Self::Validate => "Validating",
            Self::Decompress => "Decompressing",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Progress {
    pub phase: &'static str,
    pub percent: Option<f32>,
    pub bytes: u64,
    pub total: Option<u64>,
    pub chunks: Option<(u64, Option<u64>)>,
}

impl From<&ProgressEvent> for Progress {
    fn from(event: &ProgressEvent) -> Self {
        let phase = match event.phase {
            ProgressPhase::Analyzing => "Analyzing",
            ProgressPhase::Planning => "Planning",
            ProgressPhase::ReadingInput => "Reading data",
            ProgressPhase::Hashing => "Checking source integrity",
            ProgressPhase::Compressing => "Compressing",
            ProgressPhase::WritingArchive => "Writing archive",
            ProgressPhase::ReadingArchive => "Reading archive",
            ProgressPhase::Decompressing => "Decompressing",
            ProgressPhase::WritingOutput => "Writing restored file",
            ProgressPhase::Validating => "Validating",
            ProgressPhase::Comparing => "Comparing",
            ProgressPhase::CleaningUp => "Cleaning up",
            ProgressPhase::Finalizing => "Finalizing",
            _ => "Working",
        };
        Self {
            phase,
            percent: event.percentage().map(|value| value as f32),
            bytes: event.completed_bytes,
            total: event.total_bytes,
            chunks: event
                .total_items
                .map(|total| (event.completed_items, Some(total))),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Problem {
    pub message: String,
    pub code: &'static str,
    pub category: &'static str,
    pub context: String,
}

impl Problem {
    pub fn from_engine(error: &api::OperationError) -> Self {
        let message = match error.category() {
            datapack::error::ErrorCategory::Format => "DataPack could not read this format. Check the file or use Chunked compression to preserve unsupported data.",
            datapack::error::ErrorCategory::Output => "The destination is unavailable. Choose a new filename in a writable folder.",
            datapack::error::ErrorCategory::Analysis => "The data could not be analyzed. Check that the file is readable.",
            datapack::error::ErrorCategory::Configuration => "These settings cannot be used. Review the operation settings.",
            datapack::error::ErrorCategory::Io => "The file could not be accessed. Check its location and permissions.",
            _ => "The operation could not finish. Review the details, then try again.",
        };
        Self {
            message: message.into(),
            code: error.code(),
            category: error.category().as_str(),
            context: error.to_string(),
        }
    }

    pub fn desktop(code: &'static str, message: impl Into<String>) -> Self {
        let message = message.into();
        Self {
            context: message.clone(),
            message,
            code,
            category: "desktop",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Analysis {
    pub format: String,
    pub delimiter: &'static str,
    pub columns: Option<usize>,
    pub sampled: bool,
    pub bytes_analyzed: u64,
    pub source_bytes: u64,
    pub records: u64,
    pub recommendation: &'static str,
    pub reason: String,
    pub estimated_reduction: f32,
    pub dictionary_mib: f32,
    pub warnings: Vec<String>,
    pub details: Vec<String>,
}

impl From<api::AnalysisReportV1> for Analysis {
    fn from(report: api::AnalysisReportV1) -> Self {
        let recommendation = match report.planner.selected_archive_mode {
            api::AnalysisArchiveModeV1::CsvColumnarDictionary => "Structured",
            api::AnalysisArchiveModeV1::RawZstd => "Standard",
            _ => "Automatic",
        };
        let reason = match report.planner.reason.code {
            "HIGH_REPETITION_DETECTED" => "Repeated values suggest structured compression may save space.",
            "PROJECTED_DICTIONARY_SAVINGS_BELOW_THRESHOLD" | "INSUFFICIENT_REPETITION_MAJORITY" => "Standard compression is recommended because structured savings appear limited.",
            "ANALYSIS_LIMITED_RAW_ZSTD_FALLBACK" => "Analysis reached a safety limit. Standard compression preserves the original bytes.",
            _ => "The engine selected the most appropriate supported method for this analysis.",
        };
        Self {
            format: match report.dataset.parser.format { "csv" => "CSV", "tsv" => "Tab-separated data", "psv" => "Pipe-separated data", "semicolon_delimited" => "Semicolon-separated data", _ => "Delimited data" }.into(),
            delimiter: match report.dataset.parser.delimiter { "," => "Comma", "\t" => "Tab", ";" => "Semicolon", "|" => "Pipe", _ => "Other" },
            columns: report.dataset.column_count,
            sampled: !matches!(report.sampling.scope, api::SamplingScopeV1::Full) || !matches!(report.sampling.completeness, api::CompletenessV1::Complete),
            bytes_analyzed: report.sampling.bytes_analyzed,
            source_bytes: report.sampling.source_size_bytes,
            records: report.sampling.records_analyzed,
            recommendation,
            reason: reason.into(),
            estimated_reduction: report.planner.estimated_savings_percent,
            dictionary_mib: report.planner.estimated_dictionary_memory_mib,
            warnings: report.diagnostics.iter().map(|d| match d.code {
                "SAMPLE_BYTE_LIMIT_REACHED" | "SAMPLE_RECORD_LIMIT_REACHED" => "Only part of the file was analyzed. The recommendation may differ for the remaining data.".into(),
                "CARDINALITY_LIMIT_REACHED" => "Some unique-value counts reached the analysis limit; estimates are incomplete.".into(),
                _ => d.message.to_owned(),
            }).collect(),
            details: std::iter::once(format!("{}: {}", report.planner.reason.code, report.planner.reason.message)).chain(report.diagnostics.iter().map(|d| format!("{}: {}", d.code, d.message))).collect(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct FileResult {
    pub output: PathBuf,
    pub input_bytes: u64,
    pub output_bytes: u64,
    pub version: u16,
    pub method: &'static str,
    pub integrity: &'static str,
    pub details: Vec<String>,
}

pub fn method(mode: api::ArchiveModeV1) -> &'static str {
    match mode {
        api::ArchiveModeV1::CsvColumnarDictionary => "Structured",
        api::ArchiveModeV1::RawZstd => "Standard",
        api::ArchiveModeV1::ChunkedRawZstd => "Chunked",
        _ => "Other",
    }
}

pub fn reduction(input: u64, output: u64) -> Option<f64> {
    (input != 0).then(|| (1.0 - output as f64 / input as f64) * 100.0)
}

pub fn ratio(input: u64, output: u64) -> Option<f64> {
    (input != 0 && output != 0).then(|| input as f64 / output as f64)
}

pub fn bytes(value: u64) -> String {
    for (divisor, unit) in [
        (1_u64 << 40, "TiB"),
        (1 << 30, "GiB"),
        (1 << 20, "MiB"),
        (1 << 10, "KiB"),
    ] {
        if value >= divisor {
            return format!("{:.2} {unit}", value as f64 / divisor as f64);
        }
    }
    format!("{value} bytes")
}

#[derive(Debug, Clone)]
pub struct Validation {
    pub valid: bool,
    pub version: Option<u16>,
    pub original_bytes: Option<u64>,
    pub chunks: Option<u64>,
    pub source_match: &'static str,
    pub integrity: &'static str,
    pub checks: Vec<(&'static str, &'static str)>,
    pub diagnostics: Vec<String>,
}

impl From<api::ValidationReportV1> for Validation {
    fn from(report: api::ValidationReportV1) -> Self {
        let source_match = match report.against.status {
            api::AgainstStatusV1::Matched => "Source matches exactly (SHA-256 and length)",
            api::AgainstStatusV1::Mismatched => "Source does not match",
            api::AgainstStatusV1::NotRequested => "No source supplied",
            _ => "Source comparison did not finish",
        };
        let integrity = if !report.valid {
            "Invalid archive or source mismatch"
        } else if report.against.status == api::AgainstStatusV1::Matched {
            "Exact-byte integrity verified against source"
        } else if report.checks.global_sha256 == api::CheckStatusV1::Passed {
            "Integrity verified using archive SHA-256"
        } else {
            "Structure valid; supply the original file to verify exact bytes"
        };
        let c = report.checks;
        Self {
            valid: report.valid,
            version: report.archive.version,
            original_bytes: report.archive.original_size_bytes,
            chunks: report.archive.chunk_count,
            source_match,
            integrity,
            checks: [
                ("Header", c.header),
                ("Metadata", c.metadata),
                ("Payload structure", c.payload_structure),
                ("Decompression", c.decompression),
                ("Restored length", c.restored_length),
                ("Chunk table", c.chunk_table),
                ("Per-chunk SHA-256", c.per_chunk_sha256),
                ("Global SHA-256", c.global_sha256),
                ("Trailing data", c.trailing_data),
            ]
            .into_iter()
            .map(|(label, status)| (label, check_status(status)))
            .collect(),
            diagnostics: report
                .diagnostics
                .iter()
                .map(|d| format!("{}: {}", d.code, d.message))
                .collect(),
        }
    }
}

fn check_status(status: api::CheckStatusV1) -> &'static str {
    match status {
        api::CheckStatusV1::Passed => "Passed",
        api::CheckStatusV1::Failed => "FAILED",
        api::CheckStatusV1::NotApplicable => "Not applicable",
        api::CheckStatusV1::NotAvailable => "Not available in this archive",
        _ => "Not completed",
    }
}

#[derive(Debug, Clone)]
pub enum State {
    Idle,
    Selecting,
    FileSelected,
    Analyzing,
    AnalysisReady,
    Compressing,
    Cancelling(Operation),
    CompressComplete(FileResult),
    Validating,
    ValidationComplete(Validation),
    Decompressing,
    DecompressComplete(FileResult),
    Cancelled(Operation),
    Failed(Problem),
}

/// Proposals never imply permission to overwrite. The engine checks again at commit.
pub fn proposed_output(input: &Path, kind: FileKind) -> PathBuf {
    let mut name = input.file_name().unwrap_or_default().to_os_string();
    name.push(match kind {
        FileKind::Data => ".dpack",
        FileKind::Archive => ".restored",
    });
    input.with_file_name(name)
}
