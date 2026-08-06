use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::time::Instant;

use super::accumulator::AnalysisAccumulator;
use super::engine::{
    display_input_name, finish_analysis, has_line_ending_bytes, read_bounded_record, record_as_str,
    AnalysisEngine, AnalysisLimits, BoundedRecord, DatasetAnalysis, SampleConfig,
    READER_BUFFER_BYTES,
};
use super::model::{
    AnalysisCoverage, AnalysisLimitation, AnalysisParser, AnalysisStopReason, DatasetFacts,
    DelimitedFormat,
};
use crate::error::{DatapackError, Result};
use crate::formats::delimited::{
    detect_dialect, parse_document, DelimitedDialect, DetectionOutcome, LimitKind,
    LogicalRecordFramer, NewlinePolicy, QuoteMode, ScanError, ScanLimits,
};

const MAX_DETECTION_PHYSICAL_RECORDS: usize = 32;

enum PathDialectDetection {
    Outcome(DetectionOutcome),
    LegacyAuthority,
    Limited,
}

pub(crate) fn analyze_cli_path(path: &Path, sample_mb: u64) -> Result<DatasetAnalysis> {
    let config = SampleConfig::from_sample_mb(sample_mb)?;
    let limits = AnalysisLimits::default();
    match detect_path_dialect(path, &config, &limits, None)? {
        PathDialectDetection::Outcome(DetectionOutcome::Detected(dialect)) => {
            let format = DelimitedFormat::from_byte(dialect.delimiter).ok_or_else(|| {
                DatapackError::InvalidFormat(
                    "canonical detector returned an unsupported delimiter".to_string(),
                )
            })?;
            if format == DelimitedFormat::Comma {
                AnalysisEngine::new(config).analyze_path(path)
            } else {
                DelimitedAnalysisEngine::new(config, limits, format).analyze_path(path)
            }
        }
        PathDialectDetection::Outcome(DetectionOutcome::Ambiguous(candidates)) => {
            Err(DatapackError::InvalidCsv(format!(
                "ambiguous delimiter; candidates: {}",
                candidate_labels(&candidates)
            )))
        }
        PathDialectDetection::Outcome(DetectionOutcome::Undetected) => {
            analyze_undetected_path(path, config)
        }
        PathDialectDetection::LegacyAuthority => AnalysisEngine::new(config).analyze_path(path),
        PathDialectDetection::Limited => Err(DatapackError::InvalidCsv(
            "delimited dialect detection could not safely complete within configured limits"
                .to_string(),
        )),
    }
}

fn analyze_undetected_path(path: &Path, config: SampleConfig) -> Result<DatasetAnalysis> {
    match AnalysisEngine::new(config).analyze_path(path) {
        Err(DatapackError::InvalidCsv(reason))
            if reason == "file is not valid CSV within first 4 KB" =>
        {
            Err(DatapackError::InvalidCsv(
                "no supported structured delimiter was detected; input is unsupported or unstructured"
                    .to_string(),
            ))
        }
        result => result,
    }
}

pub(crate) fn comma_structured_compression_eligible_path(
    path: &Path,
    sample_mb: u64,
    scope_size_bytes: Option<u64>,
) -> Result<bool> {
    let config = SampleConfig::from_sample_mb(sample_mb)?;
    let limits = AnalysisLimits::default();
    Ok(matches!(
        detect_path_dialect(path, &config, &limits, scope_size_bytes)?,
        PathDialectDetection::Outcome(DetectionOutcome::Detected(dialect))
            if dialect.delimiter == b','
    ))
}

pub(crate) fn comma_structured_compression_eligible_bytes(bytes: &[u8]) -> bool {
    if bytes.is_empty() {
        return false;
    }
    let limits = AnalysisLimits::default();
    let scan_limits = ScanLimits {
        max_input_bytes: bytes.len(),
        max_logical_record_bytes: limits.max_record_bytes,
        max_records: usize::MAX,
        max_fields_per_record: limits.max_columns,
        max_total_fields: usize::MAX,
    };
    matches!(
        detect_dialect(bytes, scan_limits),
        Ok(DetectionOutcome::Detected(dialect)) if dialect.delimiter == b','
    )
}

