use crate::error::{DatapackError, Result};

use super::{ColumnStrategy, CompressionPlan};

const MIB: u64 = 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ColumnExecutionMode {
    Plain,
    Dictionary,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DictionaryExecutionLimits {
    pub(crate) max_values: u64,
    pub(crate) max_bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ExecutionColumn {
    column_index: usize,
    mode: ColumnExecutionMode,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ColumnExecutionPlan {
    columns: Vec<ExecutionColumn>,
    dictionary_limits: DictionaryExecutionLimits,
}

impl ColumnExecutionPlan {
    pub(crate) fn from_compression_plan(
        plan: &CompressionPlan,
        max_dictionary_values: u64,
        max_dictionary_mb: u64,
    ) -> Self {
        let columns = plan
            .columns
            .iter()
            .map(|column| ExecutionColumn {
                column_index: column.column_index,
                mode: match column.strategy {
                    ColumnStrategy::Dictionary => ColumnExecutionMode::Dictionary,
                    ColumnStrategy::Plain
                    | ColumnStrategy::DeltaCandidate
                    | ColumnStrategy::Raw => ColumnExecutionMode::Plain,
                },
            })
            .collect();
        Self {
            columns,
            dictionary_limits: DictionaryExecutionLimits {
                max_values: max_dictionary_values,
                max_bytes: max_dictionary_mb.saturating_mul(MIB),
            },
        }
    }

    pub(crate) fn validate_column_count(&self, actual_columns: usize) -> Result<()> {
        if self.columns.len() != actual_columns {
            return Err(DatapackError::InvalidFormat(format!(
                "column execution plan contains {} columns, but input contains {actual_columns}",
                self.columns.len()
            )));
        }
        for (expected_index, column) in self.columns.iter().enumerate() {
            if column.column_index != expected_index {
                return Err(DatapackError::InvalidFormat(format!(
                    "column execution plan index {} appears at position {expected_index}",
                    column.column_index
                )));
            }
        }
        Ok(())
    }

    pub(crate) fn mode(&self, column_index: usize) -> Result<ColumnExecutionMode> {
        let column = self.columns.get(column_index).ok_or_else(|| {
            DatapackError::InvalidFormat(format!(
                "column execution plan is missing column {column_index}"
            ))
        })?;
        if column.column_index != column_index {
            return Err(DatapackError::InvalidFormat(format!(
                "column execution plan expected index {column_index}, found {}",
                column.column_index
            )));
        }
        Ok(column.mode)
    }

    pub(crate) const fn dictionary_limits(&self) -> DictionaryExecutionLimits {
        self.dictionary_limits
    }

    #[cfg(test)]
    pub(crate) fn with_dictionary_limit_bytes(
        plan: &CompressionPlan,
        max_dictionary_values: u64,
        max_dictionary_bytes: u64,
    ) -> Self {
        let mut execution = Self::from_compression_plan(plan, max_dictionary_values, u64::MAX);
        execution.dictionary_limits.max_bytes = max_dictionary_bytes;
        execution
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::planning::{ArchiveMode, ColumnPlan};

    fn plan(strategies: [ColumnStrategy; 4]) -> CompressionPlan {
        CompressionPlan {
            archive_mode: ArchiveMode::CsvColumnarDictionary,
            columns: strategies
                .into_iter()
                .enumerate()
                .map(|(column_index, strategy)| ColumnPlan {
                    column_index,
                    column_name: format!("column_{column_index}"),
                    strategy,
                    reason: "test plan".to_string(),
                })
                .collect(),
            estimated_savings_percent: 25.0,
            estimated_memory_mb: 1.0,
            planning_time_ms: 0,
            reason: "test execution mapping".to_string(),
        }
    }

    #[test]
    fn frozen_advisory_strategies_project_to_supported_dcsv01_modes() {
        let source = plan([
            ColumnStrategy::Dictionary,
            ColumnStrategy::Plain,
            ColumnStrategy::DeltaCandidate,
            ColumnStrategy::Raw,
        ]);
        let execution = ColumnExecutionPlan::from_compression_plan(&source, 17, 3);

        assert_eq!(execution.mode(0).unwrap(), ColumnExecutionMode::Dictionary);
        assert_eq!(execution.mode(1).unwrap(), ColumnExecutionMode::Plain);
        assert_eq!(execution.mode(2).unwrap(), ColumnExecutionMode::Plain);
        assert_eq!(execution.mode(3).unwrap(), ColumnExecutionMode::Plain);
        assert_eq!(
            execution.dictionary_limits(),
            DictionaryExecutionLimits {
                max_values: 17,
                max_bytes: 3 * MIB,
            }
        );
        assert_eq!(source.columns[2].strategy, ColumnStrategy::DeltaCandidate);
        assert_eq!(source.columns[3].strategy, ColumnStrategy::Raw);
    }

    #[test]
    fn malformed_plan_shape_is_rejected_without_index_guessing() {
        let source = plan([
            ColumnStrategy::Dictionary,
            ColumnStrategy::Plain,
            ColumnStrategy::Plain,
            ColumnStrategy::Plain,
        ]);
        let mut execution = ColumnExecutionPlan::from_compression_plan(&source, 10, 1);

        assert!(execution.validate_column_count(3).is_err());
        execution.columns[1].column_index = 2;
        let error = execution.validate_column_count(4).unwrap_err();
        assert!(error.to_string().contains("index 2 appears at position 1"));
    }
}
