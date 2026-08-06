//! Internal factual analysis model.
//!
//! These types describe observations made by the legacy-compatible scanner.
//! They are deliberately independent from both the planner policy and the
//! serialized [`crate::metadata::DpackMetadata`] graph.

/// The first distinct value that cannot be retained by the legacy tracker.
pub(crate) const CARDINALITY_LOWER_BOUND: u64 = 8_193;
pub(crate) const LIMITED_RAW_FALLBACK_REASON: &str =
    "Analysis could not safely complete within configured limits; RawZstd fallback required.";
pub(crate) const DELIMITED_FORMAT_RAW_FALLBACK_REASON: &str =
    "Structured compression is not enabled for this detected delimiter; RawZstd fallback required.";
pub(crate) const COMMA_ELIGIBILITY_RAW_FALLBACK_REASON: &str =
    "Canonical comma structured-compression eligibility was not established; RawZstd fallback required.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DelimitedFormat {
    Comma,
    Semicolon,
    Tab,
    Pipe,
}

impl DelimitedFormat {
    pub(crate) const fn from_byte(delimiter: u8) -> Option<Self> {
        match delimiter {
            b',' => Some(Self::Comma),
            b';' => Some(Self::Semicolon),
            b'\t' => Some(Self::Tab),
            b'|' => Some(Self::Pipe),
            _ => None,
        }
    }

    pub(crate) const fn delimiter(self) -> u8 {
        match self {
            Self::Comma => b',',
            Self::Semicolon => b';',
            Self::Tab => b'\t',
            Self::Pipe => b'|',
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AnalysisParser {
    LegacyCsvPhysical,
    CanonicalDelimited(DelimitedFormat),
}

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
    HeaderByteLimit,
    RecordByteLimit,
    ColumnLimit,
    MemoryLimit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AnalysisLimitation {
    IncompleteHeader,
    HeaderByteLimit,
    RecordByteLimit,
    ColumnLimit,
    MemoryLimit,
    CardinalityMemoryLimit,
}

impl AnalysisLimitation {
    pub(crate) const fn code(self) -> &'static str {
        match self {
            Self::IncompleteHeader => "INCOMPLETE_HEADER_SAMPLE",
            Self::HeaderByteLimit => "HEADER_BYTE_LIMIT_REACHED",
            Self::RecordByteLimit => "RECORD_BYTE_LIMIT_REACHED",
            Self::ColumnLimit => "COLUMN_LIMIT_REACHED",
            Self::MemoryLimit => "ANALYSIS_MEMORY_LIMIT_REACHED",
            Self::CardinalityMemoryLimit => "CARDINALITY_MEMORY_LIMIT_REACHED",
        }
    }

    pub(crate) const fn message(self, parser: AnalysisParser) -> &'static str {
        match self {
            Self::IncompleteHeader => {
                "The header did not fit within the configured analysis scope."
            }
            Self::HeaderByteLimit if matches!(parser, AnalysisParser::CanonicalDelimited(_)) => {
                "The logical header record exceeded the header byte limit."
            }
            Self::HeaderByteLimit => "The first physical record exceeded the header byte limit.",
            Self::RecordByteLimit if matches!(parser, AnalysisParser::CanonicalDelimited(_)) => {
                "A logical data record exceeded the record byte limit."
            }
            Self::RecordByteLimit => "A physical data record exceeded the record byte limit.",
            Self::ColumnLimit => "The header exceeded the maximum supported analysis column count.",
            Self::MemoryLimit => "Analysis stopped at the internal memory accounting limit.",
            Self::CardinalityMemoryLimit => {
                "Cardinality tracking reached the shared analysis memory budget."
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AnalysisCoverage {
    pub(crate) source_size_bytes: u64,
    /// Source prefix that this consumer asked the analyzer to model.
    pub(crate) scope_size_bytes: u64,
    /// Bytes consumed by the bounded sampler.
    pub(crate) bytes_read: u64,
    /// Bytes whose selected record structure and fields were actually analyzed.
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
    pub(crate) parser: AnalysisParser,
    /// Exact first-record header width when the complete header was available.
    pub(crate) observed_column_count: Option<usize>,
    pub(crate) coverage: AnalysisCoverage,
    pub(crate) columns: Vec<ColumnFacts>,
    pub(crate) limitations: Vec<AnalysisLimitation>,
}
