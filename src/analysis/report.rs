//! Versioned machine-readable analysis report DTOs.
//!
//! This module converts internal facts and policy results into a stable JSON
//! boundary without adding serialization concerns to either model.

use serde::Serialize;

use super::engine::{DatasetAnalysis, PlanDisposition};
use super::model::{
    AnalysisLimitation, AnalysisParser, AnalysisStopReason, CardinalityEstimate, ColumnFacts,
    DelimitedFormat, DELIMITED_FORMAT_RAW_FALLBACK_REASON, LIMITED_RAW_FALLBACK_REASON,
};
use crate::error::{DatapackError, Result};
use crate::planning::{ArchiveMode, ColumnProfile, ColumnStrategy};

const ARCHIVE_CANDIDATES: [ArchiveModeV1; 2] =
    [ArchiveModeV1::CsvColumnarDictionary, ArchiveModeV1::RawZstd];
const COLUMN_CANDIDATES: [ColumnStrategyV1; 4] = [
    ColumnStrategyV1::Dictionary,
    ColumnStrategyV1::Plain,
    ColumnStrategyV1::DeltaCandidate,
    ColumnStrategyV1::Raw,
];

#[derive(Debug, Serialize)]
pub(crate) struct AnalysisReportV1<'a> {
    schema_version: u32,
    report_type: &'static str,
    dataset: DatasetReportV1<'a>,
    sampling: SamplingReportV1,
    planner: PlannerReportV1<'a>,
    diagnostics: Vec<DiagnosticV1>,
}

#[derive(Debug, Serialize)]
struct DatasetReportV1<'a> {
    source_size_bytes: u64,
    column_count: Option<usize>,
    parser: ParserReportV1,
    columns: Vec<ColumnReportV1<'a>>,
}

#[derive(Debug, Serialize)]
struct ParserReportV1 {
    format: &'static str,
    delimiter: &'static str,
    record_model: &'static str,
    header_mode: &'static str,
}

#[derive(Debug, Serialize)]
struct ColumnReportV1<'a> {
    index: usize,
    name_status: ColumnNameStatusReportV1,
    observed_values: u64,
    empty_values: u64,
    numeric_values: u64,
    value_length_bytes: ValueLengthReportV1,
    cardinality: EstimateU64ReportV1,
    repetition_rate: EstimateF64ReportV1,
    planner: ColumnPlannerReportV1<'a>,
}

#[derive(Debug, Serialize)]
struct ColumnNameStatusReportV1 {
    is_empty: bool,
    duplicate_of: Option<usize>,
}

#[derive(Debug, Serialize)]
struct ValueLengthReportV1 {
    minimum: Option<u64>,
    maximum: Option<u64>,
    mean: Option<f64>,
    total: u64,
}

#[derive(Debug, Serialize)]
struct EstimateU64ReportV1 {
    kind: EstimateKindV1,
    value: Option<u64>,
}

