use std::cmp::Ordering;
use std::time::Duration;

use serde::Serialize;

pub(super) const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Serialize)]
pub(crate) struct ComparisonReportV1 {
    pub(crate) schema_version: u32,
    pub(crate) report_type: &'static str,
    pub(crate) mode: &'static str,
    pub(crate) scope: ComparisonScopeV1,
    pub(crate) methodology: ComparisonMethodologyV1,
    pub(crate) datapack: CompetitorReportV1,
    pub(crate) standalone_zstd: CompetitorReportV1,
    pub(crate) winners: ComparisonWinnersV1,
    pub(crate) limitations: Vec<ComparisonLimitationV1>,
}

#[derive(Debug, Serialize)]
pub(crate) struct ComparisonScopeV1 {
    pub(crate) kind: &'static str,
    pub(crate) source_size_bytes: u64,
    pub(crate) compared_size_bytes: u64,
    pub(crate) prefix_limited: bool,
}

#[derive(Debug, Serialize)]
pub(crate) struct ComparisonMethodologyV1 {
    pub(crate) runs: usize,
    pub(crate) aggregation: &'static str,
    pub(crate) timing_boundary: &'static str,
    pub(crate) planning_included: bool,
    pub(crate) planning_time_ms: f64,
    pub(crate) zstd_level: i32,
    pub(crate) artifact_stability: &'static str,
    pub(crate) validation: &'static str,
}

#[derive(Debug, Serialize)]
pub(crate) struct CompetitorReportV1 {
    pub(crate) artifact_format: &'static str,
    pub(crate) selected_mode: Option<&'static str>,
    pub(crate) artifact_size_bytes: u64,
    pub(crate) compression_ratio: f64,
    pub(crate) compression: TimingReportV1,
    pub(crate) decompression: TimingReportV1,
    pub(crate) validation: CompetitorValidationV1,
}

#[derive(Debug, Serialize)]
pub(crate) struct TimingReportV1 {
    pub(crate) samples_ms: Vec<f64>,
    pub(crate) median_ms: f64,
    pub(crate) throughput_mib_per_second: Option<f64>,
}

#[derive(Debug, Serialize)]
pub(crate) struct CompetitorValidationV1 {
    pub(crate) status: &'static str,
    pub(crate) restored_size_bytes: u64,
    pub(crate) sha256_match: bool,
}

#[derive(Debug, Serialize)]
pub(crate) struct ComparisonWinnersV1 {
    pub(crate) best_storage_ratio: &'static str,
    pub(crate) fastest_compression: &'static str,
    pub(crate) fastest_decompression: &'static str,
}

#[derive(Debug, Serialize)]
pub(crate) struct ComparisonLimitationV1 {
    pub(crate) code: &'static str,
    pub(crate) message: &'static str,
}

impl ComparisonLimitationV1 {
    pub(super) const fn new(code: &'static str, message: &'static str) -> Self {
        Self { code, message }
    }
}

pub(super) fn timing_report(input_bytes: u64, samples: &[Duration]) -> TimingReportV1 {
    let median = median_duration(samples);
    TimingReportV1 {
        samples_ms: samples.iter().copied().map(duration_ms).collect(),
        median_ms: duration_ms(median),
        throughput_mib_per_second: throughput_mib_per_second(input_bytes, median),
    }
}

pub(super) fn comparison_winners(
    input_bytes: u64,
    datapack_size: u64,
    zstd_size: u64,
    datapack_compression: Duration,
    zstd_compression: Duration,
    datapack_decompression: Duration,
    zstd_decompression: Duration,
) -> ComparisonWinnersV1 {
    ComparisonWinnersV1 {
        best_storage_ratio: if input_bytes == 0 {
            "tie"
        } else {
            winner(datapack_size.cmp(&zstd_size))
        },
        fastest_compression: winner(datapack_compression.cmp(&zstd_compression)),
        fastest_decompression: winner(datapack_decompression.cmp(&zstd_decompression)),
    }
}

fn winner(ordering: Ordering) -> &'static str {
    match ordering {
        Ordering::Less => "datapack",
        Ordering::Equal => "tie",
        Ordering::Greater => "standalone_zstd",
    }
}

pub(super) fn median_duration(samples: &[Duration]) -> Duration {
    if samples.is_empty() {
        return Duration::ZERO;
    }
    let mut sorted = samples.to_vec();
    sorted.sort_unstable();
    let middle = sorted.len() / 2;
    if sorted.len() % 2 == 1 {
        sorted[middle]
    } else {
        let lower = sorted[middle - 1];
        let upper = sorted[middle];
        upper
            .checked_sub(lower)
            .and_then(|span| lower.checked_add(span / 2))
            .unwrap_or(lower)
    }
}

pub(super) fn compression_ratio(input_bytes: u64, artifact_bytes: u64) -> f64 {
    if artifact_bytes == 0 {
        0.0
    } else {
        input_bytes as f64 / artifact_bytes as f64
    }
}

fn duration_ms(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1_000.0
}

fn throughput_mib_per_second(input_bytes: u64, duration: Duration) -> Option<f64> {
    let seconds = duration.as_secs_f64();
    if input_bytes == 0 || seconds == 0.0 {
        None
    } else {
        Some(input_bytes as f64 / 1_048_576.0 / seconds)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn median_uses_middle_value_for_odd_run_count() {
        let values = [
            Duration::from_millis(9),
            Duration::from_millis(1),
            Duration::from_millis(5),
        ];
        assert_eq!(median_duration(&values), Duration::from_millis(5));
    }

    #[test]
    fn median_averages_middle_values_for_even_run_count() {
        let values = [
            Duration::from_millis(10),
            Duration::from_millis(2),
            Duration::from_millis(6),
            Duration::from_millis(4),
        ];
        assert_eq!(median_duration(&values), Duration::from_millis(5));
    }

    #[test]
    fn winner_dimensions_are_independent_and_support_ties() {
        let winners = comparison_winners(
            1_000,
            100,
            120,
            Duration::from_millis(7),
            Duration::from_millis(5),
            Duration::from_millis(3),
            Duration::from_millis(3),
        );
        assert_eq!(winners.best_storage_ratio, "datapack");
        assert_eq!(winners.fastest_compression, "standalone_zstd");
        assert_eq!(winners.fastest_decompression, "tie");
    }

    #[test]
    fn empty_input_has_no_storage_ratio_winner() {
        let winners = comparison_winners(
            0,
            53,
            9,
            Duration::from_millis(1),
            Duration::from_millis(2),
            Duration::from_millis(1),
            Duration::from_millis(2),
        );
        assert_eq!(winners.best_storage_ratio, "tie");
    }

    #[test]
    fn zero_byte_throughput_is_unavailable_and_ratio_is_finite() {
        let timing = timing_report(0, &[Duration::from_millis(1)]);
        assert_eq!(timing.throughput_mib_per_second, None);
        assert_eq!(compression_ratio(0, 9), 0.0);
    }
}
