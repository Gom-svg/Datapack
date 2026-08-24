use std::io::Write;
use std::path::PathBuf;

use crate::application::{self, CheckStatusV1, ValidateRequest, ValidationReportV1};
use crate::error::{DatapackError, Result};

use super::progress::TerminalProgressObserver;
use super::validation::{
    megabytes_to_bytes, optional_megabytes_to_bytes, validate_nonzero_megabyte_limit,
};
use super::ValidateOptions;

pub(super) fn run(archive: PathBuf, options: ValidateOptions) -> Result<()> {
    validate_nonzero_megabyte_limit("--max-memory-mb", options.max_memory_mb)?;
    if let Some(max_output_mb) = options.max_output_mb {
        validate_nonzero_megabyte_limit("--max-output-mb", max_output_mb)?;
    }
    if options.max_chunks == Some(0) {
        return Err(DatapackError::InvalidFormat(
            "--max-chunks must be greater than zero".to_string(),
        ));
    }

    let request = ValidateRequest {
        archive,
        against: options.against,
        max_output_bytes: optional_megabytes_to_bytes("--max-output-mb", options.max_output_mb)?,
        max_chunks: options.max_chunks,
        max_memory_bytes: megabytes_to_bytes("--max-memory-mb", options.max_memory_mb)?,
    };
    let mut observer = TerminalProgressObserver::new();
    let report = application::validate_with_progress(request, &mut observer)?;

    if options.json {
        print_json(&report, options.pretty)?;
    } else {
        print_text(&report)?;
    }

    if report.valid {
        Ok(())
    } else {
        Err(DatapackError::InvalidFormat(format!(
            "validation failed: {}",
            report
                .failure_summary()
                .unwrap_or("archive did not satisfy validation requirements")
        )))
    }
}

fn print_json(report: &ValidationReportV1, pretty: bool) -> Result<()> {
    let stdout = std::io::stdout();
    let mut output = stdout.lock();
    if pretty {
        serde_json::to_writer_pretty(&mut output, report)
    } else {
        serde_json::to_writer(&mut output, report)
    }
    .map_err(std::io::Error::other)?;
    output.write_all(b"\n")?;
    output.flush()?;
    Ok(())
}

fn print_text(report: &ValidationReportV1) -> Result<()> {
    let stdout = std::io::stdout();
    let mut output = stdout.lock();
    writeln!(
        output,
        "Archive validation: {}",
        if report.valid { "VALID" } else { "INVALID" }
    )?;
    writeln!(
        output,
        "Format: {}",
        report
            .archive
            .format
            .map_or("unknown", |format| format.as_str())
    )?;
    writeln!(
        output,
        "Version: {}",
        report
            .archive
            .version
            .map_or_else(|| "unknown".to_string(), |version| version.to_string())
    )?;
    writeln!(
        output,
        "Archive size: {} bytes",
        report.archive.archive_size_bytes
    )?;
    writeln!(
        output,
        "Original size: {}",
        report
            .archive
            .original_size_bytes
            .map_or_else(|| "unknown".to_string(), |size| format!("{size} bytes"))
    )?;
    writeln!(
        output,
        "Payload mode: {}",
        report
            .archive
            .payload_mode
            .map_or("unknown", |mode| mode.as_str())
    )?;
    if let Some(chunk_count) = report.archive.chunk_count {
        writeln!(output, "Chunks: {chunk_count}")?;
    }
    print_check(&mut output, "Header", report.checks.header)?;
    print_check(&mut output, "Metadata", report.checks.metadata)?;
    print_check(
        &mut output,
        "Payload structure",
        report.checks.payload_structure,
    )?;
    print_check(&mut output, "Decompression", report.checks.decompression)?;
    print_check(
        &mut output,
        "Restored length",
        report.checks.restored_length,
    )?;
    print_check(&mut output, "Chunk table", report.checks.chunk_table)?;
    print_check(
        &mut output,
        "Per-chunk SHA-256",
        report.checks.per_chunk_sha256,
    )?;
    print_check(&mut output, "Global SHA-256", report.checks.global_sha256)?;
    print_check(&mut output, "Trailing data", report.checks.trailing_data)?;
    writeln!(output, "Against source: {}", report.against.status.as_str())?;
    if let Some(source_size) = report.against.source_size_bytes {
        writeln!(output, "Against source size: {source_size} bytes")?;
    }
    for diagnostic in &report.diagnostics {
        writeln!(
            output,
            "Diagnostic {}: {}",
            diagnostic.code, diagnostic.message
        )?;
    }
    writeln!(output, "No restored output was created.")?;
    output.flush()?;
    Ok(())
}

fn print_check(output: &mut impl Write, label: &str, status: CheckStatusV1) -> std::io::Result<()> {
    let detail = match (label, status) {
        ("Per-chunk SHA-256" | "Global SHA-256", CheckStatusV1::NotAvailable) => {
            "not_available (not stored by .dpack v1)"
        }
        ("Trailing data", CheckStatusV1::NotAvailable) => {
            "not_available (v1 has no authenticated payload-end field)"
        }
        _ => status.as_str(),
    };
    writeln!(output, "{label}: {detail}")
}