#[derive(Debug, Serialize)]
struct EstimateF64ReportV1 {
    kind: EstimateKindV1,
    value: Option<f64>,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
enum EstimateKindV1 {
    Exact,
    AtLeast,
    #[allow(dead_code)] // Reserved by the V1 contract for future bounded analysis.
    Unknown,
}

#[derive(Debug, Serialize)]
struct SamplingReportV1 {
    scope: SamplingScopeV1,
    completeness: CompletenessV1,
    limited: bool,
    limit_reached: Option<AnalysisLimitV1>,
    source_size_bytes: u64,
    bytes_read: u64,
    bytes_analyzed: u64,
    records_analyzed: u64,
    configured_max_bytes: u64,
    configured_max_records: u64,
    final_newline: Option<bool>,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
enum SamplingScopeV1 {
    Full,
    Sampled,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
enum CompletenessV1 {
    Complete,
    Partial,
}

#[derive(Debug, Clone, Copy, Serialize)]
enum AnalysisLimitV1 {
    #[serde(rename = "byte_limit")]
    SampleBytes,
    #[serde(rename = "record_limit")]
    SampleRecords,
    #[serde(rename = "header_byte_limit")]
    HeaderBytes,
    #[serde(rename = "record_byte_limit")]
    RecordBytes,
    #[serde(rename = "column_limit")]
    Columns,
    #[serde(rename = "memory_limit")]
    Memory,
    #[serde(rename = "cardinality_memory_limit")]
    CardinalityMemory,
}

#[derive(Debug, Serialize)]
struct PlannerReportV1<'a> {
    policy: PolicyReportV1,
    selection_scope: &'static str,
    candidate_archive_modes: [ArchiveModeV1; 2],
    candidate_column_strategies: [ColumnStrategyV1; 4],
    selected_archive_mode: ArchiveModeV1,
    reason: ReasonReportV1<'a>,
    estimated_savings_percent: f32,
    estimated_dictionary_memory_mib: f32,
}

#[derive(Debug, Serialize)]
struct PolicyReportV1 {
    name: &'static str,
    version: u32,
}

#[derive(Debug, Serialize)]
struct ColumnPlannerReportV1<'a> {
    selected_strategy: ColumnStrategyV1,
    reason: ReasonReportV1<'a>,
    estimated_dictionary_size_kib: u64,
    estimated_encoded_size_bytes: u64,
    estimated_raw_size_bytes: u64,
}

#[derive(Debug, Serialize)]
struct ReasonReportV1<'a> {
    code: &'static str,
    message: &'a str,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
enum ArchiveModeV1 {
    CsvColumnarDictionary,
    RawZstd,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
enum ColumnStrategyV1 {
    Dictionary,
    Plain,
    DeltaCandidate,
    Raw,
}

#[derive(Debug, Serialize)]
struct DiagnosticV1 {
    code: &'static str,
    severity: DiagnosticSeverityV1,
    message: &'static str,
    column_index: Option<usize>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
enum DiagnosticSeverityV1 {
    Warning,
}

pub(crate) fn build_report_v1(analysis: &DatasetAnalysis) -> Result<AnalysisReportV1<'_>> {
    if analysis.facts.columns.len() != analysis.columns.len()
        || analysis
            .facts
            .columns
            .iter()
            .zip(&analysis.columns)
            .any(|(facts, profile)| facts.index != profile.column_index)
    {
        return Err(DatapackError::InvalidFormat(
            "internal analysis report column mismatch".to_string(),
        ));
    }

    let columns = analysis
        .facts
        .columns
        .iter()
        .zip(&analysis.columns)
        .map(|(facts, profile)| column_report(facts, profile))
        .collect::<Result<Vec<_>>>()?;
    let diagnostics = diagnostics(analysis);
    let coverage = &analysis.facts.coverage;
    let hard_limited = !analysis.facts.limitations.is_empty();
    let (scope, completeness, limited, limit_reached) = match coverage.stop_reason {
        AnalysisStopReason::Complete => {
            if hard_limited {
                (
                    SamplingScopeV1::Full,
                    CompletenessV1::Partial,
                    true,
                    primary_hard_limit(&analysis.facts.limitations),
                )
            } else {
                (SamplingScopeV1::Full, CompletenessV1::Complete, false, None)
            }
        }
        AnalysisStopReason::ByteLimit => (
            SamplingScopeV1::Sampled,
            CompletenessV1::Partial,
            true,
            Some(AnalysisLimitV1::SampleBytes),
        ),
        AnalysisStopReason::RecordLimit => (
            SamplingScopeV1::Sampled,
            CompletenessV1::Partial,
            true,
            Some(AnalysisLimitV1::SampleRecords),
        ),
        AnalysisStopReason::HeaderByteLimit => (
            SamplingScopeV1::Sampled,
            CompletenessV1::Partial,
            true,
            Some(AnalysisLimitV1::HeaderBytes),
        ),
        AnalysisStopReason::RecordByteLimit => (
            SamplingScopeV1::Sampled,
            CompletenessV1::Partial,
            true,
            Some(AnalysisLimitV1::RecordBytes),
        ),
        AnalysisStopReason::ColumnLimit => (
            sampling_scope(coverage),
            CompletenessV1::Partial,
            true,
            Some(AnalysisLimitV1::Columns),
        ),
        AnalysisStopReason::MemoryLimit => (
            sampling_scope(coverage),
            CompletenessV1::Partial,
            true,
            Some(AnalysisLimitV1::Memory),
        ),
    };
    let plan = &analysis.plan;

