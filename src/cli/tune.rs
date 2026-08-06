use std::path::PathBuf;

use crate::error::{DatapackError, Result};
use crate::storage;
use crate::tuning;

pub(super) fn parse_tune_chunk_sizes(value: &str) -> Result<Vec<u64>> {
    let values = parse_comma_list(value, "--chunk-sizes-mb")?;
    values
        .into_iter()
        .map(|value| {
            let parsed = value.parse::<u64>().map_err(|_| {
                DatapackError::InvalidFormat(format!(
                    "invalid --chunk-sizes-mb value '{value}'; expected positive MiB integers"
                ))
            })?;
            storage::chunked::chunk_size_mb_to_bytes(parsed)?;
            Ok(parsed)
        })
        .collect()
}

pub(super) fn parse_tune_threads(value: &str) -> Result<Vec<usize>> {
    let values = parse_comma_list(value, "--threads-list")?;
    values
        .into_iter()
        .map(|value| {
            if value.eq_ignore_ascii_case("max") {
                return Ok(storage::chunked::default_thread_count());
            }
            let parsed = value.parse::<usize>().map_err(|_| {
                DatapackError::InvalidFormat(format!(
                    "invalid --threads-list value '{value}'; expected positive integers or max"
                ))
            })?;
            if parsed == 0 {
                return Err(DatapackError::InvalidFormat(
                    "--threads-list values must be greater than zero".to_string(),
                ));
            }
            Ok(parsed)
        })
        .collect()
}

fn parse_comma_list<'a>(value: &'a str, flag: &str) -> Result<Vec<&'a str>> {
    let values: Vec<_> = value.split(',').map(str::trim).collect();
    if values.is_empty() || values.iter().any(|value| value.is_empty()) {
        return Err(DatapackError::InvalidFormat(format!(
            "{flag} must be a comma-separated list without empty values"
        )));
    }
    Ok(values)
}

pub(super) fn run(input: PathBuf, options: tuning::TuneOptions) -> Result<()> {
    eprintln!(
        "experimental tune: recommendations are hardware- and dataset-specific; defaults will not be changed automatically."
    );
    let summary = tuning::tune(&input, options)?;
    println!("Tune report: {}", summary.report_path.display());
    println!("Input size: {} bytes", summary.input_size_bytes);
    println!(
        "Measured input: {} bytes ({})",
        summary.measured_input_size_bytes,
        if summary.input_sampled {
            "sampled"
        } else {
            "full"
        }
    );
    println!(
        "Runs: {} successful, {} failed",
        summary.successful_runs, summary.failed_runs
    );
    print_tune_recommendation("Best throughput", summary.best_throughput.as_ref());
    print_tune_recommendation("Best compression ratio", summary.best_ratio.as_ref());
    print_tune_recommendation("Balanced", summary.balanced.as_ref());
    if !summary.temp_paths.is_empty() {
        println!("Preserved temporary files:");
        for path in &summary.temp_paths {
            println!("  {}", path.display());
        }
    }
    if summary.successful_runs == 0 {
        return Err(DatapackError::InvalidFormat(format!(
            "all tune runs failed; inspect {}",
            summary.report_path.display()
        )));
    }
    Ok(())
}

fn print_tune_recommendation(label: &str, recommendation: Option<&tuning::TuneRecommendation>) {
    println!("{label}:");
    match recommendation {
        Some(value) => {
            println!("  backend={}", value.backend.as_str());
            println!("  chunk_size_mb={}", value.chunk_size_mb);
            println!("  threads={}", value.threads);
            println!("  max_in_flight_chunks={}", value.max_in_flight_chunks);
            println!("  throughput={:.3} MB/s", value.compression_mb_per_sec);
            println!("  ratio={:.4}x", value.compression_ratio);
            println!(
                "  peak_memory_estimate_mb={:.1}",
                value.peak_memory_estimate_mb
            );
            if let Some(score) = value.balanced_score {
                println!("  balanced_score={score:.4}");
            }
        }
        None => println!("  unavailable (no successful runs)"),
    }
}