fn detect_path_dialect(
    path: &Path,
    config: &SampleConfig,
    limits: &AnalysisLimits,
    scope_size_bytes: Option<u64>,
) -> Result<PathDialectDetection> {
    let source_size_bytes = std::fs::metadata(path)
        .map_err(|_| DatapackError::AnalyzeRead(path.display().to_string()))?
        .len();
    let scope_size_bytes = scope_size_bytes
        .unwrap_or(source_size_bytes)
        .min(source_size_bytes);
    let detection_byte_limit = config
        .max_bytes
        .min(scope_size_bytes)
        .min(u64::try_from(limits.max_header_bytes).unwrap_or(u64::MAX));
    if detection_byte_limit == 0 {
        return Ok(PathDialectDetection::LegacyAuthority);
    }

    let file =
        File::open(path).map_err(|_| DatapackError::AnalyzeRead(path.display().to_string()))?;
    let mut reader = BufReader::with_capacity(READER_BUFFER_BYTES, file);
    let mut prefix = Vec::new();
    let mut record = Vec::new();
    let mut bytes_read = 0u64;
    let mut physical_records = 0usize;
    let mut last_success = None;
    let mut latest_complete_prefix_failed = false;

    while bytes_read < detection_byte_limit && physical_records < MAX_DETECTION_PHYSICAL_RECORDS {
        let remaining = detection_byte_limit
            .checked_sub(bytes_read)
            .ok_or_else(|| {
                DatapackError::InvalidFormat("dialect detection byte counter underflow".to_string())
            })?;
        let next = read_bounded_record(
            &mut reader,
            &mut record,
            remaining,
            usize::try_from(remaining).unwrap_or(usize::MAX),
        )?;
        bytes_read = checked_add_bytes(bytes_read, next.bytes_consumed())?;
        match next {
            BoundedRecord::Eof => {
                return Ok(completed_detection(
                    last_success,
                    latest_complete_prefix_failed,
                ));
            }
            BoundedRecord::Limit { .. } => {
                if let Some(outcome) = last_success {
                    return Ok(PathDialectDetection::Outcome(outcome));
                }
                return match detect_prefix_dialect(&record) {
                    Ok(DetectionOutcome::Detected(dialect)) => Ok(PathDialectDetection::Outcome(
                        DetectionOutcome::Detected(dialect),
                    )),
                    Ok(DetectionOutcome::Ambiguous(_) | DetectionOutcome::Undetected) => {
                        Ok(PathDialectDetection::Limited)
                    }
                    Err(error) if detection_error_is_resource_failure(&error) => {
                        Err(map_scan_error(error))
                    }
                    Err(_) => Ok(PathDialectDetection::Limited),
                };
            }
            BoundedRecord::Complete { .. } => {}
        }

        prefix.try_reserve_exact(record.len()).map_err(|_| {
            DatapackError::InvalidFormat(
                "cannot reserve memory for bounded dialect detection".to_string(),
            )
        })?;
        prefix.extend_from_slice(&record);
        physical_records = physical_records.checked_add(1).ok_or_else(|| {
            DatapackError::InvalidFormat("dialect detection record counter overflow".to_string())
        })?;
        match detect_prefix_dialect(&prefix) {
            Ok(outcome) => {
                last_success = Some(outcome);
                latest_complete_prefix_failed = false;
            }
            Err(error) if detection_error_is_resource_failure(&error) => {
                return Err(map_scan_error(error));
            }
            Err(_) => latest_complete_prefix_failed = true,
        }
    }

    let actual_eof = bytes_read >= source_size_bytes && scope_size_bytes >= source_size_bytes;
    if actual_eof {
        Ok(completed_detection(
            last_success,
            latest_complete_prefix_failed,
        ))
    } else {
        Ok(last_success.map_or(PathDialectDetection::Limited, PathDialectDetection::Outcome))
    }
}

fn completed_detection(
    last_success: Option<DetectionOutcome>,
    latest_complete_prefix_failed: bool,
) -> PathDialectDetection {
    if latest_complete_prefix_failed {
        if let Some(DetectionOutcome::Detected(dialect)) = last_success {
            return PathDialectDetection::Outcome(DetectionOutcome::Detected(dialect));
        }
        return PathDialectDetection::LegacyAuthority;
    }
    last_success.map_or(
        PathDialectDetection::LegacyAuthority,
        PathDialectDetection::Outcome,
    )
}

fn detect_prefix_dialect(prefix: &[u8]) -> std::result::Result<DetectionOutcome, ScanError> {
    let bounded_items = prefix.len().saturating_add(1);
    let detector_limits = ScanLimits {
        max_input_bytes: prefix.len(),
        max_logical_record_bytes: prefix.len(),
        max_records: bounded_items,
        max_fields_per_record: bounded_items,
        max_total_fields: bounded_items,
    };
    detect_dialect(prefix, detector_limits)
}

fn detection_error_is_resource_failure(error: &ScanError) -> bool {
    matches!(
        error,
        ScanError::Limit { .. } | ScanError::CounterOverflow(_) | ScanError::Allocation(_)
    )
}

