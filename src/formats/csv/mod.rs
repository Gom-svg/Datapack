use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

pub mod columnar;

use crate::encoding::dictionary::DictionaryEncoded;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NewlineStyle {
    Lf,
    Crlf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CsvAnalysis {
    pub delimiter: char,
    pub newline_style: NewlineStyle,
    pub has_headers: bool,
    pub total_rows: usize,
    pub columns: Vec<ColumnAnalysis>,
    pub dictionary_columns: Vec<DictionaryColumnPreview>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColumnAnalysis {
    pub column_name: String,
    pub total_rows: usize,
    pub unique_count: usize,
    pub repeated_value_count: usize,
    pub average_value_length: f64,
    pub suggested_encoding_strategy: EncodingStrategy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum EncodingStrategy {
    Dictionary,
    Delta,
    Rle,
    Plain,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DictionaryColumnPreview {
    pub column_name: String,
    pub unique_values: usize,
    pub encoded: DictionaryEncoded,
}

pub fn analyze(bytes: &[u8]) -> CsvAnalysis {
    let text = String::from_utf8_lossy(bytes);
    let delimiter = detect_delimiter(&text);
    let newline_style = detect_newline_style(bytes);
    let rows = parse_rows(&text, delimiter);
    let has_headers = detect_headers_from_rows(&rows);
    let names = column_names(&rows, has_headers);
    let data_start = usize::from(has_headers);
    let data_rows = rows.len().saturating_sub(data_start);
    let width = names.len();

    let mut columns = Vec::with_capacity(width);
    let dictionary_columns = Vec::new();

    for (column_index, name) in names.iter().enumerate() {
        let values: Vec<String> = rows[data_start..]
            .iter()
            .map(|row| row.get(column_index).cloned().unwrap_or_default())
            .collect();
        let stats = analyze_column(name, &values);

        columns.push(stats);
    }

    CsvAnalysis {
        delimiter,
        newline_style,
        has_headers,
        total_rows: data_rows,
        columns,
        dictionary_columns,
    }
}

pub fn detect_delimiter(text: &str) -> char {
    [',', ';', '\t', '|']
        .into_iter()
        .max_by_key(|candidate| delimiter_score(text, *candidate))
        .unwrap_or(',')
}

pub fn detect_newline_style(bytes: &[u8]) -> NewlineStyle {
    if bytes.windows(2).any(|pair| pair == b"\r\n") {
        NewlineStyle::Crlf
    } else {
        NewlineStyle::Lf
    }
}

pub fn detect_headers(text: &str, delimiter: char) -> bool {
    detect_headers_from_rows(&parse_rows(text, delimiter))
}

fn delimiter_score(text: &str, delimiter: char) -> usize {
    text.lines()
        .take(20)
        .map(|line| line.matches(delimiter).count())
        .filter(|count| *count > 0)
        .sum()
}

fn parse_rows(text: &str, delimiter: char) -> Vec<Vec<String>> {
    text.lines()
        .filter(|line| !line.is_empty())
        .map(|line| {
            line.trim_end_matches('\r')
                .split(delimiter)
                .map(ToString::to_string)
                .collect()
        })
        .collect()
}

fn detect_headers_from_rows(rows: &[Vec<String>]) -> bool {
    let Some(first) = rows.first() else {
        return false;
    };
    if first.is_empty() {
        return false;
    }

    let first_unique = first.iter().collect::<HashSet<_>>().len() == first.len();
    let first_looks_named = first.iter().all(|value| {
        !value.trim().is_empty()
            && value
                .chars()
                .any(|character| character.is_ascii_alphabetic() || character == '_')
            && !looks_numeric(value)
    });

    let second_has_data = rows.get(1).is_some_and(|second| {
        second
            .iter()
            .any(|value| looks_numeric(value) || value.len() > first.first().map_or(0, String::len))
    });

    first_unique && first_looks_named && second_has_data
}

fn column_names(rows: &[Vec<String>], has_headers: bool) -> Vec<String> {
    if has_headers {
        return rows.first().cloned().unwrap_or_default();
    }

    let width = rows.iter().map(Vec::len).max().unwrap_or(0);
    (1..=width).map(|index| format!("column_{index}")).collect()
}

fn analyze_column(name: &str, values: &[String]) -> ColumnAnalysis {
    let mut counts: HashMap<&str, usize> = HashMap::new();
    let total_length: usize = values.iter().map(String::len).sum();
    for value in values {
        *counts.entry(value).or_insert(0) += 1;
    }

    let unique_count = counts.len();
    let repeated_value_count = counts
        .values()
        .filter(|count| **count > 1)
        .map(|count| *count - 1)
        .sum();
    let average_value_length = if values.is_empty() {
        0.0
    } else {
        total_length as f64 / values.len() as f64
    };

    ColumnAnalysis {
        column_name: name.to_string(),
        total_rows: values.len(),
        unique_count,
        repeated_value_count,
        average_value_length,
        suggested_encoding_strategy: suggest_strategy(values, unique_count, repeated_value_count),
    }
}

fn suggest_strategy(
    values: &[String],
    unique_count: usize,
    repeated_value_count: usize,
) -> EncodingStrategy {
    if values.len() >= 2 && repeated_value_count > 0 && unique_count * 2 <= values.len() {
        EncodingStrategy::Dictionary
    } else if values.iter().all(|value| looks_numeric(value)) {
        EncodingStrategy::Delta
    } else {
        EncodingStrategy::Plain
    }
}

fn looks_numeric(value: &str) -> bool {
    let trimmed = value.trim();
    !trimmed.is_empty() && trimmed.parse::<f64>().is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_delimiter() {
        assert_eq!(detect_delimiter("a;b;c\n1;2;3\n"), ';');
        assert_eq!(detect_delimiter("a\tb\tc\n1\t2\t3\n"), '\t');
        assert_eq!(detect_delimiter("a|b|c\n1|2|3\n"), '|');
    }

    #[test]
    fn detects_newlines() {
        assert_eq!(detect_newline_style(b"a,b\n1,2\n"), NewlineStyle::Lf);
        assert_eq!(detect_newline_style(b"a,b\r\n1,2\r\n"), NewlineStyle::Crlf);
    }

    #[test]
    fn detects_headers() {
        assert!(detect_headers("id,name,status\n1,Ada,active\n", ','));
        assert!(!detect_headers("1,Ada,active\n2,Grace,active\n", ','));
    }
}
