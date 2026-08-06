use std::fs::File;
use std::io::{BufRead, BufReader, Read};
use std::path::Path;
use std::time::Instant;

use super::accumulator::AnalysisAccumulator;
use super::model::{
    AnalysisCoverage, AnalysisLimitation, AnalysisParser, AnalysisStopReason, DatasetFacts,
    COMMA_ELIGIBILITY_RAW_FALLBACK_REASON, DELIMITED_FORMAT_RAW_FALLBACK_REASON,
    LIMITED_RAW_FALLBACK_REASON,
};
use crate::error::{DatapackError, Result};
use crate::formats::delimited::{split_legacy_physical_record, LegacySplitError};
use crate::planning::{
    ArchiveMode, ColumnProfile, CompressionPlan, PlannerFeaturesV1, PlannerPolicyV1,
};

const MIB: u64 = 1024 * 1024;
const DEFAULT_SAMPLE_MB: u64 = 64;
const MIN_SAMPLE_MB: u64 = 1;
const MAX_SAMPLE_MB: u64 = 2048;
const DEFAULT_MAX_SAMPLE_ROWS: u64 = 10_000;
pub(super) const READER_BUFFER_BYTES: usize = 256 * 1024;
const DEFAULT_MAX_HEADER_BYTES: usize = 1024 * 1024;
const DEFAULT_MAX_RECORD_BYTES: usize = 8 * 1024 * 1024;
const DEFAULT_MAX_COLUMNS: usize = 4_096;
const DEFAULT_MAX_CARDINALITY_ENTRIES: usize = 262_144;
const DEFAULT_MAX_ANALYSIS_MEMORY_BYTES: u64 = 64 * MIB;
const ACCOUNTED_COLUMN_BYTES: u64 = 512;
const ACCOUNTED_CARDINALITY_ENTRY_BYTES: u64 = 32;

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
            max_bytes: sample_mb.checked_mul(MIB).ok_or_else(|| {
                DatapackError::InvalidFormat("--sample-mb is too large".to_string())
            })?,
            max_rows: DEFAULT_MAX_SAMPLE_ROWS,
        })
    }
}

