use std::collections::HashMap;
use std::fs::File;
use std::hash::{Hash, Hasher};
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::time::Instant;

use crate::error::{DatapackError, Result};

pub mod plan;

pub use plan::{ArchiveMode, ColumnPlan, ColumnStrategy, CompressionPlan};

const DEFAULT_SAMPLE_MB: u64 = 64;
const MIN_SAMPLE_MB: u64 = 1;
const MAX_SAMPLE_MB: u64 = 2048;
const DEFAULT_MAX_SAMPLE_ROWS: u64 = 10_000;
#[allow(dead_code)]
const FUTURE_MAX_SAMPLE_ROWS: u64 = 100_000;
const READER_BUFFER_BYTES: usize = 256 * 1024;
const UNIQUE_TRACKING_LIMIT: usize = 8_192;

#[derive(Debug, Clone)]
pub struct SampleConfig {
    pub max_bytes: u64,
    pub max_rows: u64,
}

impl SampleConfig {
    pub fn from_sample_mb(sample_mb: u64) -> Result<Self> {
        if !(MIN_SAMPLE_MB..=MAX_SAMPLE_MB).contains(&sample_mb) {
            return Err(DatapackError::InvalidFormat(format!(
                "--sample-mb must be between {MIN_SAMPLE_MB} and {MAX_SAMPLE_MB}"
            )));
        }
        Ok(Self {
            max_bytes: sample_mb * 1024 * 1024,
            max_rows: DEFAULT_MAX_SAMPLE_ROWS,
        })
    }
}

impl Default for SampleConfig {
    fn default() -> Self {
        Self {
            max_bytes: DEFAULT_SAMPLE_MB * 1024 * 1024,
            max_rows: DEFAULT_MAX_SAMPLE_ROWS,
        }
    }
}

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

#[derive(Debug, Clone)]
pub struct SampleAnalysis {
    pub input_name: String,
    pub total_file_size: u64,
    pub sampled_bytes: u64,
    pub sampled_rows: u64,
    pub columns: Vec<ColumnProfile>,
    pub plan: CompressionPlan,
}

#[derive(Debug, Clone)]
pub struct SampleAnalyzer {
    config: SampleConfig,
}

impl SampleAnalyzer {
    pub fn new(config: SampleConfig) -> Self {
        Self { config }
    }

    pub fn analyze_path(&self, path: &Path) -> Result<SampleAnalysis> {
        validate_csv_prefix(path)?;
        let started = Instant::now();
        let total_file_size = std::fs::metadata(path)
            .map_err(|_| DatapackError::AnalyzeRead(path.display().to_string()))?
            .len();
        let file =
            File::open(path).map_err(|_| DatapackError::AnalyzeRead(path.display().to_string()))?;
        let mut reader = BufReader::with_capacity(READER_BUFFER_BYTES, file);
        let mut line = String::new();
        let mut sampled_bytes = 0u64;

        let header_bytes = reader.read_line(&mut line)?;
        if header_bytes == 0 {
            return Err(DatapackError::InvalidCsv("empty file".to_string()));
        }
        sampled_bytes += header_bytes as u64;
        let mut header_fields = Vec::new();
        parse_csv_record_refs(trim_newline(&line), &mut header_fields)?;
        let headers: Vec<String> = header_fields
            .iter()
            .map(|field| trim_quotes(field).to_string())
            .collect();
        if headers.len() < 2 {
            return Err(DatapackError::InvalidCsv(
                "expected at least two CSV columns".to_string(),
            ));
        }

        let mut states: Vec<ColumnState> =
            headers.iter().map(|name| ColumnState::new(name)).collect();
        let mut sampled_rows = 0u64;

        while sampled_rows < self.config.max_rows && sampled_bytes < self.config.max_bytes {
            line.clear();
            let bytes_read = reader.read_line(&mut line)?;
            if bytes_read == 0 {
                break;
            }
            sampled_bytes += bytes_read as u64;
            if sampled_bytes > self.config.max_bytes && sampled_rows > 0 {
                break;
            }
            let trimmed = trim_newline(&line);
            if trimmed.is_empty() {
                continue;
            }
            let mut fields = Vec::with_capacity(states.len());
            parse_csv_record_refs(trimmed, &mut fields)?;
            if fields.len() != states.len() {
                return Err(DatapackError::InvalidCsv(format!(
                    "row has {} columns, expected {}",
                    fields.len(),
                    states.len()
                )));
            }
            sampled_rows += 1;
            for (state, field) in states.iter_mut().zip(fields.iter().copied()) {
                state.observe(field);
            }
        }

        let profiles = build_profiles(&states, sampled_rows, sampled_bytes, total_file_size);
        let mut plan = build_plan(&profiles, started.elapsed().as_millis() as u64);
        plan.planning_time_ms = started.elapsed().as_millis() as u64;

        Ok(SampleAnalysis {
            input_name: path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("input")
                .to_string(),
            total_file_size,
            sampled_bytes,
            sampled_rows,
            columns: profiles,
            plan,
        })
    }
}