    Ok(AnalysisReportV1 {
        schema_version: 1,
        report_type: "analysis",
        dataset: DatasetReportV1 {
            source_size_bytes: analysis.facts.source_size_bytes,
            column_count: analysis.facts.observed_column_count,
            parser: parser_report(analysis.facts.parser),
            columns,
        },
        sampling: SamplingReportV1 {
            scope,
            completeness,
            limited,
            limit_reached,
            source_size_bytes: coverage.source_size_bytes,
            bytes_read: coverage.bytes_read,
            bytes_analyzed: coverage.bytes_analyzed,
            records_analyzed: coverage.sampled_records,
            configured_max_bytes: coverage.max_bytes,
            configured_max_records: coverage.max_records,
            final_newline: coverage.final_newline,
        },
        planner: PlannerReportV1 {
            policy: PolicyReportV1 {
                name: "PlannerPolicyV1",
                version: 1,
            },
            selection_scope: match analysis.plan_disposition {
                PlanDisposition::PlannerRecommendation => "planner_recommendation",
                PlanDisposition::AnalysisLimitFallback => "safe_fallback",
                PlanDisposition::FormatFallback => "format_fallback",
            },
            candidate_archive_modes: ARCHIVE_CANDIDATES,
            candidate_column_strategies: COLUMN_CANDIDATES,
            selected_archive_mode: archive_mode(plan.archive_mode),
            reason: ReasonReportV1 {
                code: archive_reason_code(&plan.reason).ok_or_else(|| {
                    DatapackError::InvalidFormat(
                        "internal analysis report encountered an unmapped archive reason"
                            .to_string(),
                    )
                })?,
                message: &plan.reason,
            },
            estimated_savings_percent: plan.estimated_savings_percent,
            estimated_dictionary_memory_mib: plan.estimated_memory_mb,
        },
        diagnostics,
    })
}

fn parser_report(parser: AnalysisParser) -> ParserReportV1 {
    let (format, delimiter, record_model) = match parser {
        AnalysisParser::LegacyCsvPhysical => ("csv", ",", "physical_line"),
        AnalysisParser::CanonicalDelimited(DelimitedFormat::Comma) => {
            ("csv", ",", "logical_record")
        }
        AnalysisParser::CanonicalDelimited(DelimitedFormat::Semicolon) => {
            ("semicolon_delimited", ";", "logical_record")
        }
        AnalysisParser::CanonicalDelimited(DelimitedFormat::Tab) => ("tsv", "\t", "logical_record"),
        AnalysisParser::CanonicalDelimited(DelimitedFormat::Pipe) => ("psv", "|", "logical_record"),
    };
    ParserReportV1 {
        format,
        delimiter,
        record_model,
        header_mode: "first_record",
    }
}

fn column_report<'a>(
    facts: &ColumnFacts,
    profile: &'a ColumnProfile,
) -> Result<ColumnReportV1<'a>> {
    let cardinality = match facts.cardinality {
        CardinalityEstimate::Exact(value) => EstimateU64ReportV1 {
            kind: EstimateKindV1::Exact,
            value: Some(value),
        },
        CardinalityEstimate::AtLeast(value) => EstimateU64ReportV1 {
            kind: EstimateKindV1::AtLeast,
            value: Some(value),
        },
    };
    let repetition_rate = match facts.cardinality {
        CardinalityEstimate::Exact(unique) if facts.observed_values > 0 => EstimateF64ReportV1 {
            kind: EstimateKindV1::Exact,
            value: Some((1.0 - unique as f64 / facts.observed_values as f64).clamp(0.0, 1.0)),
        },
        CardinalityEstimate::Exact(_) | CardinalityEstimate::AtLeast(_) => EstimateF64ReportV1 {
            kind: EstimateKindV1::Unknown,
            value: None,
        },
    };
    let has_observations = facts.observed_values > 0;

    Ok(ColumnReportV1 {
        index: facts.index,
        name_status: ColumnNameStatusReportV1 {
            is_empty: facts.name_status.is_empty,
            duplicate_of: facts.name_status.duplicate_of,
        },
        observed_values: facts.observed_values,
        empty_values: facts.empty_values,
        numeric_values: facts.numeric_values,
        value_length_bytes: ValueLengthReportV1 {
            minimum: facts.min_value_len_bytes,
            maximum: facts.max_value_len_bytes,
            mean: has_observations.then_some(facts.mean_value_len_bytes),
            total: facts.total_value_bytes,
        },
        cardinality,
        repetition_rate,
        planner: ColumnPlannerReportV1 {
            selected_strategy: column_strategy(profile.recommended_strategy),
            reason: ReasonReportV1 {
                code: column_reason_code(&profile.reason).ok_or_else(|| {
                    DatapackError::InvalidFormat(
                        "internal analysis report encountered an unmapped column reason"
                            .to_string(),
                    )
                })?,
                message: &profile.reason,
            },
            estimated_dictionary_size_kib: profile.estimated_dict_size_kb,
            estimated_encoded_size_bytes: profile.estimated_encoded_size,
            estimated_raw_size_bytes: profile.estimated_raw_size,
        },
    })
}