impl Default for SampleConfig {
    fn default() -> Self {
        Self {
            max_bytes: DEFAULT_SAMPLE_MB * MIB,
            max_rows: DEFAULT_MAX_SAMPLE_ROWS,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct AnalysisLimits {
    pub(crate) max_header_bytes: usize,
    pub(crate) max_record_bytes: usize,
    pub(crate) max_columns: usize,
    pub(crate) max_global_cardinality_entries: usize,
    pub(crate) max_analysis_memory_bytes: u64,
}

impl Default for AnalysisLimits {
    fn default() -> Self {
        Self {
            max_header_bytes: DEFAULT_MAX_HEADER_BYTES,
            max_record_bytes: DEFAULT_MAX_RECORD_BYTES,
            max_columns: DEFAULT_MAX_COLUMNS,
            max_global_cardinality_entries: DEFAULT_MAX_CARDINALITY_ENTRIES,
            max_analysis_memory_bytes: DEFAULT_MAX_ANALYSIS_MEMORY_BYTES,
        }
    }
}

impl AnalysisLimits {
    pub(super) fn cardinality_capacity(
        &self,
        retained_header_bytes: usize,
        column_count: usize,
    ) -> Option<usize> {
        let record_bytes = u64::try_from(self.max_record_bytes.max(self.max_header_bytes)).ok()?;
        let header_bytes = u64::try_from(retained_header_bytes).ok()?;
        let columns = u64::try_from(column_count).ok()?;
        let accounted_base = record_bytes
            .checked_add(header_bytes.checked_mul(2)?)?
            .checked_add(columns.checked_mul(ACCOUNTED_COLUMN_BYTES)?)?;
        let available = self.max_analysis_memory_bytes.checked_sub(accounted_base)?;
        let entries_from_memory = available / ACCOUNTED_CARDINALITY_ENTRY_BYTES;
        let entries_from_memory = usize::try_from(entries_from_memory).unwrap_or(usize::MAX);
        Some(self.max_global_cardinality_entries.min(entries_from_memory))
    }
}

#[derive(Debug, Clone)]
pub(crate) struct DatasetAnalysis {
    pub(crate) facts: DatasetFacts,
    pub(crate) columns: Vec<ColumnProfile>,
    pub(crate) plan: CompressionPlan,
    pub(crate) plan_disposition: PlanDisposition,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PlanDisposition {
    PlannerRecommendation,
    AnalysisLimitFallback,
    FormatFallback,
}

impl DatasetAnalysis {
    pub(crate) fn requires_raw_fallback(&self) -> bool {
        self.plan_disposition != PlanDisposition::PlannerRecommendation
    }

    pub(crate) fn apply_comma_eligibility_fallback(&mut self) {
        self.plan.archive_mode = ArchiveMode::RawZstd;
        self.plan.reason = COMMA_ELIGIBILITY_RAW_FALLBACK_REASON.to_string();
        self.plan_disposition = PlanDisposition::FormatFallback;
    }
}

#[derive(Debug, Clone)]
pub(crate) struct AnalysisEngine {
    config: SampleConfig,
    limits: AnalysisLimits,
    scope_size_bytes: Option<u64>,
}

impl AnalysisEngine {
    pub(crate) fn new(config: SampleConfig) -> Self {
        Self {
            config,
            limits: AnalysisLimits::default(),
            scope_size_bytes: None,
        }
    }

    #[cfg(test)]
    pub(crate) fn with_limits(mut self, limits: AnalysisLimits) -> Self {
        self.limits = limits;
        self
    }

    pub(crate) fn with_scope_size(mut self, scope_size_bytes: u64) -> Self {
        self.scope_size_bytes = Some(scope_size_bytes);
        self
    }

    pub(crate) fn analyze_path(&self, path: &Path) -> Result<DatasetAnalysis> {
        validate_csv_prefix(path)?;
        // Preserve the legacy timing boundary: prefix validation is excluded,
        // while metadata, scanning, fact adaptation and policy are included.
        let started = Instant::now();
        let source_size_bytes = std::fs::metadata(path)
            .map_err(|_| DatapackError::AnalyzeRead(path.display().to_string()))?
            .len();
        let scope_size_bytes = self
            .scope_size_bytes
            .unwrap_or(source_size_bytes)
            .min(source_size_bytes);
        if scope_size_bytes == 0 {
            return Err(DatapackError::InvalidCsv("empty file".to_string()));
        }
        let scan_limit_bytes = self.config.max_bytes.min(scope_size_bytes);
        let coverage_context = CoverageContext {
            source_size_bytes,
            scope_size_bytes,
            config: &self.config,
        };
        let file =
            File::open(path).map_err(|_| DatapackError::AnalyzeRead(path.display().to_string()))?;
        let mut reader = BufReader::with_capacity(READER_BUFFER_BYTES, file);
        let input_name = display_input_name(path);
        let mut record = Vec::new();
        let mut bytes_read = 0u64;

        let header = read_bounded_record(
            &mut reader,
            &mut record,
            scan_limit_bytes,
            self.limits.max_header_bytes,
        )?;
        bytes_read = bytes_read
            .checked_add(header.bytes_consumed())
            .ok_or_else(|| {
                DatapackError::InvalidFormat("analysis byte counter overflow".to_string())
            })?;
        match header {
            BoundedRecord::Eof => {
                return Err(DatapackError::InvalidCsv("empty file".to_string()));
            }
            BoundedRecord::Limit {
                kind: BoundKind::Sample,
                ..
            } => {
                return Ok(finish_analysis(
                    DatasetFacts {
                        input_name,
                        source_size_bytes,
                        parser: AnalysisParser::LegacyCsvPhysical,
                        observed_column_count: None,
                        coverage: coverage(
                            &coverage_context,
                            bytes_read,
                            0,
                            0,
                            AnalysisStopReason::ByteLimit,
                            None,
                        ),
                        columns: Vec::new(),
                        limitations: vec![AnalysisLimitation::IncompleteHeader],
                    },
                    started,
                ));
            }
            BoundedRecord::Limit {
                kind: BoundKind::Record,
                ..
            } => {
                return Ok(finish_analysis(
                    DatasetFacts {
                        input_name,
                        source_size_bytes,
                        parser: AnalysisParser::LegacyCsvPhysical,
                        observed_column_count: None,
                        coverage: coverage(
                            &coverage_context,
                            bytes_read,
                            0,
                            0,
                            AnalysisStopReason::HeaderByteLimit,
                            None,
                        ),
                        columns: Vec::new(),
                        limitations: vec![AnalysisLimitation::HeaderByteLimit],
                    },
                    started,
                ));
            }
            BoundedRecord::Complete { .. } => {}
        }

        let mut bytes_analyzed = bytes_read;
        let mut last_analyzed_had_newline = has_line_ending_bytes(&record);
        let header_line = record_as_str(&record)?;
        let mut header_fields = Vec::new();
        let observed_column_count = parse_csv_record_refs(
            trim_newline(header_line),
            &mut header_fields,
            self.limits.max_columns,
        )?;
        if observed_column_count < 2 {
            return Err(DatapackError::InvalidCsv(
                "expected at least two CSV columns".to_string(),
            ));
        }
        if observed_column_count > self.limits.max_columns {
            return Ok(finish_analysis(
                DatasetFacts {
                    input_name,
                    source_size_bytes,
                    parser: AnalysisParser::LegacyCsvPhysical,
                    observed_column_count: Some(observed_column_count),
                    coverage: coverage(
                        &coverage_context,
                        bytes_read,
                        bytes_analyzed,
                        0,
                        AnalysisStopReason::ColumnLimit,
                        complete_final_newline(
                            bytes_analyzed,
                            source_size_bytes,
                            last_analyzed_had_newline,
                        ),
                    ),
                    columns: Vec::new(),
                    limitations: vec![AnalysisLimitation::ColumnLimit],
                },
                started,
            ));
        }

        let Some(cardinality_capacity) = self
            .limits
            .cardinality_capacity(record.len(), observed_column_count)
        else {
            return Ok(finish_analysis(
                DatasetFacts {
                    input_name,
                    source_size_bytes,
                    parser: AnalysisParser::LegacyCsvPhysical,
                    observed_column_count: Some(observed_column_count),
                    coverage: coverage(
                        &coverage_context,
                        bytes_read,
                        bytes_analyzed,
                        0,
                        AnalysisStopReason::MemoryLimit,
                        complete_final_newline(
                            bytes_analyzed,
                            source_size_bytes,
                            last_analyzed_had_newline,
                        ),
                    ),
                    columns: Vec::new(),
                    limitations: vec![AnalysisLimitation::MemoryLimit],
                },
                started,
            ));
        };
        let headers = header_fields
            .iter()
            .map(|field| trim_quotes(field).to_string())
            .collect::<Vec<_>>();
        let mut accumulator = AnalysisAccumulator::new(&headers, cardinality_capacity);
        let mut sampled_rows = 0u64;
        let mut hard_stop_reason = None;
        let mut limitations = Vec::new();

        while sampled_rows < self.config.max_rows && bytes_read < scan_limit_bytes {
            let remaining = scan_limit_bytes.checked_sub(bytes_read).ok_or_else(|| {
                DatapackError::InvalidFormat("analysis sample counter underflow".to_string())
            })?;
            let next = read_bounded_record(
                &mut reader,
                &mut record,
                remaining,
                self.limits.max_record_bytes,
            )?;
            bytes_read = bytes_read
                .checked_add(next.bytes_consumed())
                .ok_or_else(|| {
                    DatapackError::InvalidFormat("analysis byte counter overflow".to_string())
                })?;
            match next {
                BoundedRecord::Eof => break,
                BoundedRecord::Limit {
                    kind: BoundKind::Sample,
                    ..
                } => break,
                BoundedRecord::Limit {
                    kind: BoundKind::Record,
                    ..
                } => {
                    hard_stop_reason = Some(AnalysisStopReason::RecordByteLimit);
                    limitations.push(AnalysisLimitation::RecordByteLimit);
                    break;
                }
                BoundedRecord::Complete { .. } => {}
            }

            bytes_analyzed = bytes_read;
            last_analyzed_had_newline = has_line_ending_bytes(&record);
            let line = record_as_str(&record)?;
            let trimmed = trim_newline(line);
            if trimmed.is_empty() {
                continue;
            }
            // Field slices borrow this bounded record only for the current
            // observation, so the record buffer can be reused next iteration.
            let mut fields = Vec::with_capacity(headers.len());
            let observed_fields =
                parse_csv_record_refs(trimmed, &mut fields, headers.len().saturating_add(1))?;
            if observed_fields != headers.len() {
                return Err(DatapackError::InvalidCsv(format!(
                    "row has {observed_fields} columns, expected {}",
                    headers.len()
                )));
            }
            sampled_rows = sampled_rows.checked_add(1).ok_or_else(|| {
                DatapackError::InvalidFormat("analysis record counter overflow".to_string())
            })?;
            accumulator.observe(&fields);
        }

        let (columns, cardinality_memory_limited) = accumulator.finish();
        if cardinality_memory_limited {
            limitations.push(AnalysisLimitation::CardinalityMemoryLimit);
        }
        let source_complete = bytes_analyzed >= source_size_bytes;
        let stop_reason = hard_stop_reason.unwrap_or({
            if source_complete {
                AnalysisStopReason::Complete
            } else if sampled_rows >= self.config.max_rows {
                AnalysisStopReason::RecordLimit
            } else {
                AnalysisStopReason::ByteLimit
            }
        });
        let facts = DatasetFacts {
            input_name,
            source_size_bytes,
            parser: AnalysisParser::LegacyCsvPhysical,
            observed_column_count: Some(observed_column_count),
            coverage: coverage(
                &coverage_context,
                bytes_read,
                bytes_analyzed,
                sampled_rows,
                stop_reason,
                source_complete.then_some(last_analyzed_had_newline),
            ),
            columns,
            limitations,
        };

        Ok(finish_analysis(facts, started))
    }
}

pub(crate) fn analyze_path(path: &Path, sample_mb: u64) -> Result<DatasetAnalysis> {
    AnalysisEngine::new(SampleConfig::from_sample_mb(sample_mb)?).analyze_path(path)
}

pub(crate) fn analyze_path_with_scope(
    path: &Path,
    sample_mb: u64,
    scope_size_bytes: u64,
) -> Result<DatasetAnalysis> {
    AnalysisEngine::new(SampleConfig::from_sample_mb(sample_mb)?)
        .with_scope_size(scope_size_bytes)
        .analyze_path(path)
}

pub(super) fn finish_analysis(facts: DatasetFacts, started: Instant) -> DatasetAnalysis {
    let features = PlannerFeaturesV1::from_facts(&facts);
    let columns = PlannerPolicyV1::profiles(&features);
    let mut plan = PlannerPolicyV1::plan(&columns, elapsed_millis(started));
    let plan_disposition = if !facts.limitations.is_empty() {
        plan.archive_mode = ArchiveMode::RawZstd;
        plan.reason = LIMITED_RAW_FALLBACK_REASON.to_string();
        PlanDisposition::AnalysisLimitFallback
    } else if matches!(facts.parser, AnalysisParser::CanonicalDelimited(_)) {
        plan.archive_mode = ArchiveMode::RawZstd;
        plan.reason = DELIMITED_FORMAT_RAW_FALLBACK_REASON.to_string();
        PlanDisposition::FormatFallback
    } else {
        PlanDisposition::PlannerRecommendation
    };
    // The old implementation sampled elapsed time twice. Preserve that
    // observable integer timing behavior for the legacy renderer.
    plan.planning_time_ms = elapsed_millis(started);
    DatasetAnalysis {
        facts,
        columns,
        plan,
        plan_disposition,
    }
}

struct CoverageContext<'a> {
    source_size_bytes: u64,
    scope_size_bytes: u64,
    config: &'a SampleConfig,
}

fn coverage(
    context: &CoverageContext<'_>,
    bytes_read: u64,
    bytes_analyzed: u64,
    sampled_records: u64,
    stop_reason: AnalysisStopReason,
    final_newline: Option<bool>,
) -> AnalysisCoverage {
    AnalysisCoverage {
        source_size_bytes: context.source_size_bytes,
        scope_size_bytes: context.scope_size_bytes,
        bytes_read,
        bytes_analyzed,
        sampled_records,
        max_bytes: context.config.max_bytes,
        max_records: context.config.max_rows,
        stop_reason,
        final_newline,
    }
}

fn complete_final_newline(
    bytes_analyzed: u64,
    source_size_bytes: u64,
    last_analyzed_had_newline: bool,
) -> Option<bool> {
    (bytes_analyzed >= source_size_bytes).then_some(last_analyzed_had_newline)
}

fn elapsed_millis(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BoundKind {
    Sample,
    Record,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BoundedRecord {
    Eof,
    Complete { bytes: u64 },
    Limit { bytes: u64, kind: BoundKind },
}

impl BoundedRecord {
    pub(super) const fn bytes_consumed(self) -> u64 {
        match self {
            Self::Eof => 0,
            Self::Complete { bytes } | Self::Limit { bytes, .. } => bytes,
        }
    }
}

pub(super) fn read_bounded_record<R: BufRead>(
    reader: &mut R,
    record: &mut Vec<u8>,
    remaining_sample_bytes: u64,
    max_record_bytes: usize,
) -> Result<BoundedRecord> {
    record.clear();
    let record_limit = u64::try_from(max_record_bytes).unwrap_or(u64::MAX);
    let (allowed, limit_kind) = if record_limit <= remaining_sample_bytes {
        (record_limit, BoundKind::Record)
    } else {
        (remaining_sample_bytes, BoundKind::Sample)
    };
    if allowed == 0 {
        return Ok(BoundedRecord::Limit {
            bytes: 0,
            kind: limit_kind,
        });
    }

    let mut bytes = 0u64;
    while bytes < allowed {
        let remaining = allowed.checked_sub(bytes).ok_or_else(|| {
            DatapackError::InvalidFormat("analysis record counter underflow".to_string())
        })?;
        let buffer = reader.fill_buf()?;
        if buffer.is_empty() {
            return if bytes == 0 {
                Ok(BoundedRecord::Eof)
            } else {
                Ok(BoundedRecord::Complete { bytes })
            };
        }
        let available = usize::try_from(remaining)
            .unwrap_or(usize::MAX)
            .min(buffer.len());
        let consumed = buffer[..available]
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(available, |position| position.saturating_add(1));
        record.try_reserve_exact(consumed).map_err(|_| {
            DatapackError::InvalidFormat(
                "analysis record buffer allocation failed within configured limit".to_string(),
            )
        })?;
        record.extend_from_slice(&buffer[..consumed]);
        reader.consume(consumed);
        bytes = bytes
            .checked_add(u64::try_from(consumed).unwrap_or(u64::MAX))
            .ok_or_else(|| {
                DatapackError::InvalidFormat("analysis record counter overflow".to_string())
            })?;
        if record.ends_with(b"\n") {
            return Ok(BoundedRecord::Complete { bytes });
        }
    }

    if reader.fill_buf()?.is_empty() {
        return Ok(BoundedRecord::Complete { bytes });
    }
    Ok(BoundedRecord::Limit {
        bytes,
        kind: limit_kind,
    })
}

fn validate_csv_prefix(path: &Path) -> Result<()> {
    let mut file =
        File::open(path).map_err(|_| DatapackError::AnalyzeRead(path.display().to_string()))?;
    let mut buffer = [0u8; 4096];
    let bytes_read = Read::read(&mut file, &mut buffer)?;
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

pub(super) fn display_input_name(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("input")
        .to_string()
}

pub(super) fn record_as_str(record: &[u8]) -> Result<&str> {
    std::str::from_utf8(record)
        .map_err(|_| DatapackError::InvalidCsv("record is not valid UTF-8".to_string()))
}

fn trim_newline(line: &str) -> &str {
    line.trim_end_matches('\n').trim_end_matches('\r')
}

pub(super) fn has_line_ending_bytes(record: &[u8]) -> bool {
    record.ends_with(b"\n") || record.ends_with(b"\r")
}

fn parse_csv_record_refs<'a>(
    line: &'a str,
    fields: &mut Vec<&'a str>,
    max_retained_fields: usize,
) -> Result<usize> {
    split_legacy_physical_record(line, fields, max_retained_fields).map_err(|error| match error {
        LegacySplitError::UnterminatedQuotedField => {
            DatapackError::InvalidCsv("unterminated quoted field".to_string())
        }
        LegacySplitError::FieldCounterOverflow => {
            DatapackError::InvalidFormat("CSV field counter overflow".to_string())
        }
        LegacySplitError::Allocation => DatapackError::InvalidFormat(
            "cannot reserve memory for bounded CSV field references".to_string(),
        ),
    })
}

fn trim_quotes(value: &str) -> &str {
    value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .unwrap_or(value)
}
