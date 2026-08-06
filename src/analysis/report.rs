//! Versioned machine-readable analysis report DTOs.
//!
//! This module converts internal facts and policy results into a stable JSON
//! boundary without adding serialization concerns to either model.

use serde::Serialize;

use super::engine::DatasetAnalysis;
use super::model::{AnalysisStopReason, CardinalityEstimate, ColumnFacts};
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
    column_count: usize,
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
#[serde(rename_all = "snake_case")]
enum AnalysisLimitV1 {
    ByteLimit,
    RecordLimit,
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
    let (scope, completeness, limited, limit_reached) = match coverage.stop_reason {
        AnalysisStopReason::Complete => {
            (SamplingScopeV1::Full, CompletenessV1::Complete, false, None)
        }
        AnalysisStopReason::ByteLimit => (
            SamplingScopeV1::Sampled,
            CompletenessV1::Partial,
            true,
            Some(AnalysisLimitV1::ByteLimit),
        ),
        AnalysisStopReason::RecordLimit => (
            SamplingScopeV1::Sampled,
            CompletenessV1::Partial,
            true,
            Some(AnalysisLimitV1::RecordLimit),
        ),
    };
    let plan = &analysis.plan;

    Ok(AnalysisReportV1 {
        schema_version: 1,
        report_type: "analysis",
        dataset: DatasetReportV1 {
            source_size_bytes: analysis.facts.source_size_bytes,
            column_count: analysis.facts.columns.len(),
            parser: ParserReportV1 {
                format: "csv",
                delimiter: ",",
                record_model: "physical_line",
                header_mode: "first_record",
            },
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
            selection_scope: "planner_recommendation",
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
    match analysis.facts.coverage.stop_reason {
        AnalysisStopReason::Complete => {}
        AnalysisStopReason::ByteLimit => diagnostics.push(DiagnosticV1 {
            code: "SAMPLE_BYTE_LIMIT_REACHED",
            severity: DiagnosticSeverityV1::Warning,
            message: "Analysis stopped at the configured byte sampling limit.",
            column_index: None,
        }),
        AnalysisStopReason::RecordLimit => diagnostics.push(DiagnosticV1 {
            code: "SAMPLE_RECORD_LIMIT_REACHED",
            severity: DiagnosticSeverityV1::Warning,
            message: "Analysis stopped at the configured record sampling limit.",
            column_index: None,
        }),
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
                message: "The exact observed cardinality exceeded the tracking limit.",
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

fn archive_reason_code(reason: &str) -> Option<&'static str> {
    match reason {
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

fn column_reason_code(reason: &str) -> Option<&'static str> {
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
}
