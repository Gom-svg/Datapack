#[cfg(test)]
use std::path::Path;

#[cfg(test)]
use crate::analysis;
use crate::analysis::{CardinalityEstimate, DatasetFacts, CARDINALITY_LOWER_BOUND};
#[cfg(test)]
use crate::error::Result;

pub mod plan;

mod execution;

pub(crate) use execution::{ColumnExecutionMode, ColumnExecutionPlan, DictionaryExecutionLimits};

#[cfg(test)]
pub(crate) use crate::analysis::{
    AnalysisEngine as SampleAnalyzer, AnalysisLimits, DatasetAnalysis as SampleAnalysis,
    SampleConfig,
};
pub use plan::{ArchiveMode, ColumnPlan, ColumnStrategy, CompressionPlan};

#[derive(Debug, Clone)]
pub struct ColumnProfile {
    pub column_index: usize,
    pub column_name: String,
    pub unique_count: u64,
    pub repetition_rate: f32,
    pub avg_value_len_bytes: f32,
    pub estimated_dict_size_kb: u64,
    pub estimated_encoded_size: u64,
    pub estimated_raw_size: u64,
    pub recommended_strategy: ColumnStrategy,
    pub reason: String,
    pub exceeded_cardinality: bool,
}

/// Exact compatibility inputs consumed by [`PlannerPolicyV1`].
///
/// This adapter intentionally retains the legacy cardinality sentinel and the
/// legacy physical-value statistics. The factual model remains free to report
/// the narrower, honest cardinality lower bound independently.
#[derive(Debug, Clone)]
pub(crate) struct PlannerFeaturesV1<'a> {
    facts: &'a DatasetFacts,
}

impl<'a> PlannerFeaturesV1<'a> {
    pub(crate) fn from_facts(facts: &'a DatasetFacts) -> Self {
        Self { facts }
    }

    fn sampled_rows(&self) -> u64 {
        self.facts.coverage.sampled_records
    }

    fn sampled_bytes(&self) -> u64 {
        self.facts.coverage.bytes_read
    }

    fn total_file_size(&self) -> u64 {
        self.facts.coverage.scope_size_bytes
    }

    fn columns(&self) -> impl Iterator<Item = PlannerColumnFeaturesV1<'_>> {
        let sampled_rows = self.sampled_rows();
        self.facts.columns.iter().map(move |column| {
            let exceeded_cardinality = column.cardinality.is_censored();
            let unique_count = match column.cardinality {
                CardinalityEstimate::Exact(unique) => unique,
                CardinalityEstimate::AtLeast(_) => sampled_rows.max(CARDINALITY_LOWER_BOUND),
            };
            PlannerColumnFeaturesV1 {
                column_index: column.index,
                column_name: &column.name,
                unique_count,
                mean_len: column.mean_value_len_bytes,
                observed_values: column.observed_values,
                total_value_bytes: column.total_value_bytes,
                numeric_success: column.numeric_values,
                exceeded_cardinality,
            }
        })
    }
}

#[derive(Debug, Clone)]
struct PlannerColumnFeaturesV1<'a> {
    column_index: usize,
    column_name: &'a str,
    unique_count: u64,
    mean_len: f64,
    observed_values: u64,
    total_value_bytes: u64,
    numeric_success: u64,
    exceeded_cardinality: bool,
}

impl PlannerColumnFeaturesV1<'_> {
    fn looks_numeric_or_date(&self) -> bool {
        let lower = self.column_name.to_ascii_lowercase();
        let name_match = [
            "date", "time", "ts", "amount", "price", "qty", "count", "id",
        ]
        .iter()
        .any(|needle| lower.contains(needle));
        let numeric_rate = if self.observed_values == 0 {
            0.0
        } else {
            self.numeric_success as f32 / self.observed_values as f32
        };
        name_match || numeric_rate >= 0.80
    }
}

/// Frozen planner formulas and thresholds extracted from the legacy analyzer.
pub(crate) struct PlannerPolicyV1;

impl PlannerPolicyV1 {
    pub(crate) fn profiles(features: &PlannerFeaturesV1) -> Vec<ColumnProfile> {
        let sampled_rows = features.sampled_rows();
        let row_scale = if sampled_rows == 0 {
            1.0
        } else {
            sampled_rows as f64
        };
        let sampled_bytes = features.sampled_bytes();
        let byte_scale = if sampled_bytes == 0 {
            1.0
        } else {
            features.total_file_size() as f64 / sampled_bytes as f64
        };

        features
            .columns()
            .map(|column| {
                let unique_count = column.unique_count;
                let repetition_rate = if sampled_rows == 0 || column.exceeded_cardinality {
                    0.0
                } else {
                    (1.0 - unique_count as f32 / sampled_rows as f32).clamp(0.0, 1.0)
                };
                let projected_unique =
                    ((unique_count as f64 * byte_scale).ceil() as u64).max(unique_count);
                let avg_len = column.mean_len as f32;
                let dict_size = projected_unique.saturating_mul(avg_len.ceil() as u64 + 4);
                let id_width = if projected_unique <= 255 {
                    1
                } else if projected_unique <= 65_535 {
                    2
                } else {
                    4
                };
                let projected_rows = (row_scale * byte_scale).ceil() as u64;
                let encoded_size =
                    dict_size.saturating_add(projected_rows.saturating_mul(id_width));
                let raw_size = ((column.total_value_bytes as f64 * byte_scale).ceil() as u64)
                    .saturating_add(projected_rows);
                let (strategy, reason) = recommend_strategy(&column, unique_count, repetition_rate);

                ColumnProfile {
                    column_index: column.column_index,
                    column_name: column.column_name.to_string(),
                    unique_count,
                    repetition_rate,
                    avg_value_len_bytes: avg_len,
                    estimated_dict_size_kb: dict_size.saturating_add(1023) / 1024,
                    estimated_encoded_size: encoded_size,
                    estimated_raw_size: raw_size,
                    recommended_strategy: strategy,
                    reason,
                    exceeded_cardinality: column.exceeded_cardinality,
                }
            })
            .collect()
    }

