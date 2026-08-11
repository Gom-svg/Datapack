use crate::application::{self, CompareRequest, ComparisonReportV1, CompetitorReportV1};
use crate::error::{DatapackError, Result};
use std::fmt::Write as _;

pub(super) fn run(request: CompareRequest, json: bool, pretty: bool) -> Result<()> {
    let report = application::compare(request)?;
    if json {
        print_json(&report, pretty)
    } else {
        print_text(&report);
        Ok(())
    }
}

fn print_json(report: &ComparisonReportV1, pretty: bool) -> Result<()> {
    let output = if pretty {
        serde_json::to_string_pretty(report)
    } else {
        serde_json::to_string(report)
    }
    .map_err(|error| {
        DatapackError::InvalidFormat(format!("could not serialize comparison JSON v1: {error}"))
    })?;
    println!("{output}");
    Ok(())
}

fn print_text(report: &ComparisonReportV1) {
    let mut output = String::new();
    let _ = writeln!(output, "DataPack comparison");
    let _ = writeln!(output, "Mode: {}", report.mode);
    let _ = writeln!(output, "Scope: {}", report.scope.kind);
    let _ = writeln!(
        output,
        "Compared bytes: {} of {}",
        report.scope.compared_size_bytes, report.scope.source_size_bytes
    );
    let _ = writeln!(
        output,
        "Runs: {} (median, file-to-file)",
        report.methodology.runs
    );
    let _ = writeln!(output);
    let _ = writeln!(output, "DataPack v1");
    let _ = writeln!(
        output,
        "  selected mode: {}",
        report.datapack.selected_mode.unwrap_or("not_available")
    );
    write_competitor_metrics(&mut output, &report.datapack);
    let _ = writeln!(
        output,
        "Standalone zstd (level {})",
        report.methodology.zstd_level
    );
    write_competitor_metrics(&mut output, &report.standalone_zstd);
    let _ = writeln!(output, "Measured winners");
    let _ = writeln!(
        output,
        "  best storage ratio: {}",
        report.winners.best_storage_ratio
    );
    let _ = writeln!(
        output,
        "  fastest compression: {}",
        report.winners.fastest_compression
    );
    let _ = writeln!(
        output,
        "  fastest decompression: {}",
        report.winners.fastest_decompression
    );
    if !report.limitations.is_empty() {
        let _ = writeln!(output, "Limitations");
        for limitation in &report.limitations {
            let _ = writeln!(output, "  {}: {}", limitation.code, limitation.message);
        }
    }
    print!("{output}");
}

fn write_competitor_metrics(output: &mut String, competitor: &CompetitorReportV1) {
    let _ = writeln!(
        output,
        "  archive bytes: {}",
        competitor.artifact_size_bytes
    );
    let _ = writeln!(
        output,
        "  compression ratio: {:.3}x",
        competitor.compression_ratio
    );
    let _ = writeln!(
        output,
        "  compression median: {:.3} ms ({})",
        competitor.compression.median_ms,
        throughput_text(competitor.compression.throughput_mib_per_second)
    );
    let _ = writeln!(
        output,
        "  decompression median: {:.3} ms ({})",
        competitor.decompression.median_ms,
        throughput_text(competitor.decompression.throughput_mib_per_second)
    );
    let _ = writeln!(
        output,
        "  validation: {} (SHA-256 match)",
        competitor.validation.status
    );
    let _ = writeln!(output);
}

fn throughput_text(value: Option<f64>) -> String {
    value.map_or_else(
        || "not available".to_string(),
        |throughput| format!("{throughput:.3} MiB/s"),
    )
}