fn candidate_labels(candidates: &[u8]) -> String {
    candidates
        .iter()
        .map(|delimiter| match delimiter {
            b',' => "comma (,)",
            b';' => "semicolon (;)",
            b'\t' => "tab (\\t)",
            b'|' => "pipe (|)",
            _ => "unsupported",
        })
        .collect::<Vec<_>>()
        .join(", ")
}

struct DelimitedAnalysisEngine {
    config: SampleConfig,
    limits: AnalysisLimits,
    format: DelimitedFormat,
}

impl DelimitedAnalysisEngine {
    fn new(config: SampleConfig, limits: AnalysisLimits, format: DelimitedFormat) -> Self {
        Self {
            config,
            limits,
            format,
        }
    }

    fn analyze_path(&self, path: &Path) -> Result<DatasetAnalysis> {
        let started = Instant::now();
        let source_size_bytes = std::fs::metadata(path)
            .map_err(|_| DatapackError::AnalyzeRead(path.display().to_string()))?
            .len();
        if source_size_bytes == 0 {
            return Err(DatapackError::InvalidCsv("empty file".to_string()));
        }
        let scan_limit_bytes = self.config.max_bytes.min(source_size_bytes);
        let input_name = display_input_name(path);
        let parser = AnalysisParser::CanonicalDelimited(self.format);
        let dialect = DelimitedDialect::new(
            self.format.delimiter(),
            QuoteMode::Dcsv01Compatible,
            NewlinePolicy::Observe,
        );
        let file =
            File::open(path).map_err(|_| DatapackError::AnalyzeRead(path.display().to_string()))?;
        let mut reader = BufReader::with_capacity(READER_BUFFER_BYTES, file);
        let mut record = Vec::new();
        let mut bytes_read = 0u64;

        let header = read_bounded_logical_record(
            &mut reader,
            &mut record,
            dialect,
            scan_limit_bytes,
            self.limits.max_header_bytes,
        )?;
        bytes_read = checked_add_bytes(bytes_read, header.bytes_consumed())?;
        match header {
            LogicalBoundedRecord::Eof => {
                return Err(DatapackError::InvalidCsv("empty file".to_string()));
            }
            LogicalBoundedRecord::Limit {
                kind: LogicalBoundKind::Sample,
                ..
            } => {
                return Ok(finish_analysis(
                    DatasetFacts {
                        input_name,
                        source_size_bytes,
                        parser,
                        observed_column_count: None,
                        coverage: self.coverage(
                            source_size_bytes,
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
            LogicalBoundedRecord::Limit {
                kind: LogicalBoundKind::Record,
                ..
            } => {
                return Ok(finish_analysis(
                    DatasetFacts {
                        input_name,
                        source_size_bytes,
                        parser,
                        observed_column_count: None,
                        coverage: self.coverage(
                            source_size_bytes,
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
            LogicalBoundedRecord::Complete { .. } => {}
        }

        let mut bytes_analyzed = bytes_read;
        let mut last_analyzed_had_newline = has_line_ending_bytes(&record);
        let header_fields = match parse_record_fields(&record, dialect, self.limits.max_columns)? {
            ParsedRecord::Fields(fields) => fields,
            ParsedRecord::FieldLimit => {
                return Ok(finish_analysis(
                    DatasetFacts {
                        input_name,
                        source_size_bytes,
                        parser,
                        observed_column_count: None,
                        coverage: self.coverage(
                            source_size_bytes,
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
        };
        let observed_column_count = header_fields.len();
        if observed_column_count < 2 {
            return Err(DatapackError::InvalidCsv(
                "expected at least two delimited columns".to_string(),
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
                    parser,
                    observed_column_count: Some(observed_column_count),
                    coverage: self.coverage(
                        source_size_bytes,
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
        let headers = owned_headers(&header_fields)?;
        let mut accumulator = AnalysisAccumulator::new(&headers, cardinality_capacity);
        let mut sampled_rows = 0u64;
        let mut hard_stop_reason = None;
        let mut limitations = Vec::new();

        while sampled_rows < self.config.max_rows && bytes_read < scan_limit_bytes {
            let remaining = scan_limit_bytes.checked_sub(bytes_read).ok_or_else(|| {
                DatapackError::InvalidFormat("analysis sample counter underflow".to_string())
            })?;
            let next = read_bounded_logical_record(
                &mut reader,
                &mut record,
                dialect,
                remaining,
                self.limits.max_record_bytes,
            )?;
            bytes_read = checked_add_bytes(bytes_read, next.bytes_consumed())?;
            match next {
                LogicalBoundedRecord::Eof => break,
                LogicalBoundedRecord::Limit {
                    kind: LogicalBoundKind::Sample,
                    ..
                } => break,
                LogicalBoundedRecord::Limit {
                    kind: LogicalBoundKind::Record,
                    ..
                } => {
                    hard_stop_reason = Some(AnalysisStopReason::RecordByteLimit);
                    limitations.push(AnalysisLimitation::RecordByteLimit);
                    break;
                }
                LogicalBoundedRecord::Complete { .. } => {}
            }

            bytes_analyzed = bytes_read;
            last_analyzed_had_newline = has_line_ending_bytes(&record);
            if record_content_is_empty(&record) {
                continue;
            }
            let fields =
                match parse_record_fields(&record, dialect, headers.len().saturating_add(1))? {
                    ParsedRecord::Fields(fields) => fields,
                    ParsedRecord::FieldLimit => {
                        return Err(DatapackError::InvalidCsv(format!(
                            "row has more than {} columns",
                            headers.len()
                        )));
                    }
                };
            if fields.len() != headers.len() {
                return Err(DatapackError::InvalidCsv(format!(
                    "row has {} columns, expected {}",
                    fields.len(),
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
            parser,
            observed_column_count: Some(observed_column_count),
            coverage: self.coverage(
                source_size_bytes,
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

    fn coverage(
        &self,
        source_size_bytes: u64,
        bytes_read: u64,
        bytes_analyzed: u64,
        sampled_records: u64,
        stop_reason: AnalysisStopReason,
        final_newline: Option<bool>,
    ) -> AnalysisCoverage {
        AnalysisCoverage {
            source_size_bytes,
            scope_size_bytes: source_size_bytes,
            bytes_read,
            bytes_analyzed,
            sampled_records,
            max_bytes: self.config.max_bytes,
            max_records: self.config.max_rows,
            stop_reason,
            final_newline,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LogicalBoundKind {
    Sample,
    Record,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LogicalBoundedRecord {
    Eof,
    Complete { bytes: u64 },
    Limit { bytes: u64, kind: LogicalBoundKind },
}

impl LogicalBoundedRecord {
    const fn bytes_consumed(self) -> u64 {
        match self {
            Self::Eof => 0,
            Self::Complete { bytes } | Self::Limit { bytes, .. } => bytes,
        }
    }
}

fn read_bounded_logical_record<R: BufRead>(
    reader: &mut R,
    record: &mut Vec<u8>,
    dialect: DelimitedDialect,
    remaining_sample_bytes: u64,
    max_record_bytes: usize,
) -> Result<LogicalBoundedRecord> {
    record.clear();
    let record_limit = u64::try_from(max_record_bytes).unwrap_or(u64::MAX);
    let (allowed, limit_kind) = if record_limit <= remaining_sample_bytes {
        (record_limit, LogicalBoundKind::Record)
    } else {
        (remaining_sample_bytes, LogicalBoundKind::Sample)
    };
    if allowed == 0 {
        return Ok(LogicalBoundedRecord::Limit {
            bytes: 0,
            kind: limit_kind,
        });
    }

    let mut framer = LogicalRecordFramer::new(dialect);
    let mut bytes = 0u64;
    while bytes < allowed {
        let remaining = allowed.checked_sub(bytes).ok_or_else(|| {
            DatapackError::InvalidFormat("analysis record counter underflow".to_string())
        })?;
        let buffer = reader.fill_buf()?;
        if buffer.is_empty() {
            if bytes == 0 {
                return Ok(LogicalBoundedRecord::Eof);
            }
            framer.finish().map_err(map_scan_error)?;
            return Ok(LogicalBoundedRecord::Complete { bytes });
        }
        let available = usize::try_from(remaining)
            .unwrap_or(usize::MAX)
            .min(buffer.len());
        let mut consumed = 0usize;
        let mut complete = false;
        for &byte in &buffer[..available] {
            consumed = consumed.checked_add(1).ok_or_else(|| {
                DatapackError::InvalidFormat("analysis buffer counter overflow".to_string())
            })?;
            if framer.push(byte).map_err(map_scan_error)? {
                complete = true;
                break;
            }
        }
        record.try_reserve_exact(consumed).map_err(|_| {
            DatapackError::InvalidFormat(
                "analysis record buffer allocation failed within configured limit".to_string(),
            )
        })?;
        record.extend_from_slice(&buffer[..consumed]);
        reader.consume(consumed);
        bytes = checked_add_bytes(bytes, u64::try_from(consumed).unwrap_or(u64::MAX))?;
        if complete {
            return Ok(LogicalBoundedRecord::Complete { bytes });
        }
    }

    if reader.fill_buf()?.is_empty() {
        framer.finish().map_err(map_scan_error)?;
        return Ok(LogicalBoundedRecord::Complete { bytes });
    }
    Ok(LogicalBoundedRecord::Limit {
        bytes,
        kind: limit_kind,
    })
}

enum ParsedRecord<'a> {
    Fields(Vec<&'a str>),
    FieldLimit,
}

fn parse_record_fields(
    record: &[u8],
    dialect: DelimitedDialect,
    max_fields: usize,
) -> Result<ParsedRecord<'_>> {
    if record.contains(&0) {
        return Err(DatapackError::InvalidCsv(
            "record contains a NUL byte".to_string(),
        ));
    }
    record_as_str(record)?;
    let document = match parse_document(
        record,
        dialect,
        ScanLimits {
            max_input_bytes: record.len(),
            max_logical_record_bytes: record.len(),
            max_records: 1,
            max_fields_per_record: max_fields,
            max_total_fields: max_fields,
        },
    ) {
        Ok(document) => document,
        Err(ScanError::Limit {
            kind: LimitKind::FieldsPerRecord | LimitKind::TotalFields,
            ..
        }) => return Ok(ParsedRecord::FieldLimit),
        Err(error) => return Err(map_scan_error(error)),
    };
    let mut records = document.into_records();
    if records.len() != 1 {
        return Err(DatapackError::InvalidFormat(format!(
            "logical-record parser produced {} records for one framed record",
            records.len()
        )));
    }
    let parsed = records.pop().ok_or_else(|| {
        DatapackError::InvalidFormat("logical-record parser produced no record".to_string())
    })?;
    let mut fields = Vec::new();
    fields
        .try_reserve_exact(parsed.fields().len())
        .map_err(|_| {
            DatapackError::InvalidFormat(
                "cannot reserve memory for bounded delimited field references".to_string(),
            )
        })?;
    for range in parsed.fields() {
        let bytes = record.get(range.clone()).ok_or_else(|| {
            DatapackError::InvalidFormat("delimited field range is outside its record".to_string())
        })?;
        let value = std::str::from_utf8(bytes)
            .map_err(|_| DatapackError::InvalidCsv("record is not valid UTF-8".to_string()))?;
        fields.push(value);
    }
    Ok(ParsedRecord::Fields(fields))
}

fn owned_headers(fields: &[&str]) -> Result<Vec<String>> {
    let mut headers = Vec::new();
    headers.try_reserve_exact(fields.len()).map_err(|_| {
        DatapackError::InvalidFormat(
            "cannot reserve memory for bounded delimited headers".to_string(),
        )
    })?;
    for field in fields {
        let value = trim_quotes(field);
        let mut header = String::new();
        header.try_reserve_exact(value.len()).map_err(|_| {
            DatapackError::InvalidFormat(
                "cannot reserve memory for a bounded delimited header".to_string(),
            )
        })?;
        header.push_str(value);
        headers.push(header);
    }
    Ok(headers)
}

fn trim_quotes(value: &str) -> &str {
    value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .unwrap_or(value)
}

fn record_content_is_empty(record: &[u8]) -> bool {
    let without_lf = record.strip_suffix(b"\n").unwrap_or(record);
    without_lf
        .strip_suffix(b"\r")
        .unwrap_or(without_lf)
        .is_empty()
}

fn checked_add_bytes(current: u64, additional: u64) -> Result<u64> {
    current
        .checked_add(additional)
        .ok_or_else(|| DatapackError::InvalidFormat("analysis byte counter overflow".to_string()))
}

fn complete_final_newline(
    bytes_analyzed: u64,
    source_size_bytes: u64,
    last_analyzed_had_newline: bool,
) -> Option<bool> {
    (bytes_analyzed >= source_size_bytes).then_some(last_analyzed_had_newline)
}

fn map_scan_error(error: ScanError) -> DatapackError {
    match error {
        ScanError::QuoteNotAllowed => {
            DatapackError::InvalidCsv("quotes are not allowed".to_string())
        }
        ScanError::UnterminatedQuotedField => {
            DatapackError::InvalidCsv("unterminated quoted field".to_string())
        }
        ScanError::BareCarriageReturn => {
            DatapackError::InvalidCsv("unsupported bare CR newline in delimited input".to_string())
        }
        ScanError::MixedNewlines => {
            DatapackError::InvalidCsv("mixed delimited newline styles".to_string())
        }
        other => DatapackError::InvalidFormat(other.to_string()),
    }
}