    pub(crate) fn plan(columns: &[ColumnProfile], planning_time_ms: u64) -> CompressionPlan {
        let raw_columns = columns
            .iter()
            .filter(|column| column.repetition_rate < 0.10)
            .count();
        let encoded_size = columns
            .iter()
            .map(|column| column.estimated_encoded_size)
            .fold(0u64, u64::saturating_add);
        let raw_size = columns
            .iter()
            .map(|column| column.estimated_raw_size)
            .fold(0u64, u64::saturating_add);
        let estimated_savings_percent = if raw_size == 0 {
            0.0
        } else {
            ((1.0 - encoded_size as f32 / raw_size as f32) * 100.0).clamp(0.0, 100.0)
        };
        let estimated_memory_mb: f32 = columns
            .iter()
            .filter(|column| column.recommended_strategy == ColumnStrategy::Dictionary)
            .map(|column| column.estimated_dict_size_kb as f32 / 1024.0)
            .sum::<f32>()
            .max(0.0);

        let (archive_mode, reason) =
            if !columns.is_empty() && raw_columns * 100 >= columns.len() * 70 {
                (
                    ArchiveMode::RawZstd,
                    "Insufficient repetition across majority of columns for dictionary gains."
                        .to_string(),
                )
            } else if encoded_size as f64 >= raw_size as f64 * 0.95 {
                (
                    ArchiveMode::RawZstd,
                    "Projected dictionary savings < 5%; RawZstd is more predictable.".to_string(),
                )
            } else {
                let dictionary_columns = columns
                    .iter()
                    .filter(|column| column.recommended_strategy == ColumnStrategy::Dictionary)
                    .count();
                (
                    ArchiveMode::CsvColumnarDictionary,
                    format!(
                        "High repetition detected in {dictionary_columns}/{} columns.",
                        columns.len()
                    ),
                )
            };

        CompressionPlan {
            archive_mode,
            columns: columns
                .iter()
                .map(|column| ColumnPlan {
                    column_index: column.column_index,
                    column_name: column.column_name.clone(),
                    strategy: column.recommended_strategy,
                    reason: truncate_reason(&column.reason),
                })
                .collect(),
            estimated_savings_percent,
            estimated_memory_mb,
            planning_time_ms,
            reason,
        }
    }
}

#[allow(dead_code)] // Compatibility entry point retained while callers migrate to the policy.
pub fn build_plan(columns: &[ColumnProfile], planning_time_ms: u64) -> CompressionPlan {
    PlannerPolicyV1::plan(columns, planning_time_ms)
}

pub fn apply_dictionary_limits(
    plan: &mut CompressionPlan,
    profiles: &[ColumnProfile],
    max_dictionary_values: u64,
    max_dictionary_mb: u64,
) -> Vec<String> {
    let mut warnings = Vec::new();
    for column in &mut plan.columns {
        if column.strategy != ColumnStrategy::Dictionary {
            continue;
        }
        let Some(profile) = profiles
            .iter()
            .find(|profile| profile.column_index == column.column_index)
        else {
            continue;
        };
        let over_values = profile.unique_count > max_dictionary_values;
        let over_memory = profile.estimated_dict_size_kb > max_dictionary_mb.saturating_mul(1024);
        if over_values || over_memory {
            column.strategy = ColumnStrategy::Plain;
            column.reason = "Dictionary limit exceeded; switched to Plain".to_string();
            warnings.push(column.column_name.clone());
        }
    }
    warnings
}

fn recommend_strategy(
    column: &PlannerColumnFeaturesV1<'_>,
    unique_count: u64,
    repetition_rate: f32,
) -> (ColumnStrategy, String) {
    if column.exceeded_cardinality || unique_count > 65_535 {
        return (
            ColumnStrategy::Raw,
            "Exceeded cardinality threshold".to_string(),
        );
    }
    if unique_count <= 255 && repetition_rate >= 0.60 {
        return (
            ColumnStrategy::Dictionary,
            "Very low cardinality with high repetition".to_string(),
        );
    }
    if unique_count <= 65_535 && repetition_rate >= 0.20 {
        return (
            ColumnStrategy::Dictionary,
            "Repeated values likely benefit from dictionary IDs".to_string(),
        );
    }
    if column.looks_numeric_or_date() {
        return (
            ColumnStrategy::DeltaCandidate,
            "Numeric/date heuristic matched".to_string(),
        );
    }
    (
        ColumnStrategy::Plain,
        "No strong dictionary signal".to_string(),
    )
}

fn truncate_reason(reason: &str) -> String {
    if reason.len() <= 120 {
        reason.to_string()
    } else {
        reason[..120].to_string()
    }
}

#[cfg(test)]
pub fn analyze_path(path: &Path, sample_mb: u64) -> Result<SampleAnalysis> {
    analysis::analyze_path(path, sample_mb)
}

#[cfg(test)]
mod tests;