fn build_profiles(
    states: &[ColumnState],
    sampled_rows: u64,
    sampled_bytes: u64,
    total_file_size: u64,
) -> Vec<ColumnProfile> {
    let row_scale = if sampled_rows == 0 {
        1.0
    } else {
        sampled_rows as f64
    };
    let byte_scale = if sampled_bytes == 0 {
        1.0
    } else {
        total_file_size as f64 / sampled_bytes as f64
    };

    states
        .iter()
        .enumerate()
        .map(|(index, state)| {
            let unique_count = state.unique_count(sampled_rows);
            let repetition_rate = if sampled_rows == 0 || state.exceeded_cardinality {
                0.0
            } else {
                (1.0 - unique_count as f32 / sampled_rows as f32).clamp(0.0, 1.0)
            };
            let projected_unique =
                ((unique_count as f64 * byte_scale).ceil() as u64).max(unique_count);
            let avg_len = state.mean_len as f32;
            let dict_size = projected_unique.saturating_mul(avg_len.ceil() as u64 + 4);
            let id_width = if projected_unique <= 255 {
                1
            } else if projected_unique <= 65_535 {
                2
            } else {
                4
            };
            let projected_rows = (row_scale * byte_scale).ceil() as u64;
            let encoded_size = dict_size.saturating_add(projected_rows.saturating_mul(id_width));
            let raw_size = ((state.total_value_bytes as f64 * byte_scale).ceil() as u64)
                .saturating_add(projected_rows);
            let (strategy, reason) = recommend_strategy(state, unique_count, repetition_rate);

            ColumnProfile {
                column_index: index,
                column_name: state.name.clone(),
                unique_count,
                repetition_rate,
                avg_value_len_bytes: avg_len,
                estimated_dict_size_kb: dict_size.saturating_add(1023) / 1024,
                estimated_encoded_size: encoded_size,
                estimated_raw_size: raw_size,
                recommended_strategy: strategy,
                reason,
                exceeded_cardinality: state.exceeded_cardinality,
            }
        })
        .collect()
}

