use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::time::Instant;

use super::accumulator::AnalysisAccumulator;
use super::model::{AnalysisCoverage, AnalysisStopReason, DatasetFacts};
use crate::error::{DatapackError, Result};
use crate::planning::{ColumnProfile, CompressionPlan, PlannerFeaturesV1, PlannerPolicyV1};

const DEFAULT_SAMPLE_MB: u64 = 64;
const MIN_SAMPLE_MB: u64 = 1;
const MAX_SAMPLE_MB: u64 = 2048;
const DEFAULT_MAX_SAMPLE_ROWS: u64 = 10_000;
const READER_BUFFER_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone)]
pub(crate) struct SampleConfig {
    pub(crate) max_bytes: u64,
    pub(crate) max_rows: u64,
}

impl SampleConfig {
    pub(crate) fn from_sample_mb(sample_mb: u64) -> Result<Self> {
        if !(MIN_SAMPLE_MB..=MAX_SAMPLE_MB).contains(&sample_mb) {
            return Err(DatapackError::InvalidFormat(format!(
                "--sample-mb must be between {MIN_SAMPLE_MB} and {MAX_SAMPLE_MB}"
            )));
        }
        Ok(Self {
            max_bytes: sample_mb * 1024 * 1024,
            max_rows: DEFAULT_MAX_SAMPLE_ROWS,
        })
    }
}

