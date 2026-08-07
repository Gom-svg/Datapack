use std::fmt::Write as _;
use std::io::Write as _;
use std::path::PathBuf;

use crate::advisor::{self, AdvisorReportV1, EvidenceV1};
use crate::error::Result;

pub(super) fn run(input: PathBuf, sample_mb: u64, json: bool, pretty: bool) -> Result<()> {
    let report = advisor::advise_path(&input, sample_mb)?;
    if json {
        print_json(&report, pretty)
    } else {
        print_text(&report)
    }
}

fn print_json(report: &AdvisorReportV1, pretty: bool) -> Result<()> {
    let stdout = std::io::stdout();
    let mut output = stdout.lock();
    if pretty {
        serde_json::to_writer_pretty(&mut output, report)
    } else {
        serde_json::to_writer(&mut output, report)
    }
    .map_err(std::io::Error::other)?;
    output.write_all(b"\n")?;
    Ok(())
}

fn print_text(report: &AdvisorReportV1) -> Result<()> {
    let mut rendered = String::new();
    let _ = writeln!(rendered, "DataPack advisor");
    let _ = writeln!(
        rendered,
        "Analysis: {} ({}; {} scope)",
        report.analysis.status.as_str(),
        report.analysis.completeness.as_str(),
        report.analysis.scope.as_str()
    );
    if let Some(planner) = &report.analysis.planner {
        let _ = writeln!(
            rendered,
            "Planner: {} v{}",
            planner.policy.name, planner.policy.version
        );
        let _ = writeln!(rendered, "Selection scope: {}", planner.selection_scope);
        let _ = writeln!(
            rendered,
            "Archive selection: {}",
            planner.selected_archive_mode.as_str()
        );
    } else {
        let _ = writeln!(rendered, "Planner: unavailable");
    }
    let _ = writeln!(rendered);
    let _ = writeln!(rendered, "Recommendations");
    for recommendation in &report.recommendations {
        let _ = writeln!(
            rendered,
            "- {}: {}",
            recommendation.code, recommendation.message
        );
        let _ = writeln!(
            rendered,
            "  Evidence: {}",
            recommendation
                .evidence
                .iter()
                .map(evidence_text)
                .collect::<Vec<_>>()
                .join(", ")
        );
    }

    std::io::stdout().lock().write_all(rendered.as_bytes())?;
    Ok(())
}

fn evidence_text(evidence: &EvidenceV1) -> String {
    match evidence.column_index {
        Some(column_index) => format!(
            "{}:{}[column={column_index}]",
            evidence.source.as_str(),
            evidence.code
        ),
        None => format!("{}:{}", evidence.source.as_str(), evidence.code),
    }
}
