//! Internal factual analysis model.
//!
//! These types describe observations made by the legacy-compatible scanner.
//! They are deliberately independent from both the planner policy and the
//! serialized [`crate::metadata::DpackMetadata`] graph.

/// The first distinct value that cannot be retained by the legacy tracker.
pub(crate) const CARDINALITY_LOWER_BOUND: u64 = 8_193;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CardinalityEstimate {
    Exact(u64),
    AtLeast(u64),
}

impl CardinalityEstimate {
    pub(crate) fn is_censored(self) -> bool {
        matches!(self, Self::AtLeast(_))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)] // Consumed by later presentation/report phases; populated now.
pub(crate) struct ColumnNameStatus {
    pub(crate) is_empty: bool,
    pub(crate) duplicate_of: Option<usize>,
}

impl ColumnNameStatus {
    #[cfg(test)]
    pub(crate) const fn named() -> Self {
        Self {
            is_empty: false,
            duplicate_of: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AnalysisStopReason {
    Complete,
    ByteLimit,
    RecordLimit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AnalysisCoverage {
    pub(crate) source_size_bytes: u64,
    /// Bytes read by the legacy sampler, including its characterized overshoot.
    pub(crate) bytes_read: u64,
    /// Bytes whose line structure and fields were actually analyzed.
    pub(crate) bytes_analyzed: u64,
    pub(crate) sampled_records: u64,
    pub(crate) max_bytes: u64,
    pub(crate) max_records: u64,
    pub(crate) stop_reason: AnalysisStopReason,
    /// Known only when every source byte was analyzed.
    pub(crate) final_newline: Option<bool>,
}

#[derive(Debug, Clone)]
pub(crate) struct ColumnFacts {
    pub(crate) index: usize,
    pub(crate) name: String,
    #[allow(dead_code)] // Passive diagnostic; intentionally excluded from policy v1.
    pub(crate) name_status: ColumnNameStatus,
    pub(crate) observed_values: u64,
    /// Physical empty fields only. A quoted `""` has length two and is not empty.
    #[allow(dead_code)] // Passive metric; intentionally excluded from policy v1.
    pub(crate) empty_values: u64,
    #[allow(dead_code)] // Passive metric; intentionally excluded from policy v1.
    pub(crate) min_value_len_bytes: Option<u64>,
    #[allow(dead_code)] // Passive metric; intentionally excluded from policy v1.
    pub(crate) max_value_len_bytes: Option<u64>,
    pub(crate) mean_value_len_bytes: f64,
    pub(crate) total_value_bytes: u64,
    pub(crate) numeric_values: u64,
    pub(crate) cardinality: CardinalityEstimate,
}

#[derive(Debug, Clone)]
pub(crate) struct DatasetFacts {
    pub(crate) input_name: String,
    pub(crate) source_size_bytes: u64,
    pub(crate) coverage: AnalysisCoverage,
    pub(crate) columns: Vec<ColumnFacts>,
}