fn diagnostics(analysis: &DatasetAnalysis) -> Vec<DiagnosticV1> {
    let mut diagnostics = Vec::new();
    if let Some((code, message)) =
        analysis_stop_reason_diagnostic(analysis.facts.coverage.stop_reason)
    {
        diagnostics.push(DiagnosticV1 {
            code,
            severity: DiagnosticSeverityV1::Warning,
            message,
            column_index: None,
        });
    }

    for limitation in &analysis.facts.limitations {
        diagnostics.push(DiagnosticV1 {
            code: limitation.code(),
            severity: DiagnosticSeverityV1::Warning,
            message: limitation.message(analysis.facts.parser),
            column_index: None,
        });
    }

    for column in &analysis.facts.columns {
        if column.name_status.is_empty {
            diagnostics.push(DiagnosticV1 {
                code: "EMPTY_COLUMN_NAME",
                severity: DiagnosticSeverityV1::Warning,
                message: "The header contains an empty column name.",
                column_index: Some(column.index),
            });
        }
        if column.name_status.duplicate_of.is_some() {
            diagnostics.push(DiagnosticV1 {
                code: "DUPLICATE_COLUMN_NAME",
                severity: DiagnosticSeverityV1::Warning,
                message: "The column name duplicates an earlier column name.",
                column_index: Some(column.index),
            });
        }
        if matches!(column.cardinality, CardinalityEstimate::AtLeast(_)) {
            diagnostics.push(DiagnosticV1 {
                code: "CARDINALITY_LIMIT_REACHED",
                severity: DiagnosticSeverityV1::Warning,
                message: "Observed cardinality hash tracking was censored by an analysis limit.",
                column_index: Some(column.index),
            });
        }
    }
    diagnostics
}

fn archive_mode(mode: ArchiveMode) -> ArchiveModeV1 {
    match mode {
        ArchiveMode::CsvColumnarDictionary => ArchiveModeV1::CsvColumnarDictionary,
        ArchiveMode::RawZstd => ArchiveModeV1::RawZstd,
    }
}

fn column_strategy(strategy: ColumnStrategy) -> ColumnStrategyV1 {
    match strategy {
        ColumnStrategy::Dictionary => ColumnStrategyV1::Dictionary,
        ColumnStrategy::Plain => ColumnStrategyV1::Plain,
        ColumnStrategy::DeltaCandidate => ColumnStrategyV1::DeltaCandidate,
        ColumnStrategy::Raw => ColumnStrategyV1::Raw,
    }
}

pub(crate) fn archive_reason_code(reason: &str) -> Option<&'static str> {
    match reason {
        LIMITED_RAW_FALLBACK_REASON => Some("ANALYSIS_LIMITED_RAW_ZSTD_FALLBACK"),
        DELIMITED_FORMAT_RAW_FALLBACK_REASON => {
            Some("STRUCTURED_COMPRESSION_NOT_ENABLED_FOR_DIALECT")
        }
        "Insufficient repetition across majority of columns for dictionary gains." => {
            Some("INSUFFICIENT_REPETITION_MAJORITY")
        }
        "Projected dictionary savings < 5%; RawZstd is more predictable." => {
            Some("PROJECTED_DICTIONARY_SAVINGS_BELOW_THRESHOLD")
        }
        _ if reason.starts_with("High repetition detected in ") => Some("HIGH_REPETITION_DETECTED"),
        _ => None,
    }
}

pub(crate) const fn analysis_stop_reason_code(reason: AnalysisStopReason) -> Option<&'static str> {
    match analysis_stop_reason_diagnostic(reason) {
        Some((code, _)) => Some(code),
        None => None,
    }
}

const fn analysis_stop_reason_diagnostic(
    reason: AnalysisStopReason,
) -> Option<(&'static str, &'static str)> {
    match reason {
        AnalysisStopReason::Complete => None,
        AnalysisStopReason::ByteLimit => Some((
            "SAMPLE_BYTE_LIMIT_REACHED",
            "Analysis stopped at the configured byte sampling limit.",
        )),
        AnalysisStopReason::RecordLimit => Some((
            "SAMPLE_RECORD_LIMIT_REACHED",
            "Analysis stopped at the configured record sampling limit.",
        )),
        AnalysisStopReason::HeaderByteLimit
        | AnalysisStopReason::RecordByteLimit
        | AnalysisStopReason::ColumnLimit
        | AnalysisStopReason::MemoryLimit => None,
    }
}

