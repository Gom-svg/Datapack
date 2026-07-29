#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveMode {
    CsvColumnarDictionary,
    RawZstd,
}

impl ArchiveMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::CsvColumnarDictionary => "CsvColumnarDictionary",
            Self::RawZstd => "RawZstd",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColumnStrategy {
    Dictionary,
    Plain,
    DeltaCandidate,
    Raw,
}

impl ColumnStrategy {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Dictionary => "Dictionary",
            Self::Plain => "Plain",
            Self::DeltaCandidate => "DeltaCandidate",
            Self::Raw => "Raw",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ColumnPlan {
    pub column_index: usize,
    pub column_name: String,
    pub strategy: ColumnStrategy,
    pub reason: String,
}

#[derive(Debug, Clone)]
pub struct CompressionPlan {
    pub archive_mode: ArchiveMode,
    pub columns: Vec<ColumnPlan>,
    pub estimated_savings_percent: f32,
    pub estimated_memory_mb: f32,
    pub planning_time_ms: u64,
    pub reason: String,
}
