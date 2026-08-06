use std::io::Write;

use crate::analysis::{self, DatasetAnalysis};
use crate::error::Result;

pub(super) fn print_analysis_v1(analysis: &DatasetAnalysis, pretty: bool) -> Result<()> {
    let report = analysis::build_report_v1(analysis)?;
    let stdout = std::io::stdout();
    let mut output = stdout.lock();
    if pretty {
        serde_json::to_writer_pretty(&mut output, &report)
    } else {
        serde_json::to_writer(&mut output, &report)
    }
    .map_err(std::io::Error::other)?;
    output.write_all(b"\n")?;
    Ok(())
}