fn sampling_scope(coverage: &super::model::AnalysisCoverage) -> SamplingScopeV1 {
    if coverage.bytes_analyzed >= coverage.source_size_bytes {
        SamplingScopeV1::Full
    } else {
        SamplingScopeV1::Sampled
    }
}

fn primary_hard_limit(limitations: &[AnalysisLimitation]) -> Option<AnalysisLimitV1> {
    limitations.first().map(|limitation| match limitation {
        AnalysisLimitation::IncompleteHeader => AnalysisLimitV1::SampleBytes,
        AnalysisLimitation::HeaderByteLimit => AnalysisLimitV1::HeaderBytes,
        AnalysisLimitation::RecordByteLimit => AnalysisLimitV1::RecordBytes,
        AnalysisLimitation::ColumnLimit => AnalysisLimitV1::Columns,
        AnalysisLimitation::MemoryLimit => AnalysisLimitV1::Memory,
        AnalysisLimitation::CardinalityMemoryLimit => AnalysisLimitV1::CardinalityMemory,
    })
}

pub(crate) fn column_reason_code(reason: &str) -> Option<&'static str> {
    match reason {
        "Exceeded cardinality threshold" => Some("CARDINALITY_THRESHOLD_EXCEEDED"),
        "Very low cardinality with high repetition" => Some("VERY_LOW_CARDINALITY_HIGH_REPETITION"),
        "Repeated values likely benefit from dictionary IDs" => {
            Some("REPETITION_SUPPORTS_DICTIONARY")
        }
        "Numeric/date heuristic matched" => Some("NUMERIC_OR_DATE_HEURISTIC_MATCHED"),
        "No strong dictionary signal" => Some("NO_STRONG_DICTIONARY_SIGNAL"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{archive_reason_code, column_reason_code};

    #[test]
    fn archive_reason_codes_cover_every_planner_policy_v1_branch() {
        assert_eq!(
            archive_reason_code(super::LIMITED_RAW_FALLBACK_REASON),
            Some("ANALYSIS_LIMITED_RAW_ZSTD_FALLBACK")
        );
        assert_eq!(
            archive_reason_code(super::DELIMITED_FORMAT_RAW_FALLBACK_REASON),
            Some("STRUCTURED_COMPRESSION_NOT_ENABLED_FOR_DIALECT")
        );
        assert_eq!(
            archive_reason_code(
                "Insufficient repetition across majority of columns for dictionary gains."
            ),
            Some("INSUFFICIENT_REPETITION_MAJORITY")
        );
        assert_eq!(
            archive_reason_code("Projected dictionary savings < 5%; RawZstd is more predictable."),
            Some("PROJECTED_DICTIONARY_SAVINGS_BELOW_THRESHOLD")
        );
        assert_eq!(
            archive_reason_code("High repetition detected in 3/4 columns."),
            Some("HIGH_REPETITION_DETECTED")
        );
    }

    #[test]
    fn column_reason_codes_cover_every_planner_policy_v1_branch() {
        let cases = [
            (
                "Exceeded cardinality threshold",
                "CARDINALITY_THRESHOLD_EXCEEDED",
            ),
            (
                "Very low cardinality with high repetition",
                "VERY_LOW_CARDINALITY_HIGH_REPETITION",
            ),
            (
                "Repeated values likely benefit from dictionary IDs",
                "REPETITION_SUPPORTS_DICTIONARY",
            ),
            (
                "Numeric/date heuristic matched",
                "NUMERIC_OR_DATE_HEURISTIC_MATCHED",
            ),
            ("No strong dictionary signal", "NO_STRONG_DICTIONARY_SIGNAL"),
        ];
        for (reason, expected) in cases {
            assert_eq!(column_reason_code(reason), Some(expected));
        }
    }

    #[test]
    fn record_limit_diagnostics_match_the_selected_record_model() {
        assert_eq!(
            super::AnalysisLimitation::HeaderByteLimit
                .message(super::AnalysisParser::LegacyCsvPhysical),
            "The first physical record exceeded the header byte limit."
        );
        assert_eq!(
            super::AnalysisLimitation::RecordByteLimit.message(
                super::AnalysisParser::CanonicalDelimited(super::DelimitedFormat::Pipe)
            ),
            "A logical data record exceeded the record byte limit."
        );
    }
}
