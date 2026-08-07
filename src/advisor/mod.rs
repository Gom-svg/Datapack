//! Deterministic advice derived from existing analysis facts and policy results.
//!
//! AdvisorPolicyV1 does not inspect input bytes, run compression, or measure
//! performance. It only converts the shared analysis result into a stable,
//! privacy-preserving report.

use std::path::Path;

use serde::Serialize;

use crate::analysis::{
    self, analysis_stop_reason_code, archive_reason_code, column_reason_code, AnalysisStopReason,
    CardinalityEstimate, DatasetAnalysis, PlanDisposition, CARDINALITY_LOWER_BOUND,
    STRUCTURED_ANALYSIS_UNAVAILABLE_CODE,
};
use crate::error::{DatapackError, Result};
use crate::planning::ArchiveMode;

const CARDINALITY_LIMIT_REACHED: &str = "CARDINALITY_LIMIT_REACHED";
const CARDINALITY_THRESHOLD_EXCEEDED: &str = "CARDINALITY_THRESHOLD_EXCEEDED";

#[derive(Debug, Serialize)]
pub(crate) struct AdvisorReportV1 {
    pub(crate) schema_version: u32,
    pub(crate) report_type: &'static str,
    pub(crate) policy: PolicyReportV1,
    pub(crate) analysis: AdvisorAnalysisV1,
    pub(crate) recommendations: Vec<RecommendationV1>,
}

#[derive(Debug, Serialize)]
pub(crate) struct PolicyReportV1 {
    pub(crate) name: &'static str,
    pub(crate) version: u32,
}