impl Default for SampleConfig {
    fn default() -> Self {
        Self {
            max_bytes: DEFAULT_SAMPLE_MB * 1024 * 1024,
            max_rows: DEFAULT_MAX_SAMPLE_ROWS,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct DatasetAnalysis {
    pub(crate) facts: DatasetFacts,
    pub(crate) columns: Vec<ColumnProfile>,
    pub(crate) plan: CompressionPlan,
}

#[derive(Debug, Clone)]
pub(crate) struct AnalysisEngine {
    config: SampleConfig,
}

impl AnalysisEngine {
    pub(crate) fn new(config: SampleConfig) -> Self {
        Self { config }
    }

    pub(crate) fn analyze_path(&self, path: &Path) -> Result<DatasetAnalysis> {
        validate_csv_prefix(path)?;
        // Preserve the legacy timing boundary: prefix validation is excluded,
        // while metadata, scanning, fact adaptation and policy are included.
        let started = Instant::now();
        let source_size_bytes = std::fs::metadata(path)
            .map_err(|_| DatapackError::AnalyzeRead(path.display().to_string()))?
            .len();
        let file =
            File::open(path).map_err(|_| DatapackError::AnalyzeRead(path.display().to_string()))?;
        let mut reader = BufReader::with_capacity(READER_BUFFER_BYTES, file);
        let mut line = String::new();
        let mut bytes_read = 0u64;
        let mut bytes_analyzed = 0u64;

        let header_bytes = reader.read_line(&mut line)?;
        if header_bytes == 0 {
            return Err(DatapackError::InvalidCsv("empty file".to_string()));
        }
        bytes_read += header_bytes as u64;
        bytes_analyzed += header_bytes as u64;
        let mut last_analyzed_had_newline = has_line_ending(&line);
        let mut header_fields = Vec::new();
        parse_csv_record_refs(trim_newline(&line), &mut header_fields)?;
        let headers: Vec<String> = header_fields
            .iter()
            .map(|field| trim_quotes(field).to_string())
            .collect();
        if headers.len() < 2 {
            return Err(DatapackError::InvalidCsv(
                "expected at least two CSV columns".to_string(),
            ));
        }

        let mut accumulator = AnalysisAccumulator::new(&headers);
        let mut sampled_rows = 0u64;

        while sampled_rows < self.config.max_rows && bytes_read < self.config.max_bytes {
            line.clear();
            let current_line_bytes = reader.read_line(&mut line)?;
            if current_line_bytes == 0 {
                break;
            }
            bytes_read += current_line_bytes as u64;
            if bytes_read > self.config.max_bytes && sampled_rows > 0 {
                break;
            }

            bytes_analyzed += current_line_bytes as u64;
            last_analyzed_had_newline = has_line_ending(&line);
            let trimmed = trim_newline(&line);
            if trimmed.is_empty() {
                continue;
            }
            let mut fields = Vec::with_capacity(headers.len());
            parse_csv_record_refs(trimmed, &mut fields)?;
            if fields.len() != headers.len() {
                return Err(DatapackError::InvalidCsv(format!(
                    "row has {} columns, expected {}",
                    fields.len(),
                    headers.len()
                )));
            }
            sampled_rows += 1;
            accumulator.observe(&fields);
        }

        let input_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("input")
            .to_string();
        let complete = bytes_analyzed >= source_size_bytes;
        let stop_reason = if complete {
            AnalysisStopReason::Complete
        } else if sampled_rows >= self.config.max_rows {
            AnalysisStopReason::RecordLimit
        } else {
            AnalysisStopReason::ByteLimit
        };
        let facts = DatasetFacts {
            input_name,
            source_size_bytes,
            coverage: AnalysisCoverage {
                source_size_bytes,
                bytes_read,
                bytes_analyzed,
                sampled_records: sampled_rows,
                max_bytes: self.config.max_bytes,
                max_records: self.config.max_rows,
                stop_reason,
                final_newline: complete.then_some(last_analyzed_had_newline),
            },
            columns: accumulator.finish(),
        };

        let features = PlannerFeaturesV1::from_facts(&facts);
        let columns = PlannerPolicyV1::profiles(&features);
        let mut plan = PlannerPolicyV1::plan(&columns, started.elapsed().as_millis() as u64);
        // The old implementation sampled elapsed time twice. Preserve that
        // observable integer timing behavior for the legacy renderer.
        plan.planning_time_ms = started.elapsed().as_millis() as u64;

        Ok(DatasetAnalysis {
            facts,
            columns,
            plan,
        })
    }
}

pub(crate) fn analyze_path(path: &Path, sample_mb: u64) -> Result<DatasetAnalysis> {
    AnalysisEngine::new(SampleConfig::from_sample_mb(sample_mb)?).analyze_path(path)
}

fn validate_csv_prefix(path: &Path) -> Result<()> {
    let mut file =
        File::open(path).map_err(|_| DatapackError::AnalyzeRead(path.display().to_string()))?;
    let mut buffer = [0u8; 4096];
    let bytes_read = std::io::Read::read(&mut file, &mut buffer)?;
    let prefix = &buffer[..bytes_read];
    if prefix.is_empty()
        || prefix.contains(&0)
        || std::str::from_utf8(prefix).is_err()
        || !prefix.contains(&b',')
    {
        return Err(DatapackError::InvalidCsv(
            "file is not valid CSV within first 4 KB".to_string(),
        ));
    }
    Ok(())
}

fn trim_newline(line: &str) -> &str {
    line.trim_end_matches('\n').trim_end_matches('\r')
}

fn has_line_ending(line: &str) -> bool {
    line.ends_with('\n') || line.ends_with('\r')
}

fn parse_csv_record_refs<'a>(line: &'a str, fields: &mut Vec<&'a str>) -> Result<()> {
    fields.clear();
    let mut chars = line.char_indices().peekable();
    let mut field_start = 0usize;
    let mut quoted = false;
    while let Some((index, ch)) = chars.next() {
        match ch {
            '"' if quoted && chars.peek().is_some_and(|(_, next)| *next == '"') => {
                chars.next();
            }
            '"' => quoted = !quoted,
            ',' if !quoted => {
                fields.push(&line[field_start..index]);
                field_start = index + ch.len_utf8();
            }
            _ => {}
        }
    }
    if quoted {
        return Err(DatapackError::InvalidCsv(
            "unterminated quoted field".to_string(),
        ));
    }
    fields.push(&line[field_start..]);
    Ok(())
}

fn trim_quotes(value: &str) -> &str {
    value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .unwrap_or(value)
}
