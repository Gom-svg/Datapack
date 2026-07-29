use std::collections::HashMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TxtAnalysis {
    pub total_lines: usize,
    pub repeated_lines: usize,
    pub repeated_phrases: Vec<PhraseCount>,
    pub compression_note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhraseCount {
    pub phrase: String,
    pub count: usize,
}

pub fn analyze(bytes: &[u8]) -> TxtAnalysis {
    let text = String::from_utf8_lossy(bytes);
    let lines: Vec<&str> = text.lines().collect();
    let repeated_lines = repeated_count(lines.iter().copied());
    let repeated_phrases = repeated_phrases(&text);

    TxtAnalysis {
        total_lines: lines.len(),
        repeated_lines,
        repeated_phrases,
        compression_note:
            "v0.1 stores TXT/log payloads as zstd-compressed raw bytes for exact reconstruction"
                .to_string(),
    }
}

fn repeated_count<'a>(values: impl Iterator<Item = &'a str>) -> usize {
    let mut counts = HashMap::new();
    for value in values {
        *counts.entry(value).or_insert(0usize) += 1;
    }
    counts
        .values()
        .filter(|count| **count > 1)
        .map(|count| *count - 1)
        .sum()
}

fn repeated_phrases(text: &str) -> Vec<PhraseCount> {
    let mut counts = HashMap::new();
    for line in text.lines() {
        let words: Vec<&str> = line.split_whitespace().collect();
        for window in words.windows(3) {
            let phrase = window.join(" ");
            *counts.entry(phrase).or_insert(0usize) += 1;
        }
    }

    let mut phrases: Vec<_> = counts
        .into_iter()
        .filter(|(_, count)| *count > 1)
        .map(|(phrase, count)| PhraseCount { phrase, count })
        .collect();
    phrases.sort_by(|left, right| {
        right
            .count
            .cmp(&left.count)
            .then(left.phrase.cmp(&right.phrase))
    });
    phrases.truncate(10);
    phrases
}