#[derive(Debug, Serialize)]
pub(crate) struct AdvisorAnalysisV1 {
    pub(crate) status: AnalysisStatusV1,
    pub(crate) scope: AnalysisScopeV1,
    pub(crate) completeness: AnalysisCompletenessV1,
    pub(crate) source_size_bytes: u64,
    pub(crate) bytes_analyzed: Option<u64>,
    pub(crate) records_analyzed: Option<u64>,
    pub(crate) planner: Option<PlannerBasisV1>,
    pub(crate) evidence: Vec<EvidenceV1>,
    pub(crate) high_cardinality_columns: Vec<HighCardinalityColumnV1>,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AnalysisStatusV1 {
    Available,
    Unavailable,
}

impl AnalysisStatusV1 {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Available => "available",
            Self::Unavailable => "unavailable",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AnalysisScopeV1 {
    Full,
    Sampled,
    Unavailable,
}

impl AnalysisScopeV1 {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::Sampled => "sampled",
            Self::Unavailable => "unavailable",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AnalysisCompletenessV1 {
    Complete,
    Partial,
    Unavailable,
}

impl AnalysisCompletenessV1 {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Complete => "complete",
            Self::Partial => "partial",
            Self::Unavailable => "unavailable",
        }
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct PlannerBasisV1 {
    pub(crate) policy: PolicyReportV1,
    pub(crate) selection_scope: &'static str,
    pub(crate) selected_archive_mode: ArchiveModeV1,
    pub(crate) reason_code: &'static str,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ArchiveModeV1 {
    CsvColumnarDictionary,
    RawZstd,
}

impl ArchiveModeV1 {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::CsvColumnarDictionary => "csv_columnar_dictionary",
            Self::RawZstd => "raw_zstd",
        }
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct HighCardinalityColumnV1 {
    pub(crate) column_index: usize,
    pub(crate) cardinality: CardinalityLowerBoundV1,
}

#[derive(Debug, Serialize)]
pub(crate) struct CardinalityLowerBoundV1 {
    kind: &'static str,
    value: u64,
}

#[derive(Debug, Serialize)]
pub(crate) struct RecommendationV1 {
    pub(crate) code: &'static str,
    pub(crate) category: &'static str,
    pub(crate) message: &'static str,
    pub(crate) evidence: Vec<EvidenceV1>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct EvidenceV1 {
    pub(crate) source: EvidenceSourceV1,
    pub(crate) code: &'static str,
    pub(crate) column_index: Option<usize>,
}

impl EvidenceV1 {
    const fn new(
        source: EvidenceSourceV1,
        code: &'static str,
        column_index: Option<usize>,
    ) -> Self {
        Self {
            source,
            code,
            column_index,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum EvidenceSourceV1 {
    AnalysisStatus,
    AnalysisDiagnostic,
    PlannerPolicy,
    ColumnPolicy,
    SafetyPolicy,
    CompatibilityPolicy,
}

impl EvidenceSourceV1 {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::AnalysisStatus => "analysis_status",
            Self::AnalysisDiagnostic => "analysis_diagnostic",
            Self::PlannerPolicy => "planner_policy",
            Self::ColumnPolicy => "column_policy",
            Self::SafetyPolicy => "safety_policy",
            Self::CompatibilityPolicy => "compatibility_policy",
        }
    }
}

pub(crate) fn advise_path(path: &Path, sample_mb: u64) -> Result<AdvisorReportV1> {
    match analysis::analyze_cli_path(path, sample_mb) {
        Ok(analysis) => build_available_report(&analysis),
        Err(DatapackError::InvalidCsv(_)) => {
            let source_size_bytes = std::fs::metadata(path)
                .map_err(|_| DatapackError::AnalyzeRead(path.display().to_string()))?
                .len();
            Ok(build_unavailable_report(source_size_bytes))
        }
        Err(error) => Err(error),
    }
}

fn build_available_report(analysis: &DatasetAnalysis) -> Result<AdvisorReportV1> {
    if analysis.facts.columns.len() != analysis.columns.len()
        || analysis
            .facts
            .columns
            .iter()
            .zip(&analysis.columns)
            .any(|(facts, profile)| facts.index != profile.column_index)
    {
        return Err(DatapackError::InvalidFormat(
            "AdvisorPolicyV1 encountered an internal analysis column mismatch".to_string(),
        ));
    }
    let reason_code = archive_reason_code(&analysis.plan.reason).ok_or_else(|| {
        DatapackError::InvalidFormat(
            "AdvisorPolicyV1 encountered an unmapped archive selection reason".to_string(),
        )
    })?;
    let partial = analysis.facts.coverage.stop_reason != AnalysisStopReason::Complete
        || !analysis.facts.limitations.is_empty();
    let scope =
        if analysis.facts.coverage.bytes_analyzed >= analysis.facts.coverage.source_size_bytes {
            AnalysisScopeV1::Full
        } else {
            AnalysisScopeV1::Sampled
        };

    let selection_evidence = EvidenceV1::new(
        selection_evidence_source(analysis.plan_disposition),
        reason_code,
        None,
    );
    let mut evidence = vec![selection_evidence.clone()];
    let mut limiting_evidence = Vec::new();
    if let Some(code) = analysis_stop_reason_code(analysis.facts.coverage.stop_reason) {
        push_unique_evidence(
            &mut evidence,
            EvidenceV1::new(EvidenceSourceV1::AnalysisDiagnostic, code, None),
        );
        push_unique_evidence(
            &mut limiting_evidence,
            EvidenceV1::new(EvidenceSourceV1::AnalysisDiagnostic, code, None),
        );
    }
    for limitation in &analysis.facts.limitations {
        let code = limitation.code();
        push_unique_evidence(
            &mut evidence,
            EvidenceV1::new(EvidenceSourceV1::AnalysisDiagnostic, code, None),
        );
        push_unique_evidence(
            &mut limiting_evidence,
            EvidenceV1::new(EvidenceSourceV1::AnalysisDiagnostic, code, None),
        );
    }

    let mut high_cardinality_columns = Vec::new();
    let mut high_cardinality_evidence = Vec::new();
    for (facts, profile) in analysis.facts.columns.iter().zip(&analysis.columns) {
        let Some(value) = high_cardinality_lower_bound(facts.cardinality) else {
            continue;
        };
        high_cardinality_columns.push(HighCardinalityColumnV1 {
            column_index: facts.index,
            cardinality: CardinalityLowerBoundV1 {
                kind: "at_least",
                value,
            },
        });
        let diagnostic = EvidenceV1::new(
            EvidenceSourceV1::AnalysisDiagnostic,
            CARDINALITY_LIMIT_REACHED,
            Some(facts.index),
        );
        push_unique_evidence(&mut evidence, diagnostic.clone());
        push_unique_evidence(&mut high_cardinality_evidence, diagnostic);
        if column_reason_code(&profile.reason) == Some(CARDINALITY_THRESHOLD_EXCEEDED) {
            let column_policy = EvidenceV1::new(
                EvidenceSourceV1::ColumnPolicy,
                CARDINALITY_THRESHOLD_EXCEEDED,
                Some(facts.index),
            );
            push_unique_evidence(&mut evidence, column_policy.clone());
            push_unique_evidence(&mut high_cardinality_evidence, column_policy);
        }
    }

    let selected_archive_mode = archive_mode(analysis.plan.archive_mode);
    let mut recommendations = vec![compression_recommendation(
        selected_archive_mode,
        analysis.plan_disposition,
        selection_evidence,
    )];
    if !high_cardinality_columns.is_empty() {
        recommendations.push(RecommendationV1 {
            code: "HIGH_CARDINALITY_OBSERVED",
            category: "dataset_observation",
            message: "One or more columns reached a censored lower bound of at least 8193 distinct observed values.",
            evidence: high_cardinality_evidence,
        });
    }
    if partial {
        let limiting_evidence =
            nonempty_limit_evidence(limiting_evidence, analysis.plan_disposition, reason_code);
        recommendations.push(RecommendationV1 {
            code: "ANALYSIS_LIMITED",
            category: "analysis_limitation",
            message:
                "Analysis is partial because a configured sampling or safety limit was reached.",
            evidence: limiting_evidence.clone(),
        });
        recommendations.push(RecommendationV1 {
            code: "FULL_COMPARISON_RECOMMENDED",
            category: "next_action",
            message: "Run Compare in Full mode when complete-input measurements and round-trip validation are needed.",
            evidence: limiting_evidence,
        });
    }

    Ok(AdvisorReportV1 {
        schema_version: 1,
        report_type: "advisor",
        policy: advisor_policy(),
        analysis: AdvisorAnalysisV1 {
            status: AnalysisStatusV1::Available,
            scope,
            completeness: if partial {
                AnalysisCompletenessV1::Partial
            } else {
                AnalysisCompletenessV1::Complete
            },
            source_size_bytes: analysis.facts.source_size_bytes,
            bytes_analyzed: Some(analysis.facts.coverage.bytes_analyzed),
            records_analyzed: Some(analysis.facts.coverage.sampled_records),
            planner: Some(PlannerBasisV1 {
                policy: planner_policy(),
                selection_scope: selection_scope(analysis.plan_disposition),
                selected_archive_mode,
                reason_code,
            }),
            evidence,
            high_cardinality_columns,
        },
        recommendations,
    })
}

fn build_unavailable_report(source_size_bytes: u64) -> AdvisorReportV1 {
    AdvisorReportV1 {
        schema_version: 1,
        report_type: "advisor",
        policy: advisor_policy(),
        analysis: AdvisorAnalysisV1 {
            status: AnalysisStatusV1::Unavailable,
            scope: AnalysisScopeV1::Unavailable,
            completeness: AnalysisCompletenessV1::Unavailable,
            source_size_bytes,
            bytes_analyzed: None,
            records_analyzed: None,
            planner: None,
            evidence: vec![EvidenceV1::new(
                EvidenceSourceV1::AnalysisStatus,
                STRUCTURED_ANALYSIS_UNAVAILABLE_CODE,
                None,
            )],
            high_cardinality_columns: Vec::new(),
        },
        recommendations: vec![
            RecommendationV1 {
                code: "RAW_ZSTD_RECOMMENDED",
                category: "compression_strategy",
                message: "Structured analysis is unavailable; use the byte-preserving RawZstd fallback.",
                evidence: vec![EvidenceV1::new(
                    EvidenceSourceV1::AnalysisStatus,
                    STRUCTURED_ANALYSIS_UNAVAILABLE_CODE,
                    None,
                )],
            },
            RecommendationV1 {
                code: "FULL_COMPARISON_RECOMMENDED",
                category: "next_action",
                message: "Run Compare in Full mode when complete-input measurements and round-trip validation are needed.",
                evidence: vec![EvidenceV1::new(
                    EvidenceSourceV1::AnalysisStatus,
                    STRUCTURED_ANALYSIS_UNAVAILABLE_CODE,
                    None,
                )],
            },
        ],
    }
}

fn compression_recommendation(
    mode: ArchiveModeV1,
    disposition: PlanDisposition,
    evidence: EvidenceV1,
) -> RecommendationV1 {
    let (code, message) = match (mode, disposition) {
        (ArchiveModeV1::CsvColumnarDictionary, PlanDisposition::PlannerRecommendation) => (
            "STRUCTURED_COMPRESSION_RECOMMENDED",
            "PlannerPolicyV1 recommends structured compression for the analyzed scope; compression retains its safe RawZstd fallback.",
        ),
        (ArchiveModeV1::RawZstd, PlanDisposition::PlannerRecommendation) => (
            "RAW_ZSTD_RECOMMENDED",
            "PlannerPolicyV1 recommends RawZstd for the analyzed scope.",
        ),
        (ArchiveModeV1::RawZstd, PlanDisposition::AnalysisLimitFallback) => (
            "RAW_ZSTD_RECOMMENDED",
            "Analysis safety limits require the byte-preserving RawZstd fallback.",
        ),
        (ArchiveModeV1::RawZstd, PlanDisposition::FormatFallback) => (
            "RAW_ZSTD_RECOMMENDED",
            "The compatibility adapter requires the byte-preserving RawZstd fallback.",
        ),
        (ArchiveModeV1::CsvColumnarDictionary, _) => (
            "STRUCTURED_COMPRESSION_RECOMMENDED",
            "Structured compression is selected for the analyzed scope; compression retains its safe RawZstd fallback.",
        ),
    };
    RecommendationV1 {
        code,
        category: "compression_strategy",
        message,
        evidence: vec![evidence],
    }
}

fn archive_mode(mode: ArchiveMode) -> ArchiveModeV1 {
    match mode {
        ArchiveMode::CsvColumnarDictionary => ArchiveModeV1::CsvColumnarDictionary,
        ArchiveMode::RawZstd => ArchiveModeV1::RawZstd,
    }
}

const fn advisor_policy() -> PolicyReportV1 {
    PolicyReportV1 {
        name: "AdvisorPolicyV1",
        version: 1,
    }
}

const fn planner_policy() -> PolicyReportV1 {
    PolicyReportV1 {
        name: "PlannerPolicyV1",
        version: 1,
    }
}

const fn selection_scope(disposition: PlanDisposition) -> &'static str {
    match disposition {
        PlanDisposition::PlannerRecommendation => "planner_recommendation",
        PlanDisposition::AnalysisLimitFallback => "safe_fallback",
        PlanDisposition::FormatFallback => "format_fallback",
    }
}

const fn selection_evidence_source(disposition: PlanDisposition) -> EvidenceSourceV1 {
    match disposition {
        PlanDisposition::PlannerRecommendation => EvidenceSourceV1::PlannerPolicy,
        PlanDisposition::AnalysisLimitFallback => EvidenceSourceV1::SafetyPolicy,
        PlanDisposition::FormatFallback => EvidenceSourceV1::CompatibilityPolicy,
    }
}

fn high_cardinality_lower_bound(cardinality: CardinalityEstimate) -> Option<u64> {
    match cardinality {
        CardinalityEstimate::AtLeast(value) if value >= CARDINALITY_LOWER_BOUND => Some(value),
        CardinalityEstimate::Exact(_) | CardinalityEstimate::AtLeast(_) => None,
    }
}

fn push_unique_evidence(evidence: &mut Vec<EvidenceV1>, item: EvidenceV1) {
    if !evidence.contains(&item) {
        evidence.push(item);
    }
}

fn nonempty_limit_evidence(
    limiting_evidence: Vec<EvidenceV1>,
    disposition: PlanDisposition,
    fallback_reason_code: &'static str,
) -> Vec<EvidenceV1> {
    if limiting_evidence.is_empty() {
        vec![EvidenceV1::new(
            selection_evidence_source(disposition),
            fallback_reason_code,
            None,
        )]
    } else {
        limiting_evidence
    }
}

#[cfg(test)]
mod tests {
    use super::high_cardinality_lower_bound;
    use crate::analysis::{CardinalityEstimate, CARDINALITY_LOWER_BOUND};

    #[test]
    fn high_cardinality_requires_the_certified_censored_lower_bound() {
        assert_eq!(
            high_cardinality_lower_bound(CardinalityEstimate::AtLeast(CARDINALITY_LOWER_BOUND - 1)),
            None
        );
        assert_eq!(
            high_cardinality_lower_bound(CardinalityEstimate::AtLeast(CARDINALITY_LOWER_BOUND)),
            Some(CARDINALITY_LOWER_BOUND)
        );
        assert_eq!(
            high_cardinality_lower_bound(CardinalityEstimate::Exact(CARDINALITY_LOWER_BOUND)),
            None
        );
    }
}