pub fn build_plan(columns: &[ColumnProfile], planning_time_ms: u64) -> CompressionPlan {
    let raw_columns = columns
        .iter()
        .filter(|column| column.repetition_rate < 0.10)
        .count();
    let encoded_size: u64 = columns
        .iter()
        .map(|column| column.estimated_encoded_size)
        .sum();
    let raw_size: u64 = columns.iter().map(|column| column.estimated_raw_size).sum();
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

    let (archive_mode, reason) = if !columns.is_empty() && raw_columns * 100 >= columns.len() * 70 {
        (
            ArchiveMode::RawZstd,
            "Insufficient repetition across majority of columns for dictionary gains.".to_string(),
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
    state: &ColumnState,
    unique_count: u64,
    repetition_rate: f32,
) -> (ColumnStrategy, String) {
    if state.exceeded_cardinality || unique_count > 65_535 {
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
    if state.looks_numeric_or_date() {
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

fn validate_csv_prefix(path: &Path) -> Result<()> {
    let mut file =
        File::open(path).map_err(|_| DatapackError::AnalyzeRead(path.display().to_string()))?;
    let mut buffer = [0u8; 4096];
    let bytes_read = std::io::Read::read(&mut file, &mut buffer)?;
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

fn trim_newline(line: &str) -> &str {
    line.trim_end_matches('\n').trim_end_matches('\r')
}

fn parse_csv_record_refs<'a>(line: &'a str, fields: &mut Vec<&'a str>) -> Result<()> {
    fields.clear();
    let mut chars = line.char_indices().peekable();
    let mut field_start = 0usize;
    let mut quoted = false;
    while let Some((index, ch)) = chars.next() {
        match ch {
            '"' if quoted && chars.peek().is_some_and(|(_, next)| *next == '"') => {
                chars.next();
            }
            '"' => quoted = !quoted,
            ',' if !quoted => {
                fields.push(&line[field_start..index]);
                field_start = index + ch.len_utf8();
            }
            _ => {}
        }
    }
    if quoted {
        return Err(DatapackError::InvalidCsv(
            "unterminated quoted field".to_string(),
        ));
    }
    fields.push(&line[field_start..]);
    Ok(())
}

fn trim_quotes(value: &str) -> &str {
    value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .unwrap_or(value)
}

fn truncate_reason(reason: &str) -> String {
    if reason.len() <= 120 {
        reason.to_string()
    } else {
        reason[..120].to_string()
    }
}

#[derive(Debug, Clone)]
struct ColumnState {
    name: String,
    unique_hashes: HashMap<u64, ()>,
    mean_len: f64,
    len_count: u64,
    total_value_bytes: u64,
    numeric_success: u64,
    exceeded_cardinality: bool,
}

impl ColumnState {
    fn new(name: &str) -> Self {
        Self {
            name: name.to_owned(),
            unique_hashes: HashMap::new(),
            mean_len: 0.0,
            len_count: 0,
            total_value_bytes: 0,
            numeric_success: 0,
            exceeded_cardinality: false,
        }
    }

    fn observe(&mut self, value: &str) {
        self.len_count += 1;
        let len = value.len() as f64;
        self.mean_len += (len - self.mean_len) / self.len_count as f64;
        self.total_value_bytes = self.total_value_bytes.saturating_add(value.len() as u64);
        if value.parse::<i64>().is_ok() || value.parse::<f64>().is_ok() {
            self.numeric_success += 1;
        }
        if !self.exceeded_cardinality {
            let hash = stable_hash(&value);
            if self.unique_hashes.len() < UNIQUE_TRACKING_LIMIT {
                self.unique_hashes.insert(hash, ());
            } else if !self.unique_hashes.contains_key(&hash) {
                self.exceeded_cardinality = true;
                self.unique_hashes.clear();
            }
        }
    }

    fn unique_count(&self, sampled_rows: u64) -> u64 {
        if self.exceeded_cardinality {
            sampled_rows.max((UNIQUE_TRACKING_LIMIT + 1) as u64)
        } else {
            self.unique_hashes.len() as u64
        }
    }

    fn looks_numeric_or_date(&self) -> bool {
        let lower = self.name.to_ascii_lowercase();
        let name_match = [
            "date", "time", "ts", "amount", "price", "qty", "count", "id",
        ]
        .iter()
        .any(|needle| lower.contains(needle));
        let numeric_rate = if self.len_count == 0 {
            0.0
        } else {
            self.numeric_success as f32 / self.len_count as f32
        };
        name_match || numeric_rate >= 0.80
    }
}

fn stable_hash<T: Hash>(value: &T) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

pub fn analyze_path(path: &Path, sample_mb: u64) -> Result<SampleAnalysis> {
    SampleAnalyzer::new(SampleConfig::from_sample_mb(sample_mb)?).analyze_path(path)
}

#[cfg(test)]
mod tests;
