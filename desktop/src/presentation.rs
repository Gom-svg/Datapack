//! Pure text presentation, shared by the native window and its tests.
use crate::adapter::Controller;
use crate::model::*;

#[derive(Debug, Default)]
pub struct View {
    pub title: String,
    pub body: String,
    pub details: String,
}

pub fn view(controller: &Controller) -> View {
    match &controller.state {
        State::Idle => View { title: "Make room. Keep every byte.".into(), body: "Select a flat-data file to get started.\r\n\r\nAnalyze to understand its structure and recommendation, then compress it into a DataPack archive.\r\n\r\nUse Validate & restore to check an archive or recover your original data.\r\n\r\nAll operations stay on this computer. No accounts, uploads, or telemetry.".into(), ..View::default() },
        State::FileSelected if controller.selected.as_ref().is_some_and(|f| f.kind == FileKind::Archive) => View { title: "Check integrity. Restore your data.".into(), body: "Validate this archive to inspect its version, expected restored size, and integrity.\r\n\r\nSupply the original file for an exact source comparison. V1 requires this comparison to establish byte integrity; V2 also includes SHA-256 checks.\r\n\r\nDecompression writes to a new file. Existing files are protected.".into(), ..View::default() },
        State::FileSelected => View { title: "Ready to analyze".into(), body: "Analyze to detect format and delimiter, inspect structure, and see the compression recommendation.\r\n\r\nAutomatic uses the engine's recommended method. Chunked is a bounded option for large or unsupported files.\r\n\r\nChoose a new destination filename. DataPack never silently replaces an existing file.".into(), ..View::default() },
        State::AnalysisReady => controller.analysis.as_ref().map(analysis_view).unwrap_or_default(),
        State::CompressComplete(result) => View {
            title: "Archive created".into(),
            body: format!("{}\r\n\r\nOriginal size: {} ({} bytes)\r\nArchive size: {} ({} bytes)\r\nStorage reduction: {}\r\nCompression ratio: {}\r\nCompression method: {} · Archive V{}\r\n\r\nWritten to: {}", result.integrity, bytes(result.input_bytes), result.input_bytes, bytes(result.output_bytes), result.output_bytes, reduction(result.input_bytes, result.output_bytes).map_or("Not applicable".into(), |v| format!("{v:.1}%")), ratio(result.input_bytes, result.output_bytes).map_or("Not applicable".into(), |v| format!("{v:.2}×")), result.method, result.version, result.output.display()),
            details: result.details.join("\r\n"),
        },
        State::DecompressComplete(result) => View { title: "File restored".into(), body: format!("{}\r\n\r\nRestored size: {} ({} bytes)\r\nCompression method: {} · Archive V{}\r\n\r\nWritten to: {}", result.integrity, bytes(result.output_bytes), result.output_bytes, result.method, result.version, result.output.display()), details: result.details.join("\r\n") },
        State::ValidationComplete(result) => View {
            title: if result.valid { "Valid — validation complete" } else { "INVALID — validation complete" }.into(),
            body: format!("{}\r\n{}\r\n\r\nArchive version: {}\r\nExpected restored size: {}\r\nChunks: {}\r\n\r\n{}", result.integrity, result.source_match, result.version.map_or("Unknown".into(), |v| v.to_string()), result.original_bytes.map_or("Unknown".into(), bytes), result.chunks.map_or("Not applicable".into(), |n| n.to_string()), result.diagnostics.join("\r\n")),
            details: result.checks.iter().map(|(name, state)| format!("{name}: {state}")).collect::<Vec<_>>().join("\r\n"),
        },
        State::Failed(problem) => View { title: "Operation failed".into(), body: format!("{}\r\n\r\n{}\r\n\r\nReview the file and destination, then retry. Existing destinations are never automatically replaced.", problem.message, problem.context), details: format!("Code: {}\r\nCategory: {}\r\nContext: {}", problem.code, problem.category, problem.context) },
        State::Cancelled(_) => View { title: "Operation cancelled".into(), body: "No successful result was reported. Uncommitted partial output was cleaned up.\r\n\r\nYou can retry or select another file.".into(), ..View::default() },
        State::Cancelling(_) => View { title: "Cancelling safely…".into(), body: "Waiting for the next safe engine checkpoint.\r\n\r\nA result already committed will still be reported as complete. Keep DataPack open while it finishes.".into(), ..View::default() },
        _ => View { title: controller.operation().map_or("Working", Operation::label).into(), body: "Your operation is running. Progress below comes directly from the engine.\r\n\r\nSome phases do not provide a total. Their progress is indeterminate.\r\n\r\nCancellation takes effect at the next safe checkpoint; structured operations may take longer to stop.".into(), ..View::default() },
    }
}

fn analysis_view(analysis: &Analysis) -> View {
    let warning = if analysis.dictionary_mib >= 512.0 {
        "\r\nThe dictionary estimate is substantial. Consider Chunked for bounded in-flight data."
    } else {
        ""
    };
    View {
        title: format!("{} recommended", analysis.recommendation),
        body: format!("{}\r\n\r\nDetected: {} · {} delimiter · {} columns\r\n{}: {} records, {} of {} analyzed\r\nEstimated structured savings: {:.1}%\r\nEstimated dictionary memory: {:.1} MiB{}\r\n\r\nEstimates are planner guidance, not guaranteed savings or a process memory limit.{}\r\n{}", analysis.reason, analysis.format, analysis.delimiter, analysis.columns.map_or("Unknown".into(), |n| n.to_string()), if analysis.sampled { "PARTIAL analysis" } else { "Full-file analysis" }, analysis.records, bytes(analysis.bytes_analyzed), bytes(analysis.source_bytes), analysis.estimated_reduction, analysis.dictionary_mib, warning, if analysis.sampled { " The remaining data may differ from this sample." } else { "" }, analysis.warnings.iter().take(4).cloned().collect::<Vec<_>>().join("\r\n")),
        details: analysis.details.join("\r\n"),
    }
}

pub fn progress_text(progress: Option<&Progress>) -> String {
    match progress {
        Some(p) => format!(
            "{}{}{}",
            p.phase,
            p.percent
                .map_or(" — progress total unavailable".into(), |percent| format!(
                    " — {percent:.0}% of this phase"
                )),
            if p.total.is_some() || p.bytes > 0 {
                format!(
                    " · {} processed{}",
                    bytes(p.bytes),
                    p.total
                        .map_or(String::new(), |total| format!(" of {}", bytes(total)))
                )
            } else {
                String::new()
            }
        ),
        None => "Preparing operation…".into(),
    }
}
