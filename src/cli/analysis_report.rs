use crate::analysis::{AnalysisParser, DatasetAnalysis, DelimitedFormat};
use crate::planning::ArchiveMode;

pub(super) fn print_planning_analysis(analysis: &DatasetAnalysis, include_plan: bool) {
    let facts = &analysis.facts;
    let plan = &analysis.plan;
    println!("DataPack Analysis: {}", facts.input_name);
    println!("Original size:     {} bytes", facts.source_size_bytes);
    if let AnalysisParser::CanonicalDelimited(format) = facts.parser {
        println!("Detected dialect:  {}", dialect_label(format));
    }
    println!("Sampled rows:      {}", facts.coverage.sampled_records);
    println!("Sampled bytes:     {}", facts.coverage.bytes_read);
    if !facts.limitations.is_empty() {
        let codes = facts
            .limitations
            .iter()
            .map(|limitation| limitation.code())
            .collect::<Vec<_>>()
            .join(", ");
        println!("Analysis limited:  {codes}");
    }
    if include_plan {
        println!("Archive mode:        {}", plan.archive_mode.as_str());
        println!(
            "Estimated savings:   {:.1}%",
            plan.estimated_savings_percent
        );
        println!("Estimated memory:    {:.1} MB", plan.estimated_memory_mb);
        println!("Planning time:       {} ms", plan.planning_time_ms);
        println!("Reason:              {}", plan.reason);
        println!();
    }
    println!(
        "{:<14} {:<15} {:>12} {:>9}  Reason",
        "Column", "Strategy", "Unique est", "Rep rate"
    );
    println!(
        "{:-<14} {:-<15} {:-<12} {:-<9}  {:-<24}",
        "", "", "", "", ""
    );
    for column in &analysis.columns {
        let column_name = match facts.parser {
            AnalysisParser::LegacyCsvPhysical => truncate_display(&column.column_name, 14),
            AnalysisParser::CanonicalDelimited(_) => truncate_control_safe(&column.column_name, 14),
        };
        let unique = if column.exceeded_cardinality {
            ">65535".to_string()
        } else {
            column.unique_count.to_string()
        };
        println!(
            "{:<14} {:<15} {:>12} {:>8.1}%  {}",
            column_name,
            column.recommended_strategy.as_str(),
            unique,
            column.repetition_rate * 100.0,
            format_args!(
                "{}; avg {:.1} bytes",
                column.reason, column.avg_value_len_bytes
            )
        );
    }
    if include_plan && plan.archive_mode == ArchiveMode::RawZstd {
        println!();
        println!("Fallback to RawZstd: {}", plan.reason);
    }
}

fn dialect_label(format: DelimitedFormat) -> &'static str {
    match format {
        DelimitedFormat::Comma => "CSV (,)",
        DelimitedFormat::Semicolon => "semicolon-delimited (;)",
        DelimitedFormat::Tab => "TSV (tab)",
        DelimitedFormat::Pipe => "PSV (|)",
    }
}

fn truncate_control_safe(value: &str, width: usize) -> String {
    let mut output = String::with_capacity(width);
    for character in value.chars() {
        let escaped: &[char] = match character {
            '\n' => &['\\', 'n'],
            '\r' => &['\\', 'r'],
            '\t' => &['\\', 't'],
            _ if character.is_control() => &['?'],
            _ => {
                if output.chars().count() >= width {
                    break;
                }
                output.push(character);
                continue;
            }
        };
        for escaped_character in escaped {
            if output.chars().count() >= width {
                return output;
            }
            output.push(*escaped_character);
        }
    }
    output
}

fn truncate_display(value: &str, width: usize) -> String {
    value.chars().take(width).collect()
}
