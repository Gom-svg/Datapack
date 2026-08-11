use std::path::PathBuf;

use crate::benchmark::{
    self, BenchmarkArtifacts, BenchmarkExecution, BenchmarkRequest, ProfileTimings,
};
use crate::error::Result;

use super::BenchmarkOptions;

mod report;

#[cfg(test)]
mod tests;

pub(super) fn run(input: PathBuf, options: BenchmarkOptions) -> Result<()> {
    let json = options.json;
    let profile = options.profile;
    let request = BenchmarkRequest {
        input,
        keep_temp: options.keep_temp,
        quick: options.quick,
        runs: options.runs,
        profile: options.profile,
        chunked: options.chunked,
        chunk_size_mb: options.chunk_size_mb,
        threads: options.threads,
        max_in_flight_chunks: options.max_in_flight_chunks,
        backend: options.backend.map(|backend| backend.storage_backend()),
        adaptive_level: options.adaptive_level,
        no_zstd_baseline: options.no_zstd_baseline,
        no_roundtrip: options.no_roundtrip,
        no_hash: options.no_hash,
        estimate_only: options.estimate_only,
        max_input_mb: options.max_input_mb,
    };
    let execution = benchmark::execute_with_events(request, &mut report::print_event)?;
    render_execution(execution, json, profile);
    Ok(())
}

fn render_execution(execution: BenchmarkExecution, json: bool, profile: bool) {
    match execution {
        BenchmarkExecution::EstimateOnly {
            analysis,
            partial_reasons,
            total_elapsed_ms,
            profile_timings,
        } => {
            if json {
                report::print_estimate_only_json(&analysis, &partial_reasons, total_elapsed_ms);
            } else {
                report::print_estimate_only_table(&analysis, &partial_reasons, total_elapsed_ms);
            }
            print_profile_if_requested(profile, &profile_timings);
        }
        BenchmarkExecution::Measured {
            metrics,
            profile_timings,
            mode_label,
            artifacts,
        } => {
            if json {
                report::print_benchmark_json(&metrics);
            } else {
                report::print_benchmark_table(&metrics);
                println!("mode                     {mode_label}");
                if let Some(error) = &metrics.columnar_candidate_error {
                    eprintln!("columnar_candidate_error = {error:?}");
                }
                print_artifacts(artifacts);
            }
            print_profile_if_requested(profile, &profile_timings);
        }
    }
}

fn print_profile_if_requested(profile: bool, timings: &ProfileTimings) {
    if profile {
        report::print_profile_timings(timings);
    }
}

fn print_artifacts(artifacts: BenchmarkArtifacts) {
    if let Some(path) = artifacts.datapack {
        println!("temp_artifact             {}", path.display());
    }
    if let Some(path) = artifacts.zstd {
        println!("zstd_temp_artifact        {}", path.display());
    }
    if let Some(path) = artifacts.chunked {
        println!("chunked_temp_artifact     {}", path.display());
    }
}
