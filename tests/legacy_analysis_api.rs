use std::path::Path;

use datapack::analysis::analyze_bytes;
use datapack::formats::csv::{detect_delimiter, ColumnAnalysis, EncodingStrategy, NewlineStyle};
use datapack::metadata::{DpackMetadata, ExtensionPoint, FileType, PayloadKind, CURRENT_VERSION};

const TXT_COMPRESSION_NOTE: &str =
    "v0.1 stores TXT/log payloads as zstd-compressed raw bytes for exact reconstruction";

#[test]
fn public_analysis_signature_and_csv_results_are_stable() {
    let analyze: fn(&Path, &[u8]) -> DpackMetadata = analyze_bytes;
    let bytes = b"id,status\r\n1,active\r\n2,active\r\n";

    let metadata = analyze(Path::new("sample.CSV"), bytes);

    assert_common_metadata(&metadata, FileType::Csv, bytes.len());
    assert!(metadata.txt.is_none());
    let csv = metadata.csv.as_ref().expect("CSV analysis");
    assert_eq!(csv.delimiter, ',');
    assert_eq!(csv.newline_style, NewlineStyle::Crlf);
    assert!(csv.has_headers);
    assert_eq!(csv.total_rows, 2);
    assert!(csv.dictionary_columns.is_empty());
    assert_eq!(csv.columns.len(), 2);
    assert_column(&csv.columns[0], "id", 2, 2, 0, 1.0, EncodingStrategy::Delta);
    assert_column(
        &csv.columns[1],
        "status",
        2,
        1,
        1,
        6.0,
        EncodingStrategy::Dictionary,
    );
}

#[test]
fn public_delimiter_ties_and_no_evidence_keep_legacy_last_candidate_behavior() {
    let tied = "a,b|c\n1,2|3\n";
    assert_eq!(detect_delimiter(tied), '|');
    assert_eq!(
        analyze_bytes(Path::new("tied.csv"), tied.as_bytes())
            .csv
            .expect("CSV analysis")
            .delimiter,
        '|'
    );

    let no_evidence = "alpha\nbeta\n";
    assert_eq!(detect_delimiter(no_evidence), '|');
    assert_eq!(
        analyze_bytes(Path::new("no-evidence.csv"), no_evidence.as_bytes())
            .csv
            .expect("CSV analysis")
            .delimiter,
        '|'
    );
}

#[test]
fn public_analysis_routes_txt_and_log_paths_to_text_results() {
    let bytes = b"alpha beta gamma\nalpha beta gamma\nunique words\n";

    for path in ["notes.txt", "service.LOG"] {
        let metadata = analyze_bytes(Path::new(path), bytes);

        assert_common_metadata(&metadata, FileType::Txt, bytes.len());
        assert!(metadata.csv.is_none());
        assert_text_analysis(&metadata, 3, 1, "alpha beta gamma", 2);
    }
}

#[test]
fn public_analysis_routes_unknown_paths_to_text_results() {
    let bytes = b"opaque unknown content\nopaque unknown content\n";

    let metadata = analyze_bytes(Path::new("payload.bin"), bytes);

    assert_common_metadata(&metadata, FileType::Unknown, bytes.len());
    assert!(metadata.csv.is_none());
    assert_text_analysis(&metadata, 2, 1, "opaque unknown content", 2);
}

#[test]
fn public_analysis_metadata_survives_bincode_round_trip() {
    let bytes = b"alpha beta gamma\nalpha beta gamma\n";
    let metadata = analyze_bytes(Path::new("events.log"), bytes);

    let encoded = bincode::serialize(&metadata).expect("serialize public analysis metadata");
    let decoded: DpackMetadata =
        bincode::deserialize(&encoded).expect("deserialize public analysis metadata");

    assert_common_metadata(&decoded, FileType::Txt, bytes.len());
    assert!(decoded.csv.is_none());
    assert_text_analysis(&decoded, 2, 1, "alpha beta gamma", 2);
}

fn assert_common_metadata(metadata: &DpackMetadata, file_type: FileType, original_size: usize) {
    assert_eq!(metadata.version, CURRENT_VERSION);
    assert_eq!(metadata.original_file_type, file_type);
    assert_eq!(metadata.original_size, original_size as u64);
    assert_eq!(metadata.payload_kind, PayloadKind::RawZstd);
    assert_eq!(
        metadata.future_extensions,
        vec![
            ExtensionPoint::new("json", false),
            ExtensionPoint::new("toon", false),
            ExtensionPoint::new("gpu", false),
            ExtensionPoint::new("ssd_cache", false),
            ExtensionPoint::new("ai_ml_models", false),
        ]
    );
}

fn assert_column(
    column: &ColumnAnalysis,
    name: &str,
    total_rows: usize,
    unique_count: usize,
    repeated_value_count: usize,
    average_value_length: f64,
    strategy: EncodingStrategy,
) {
    assert_eq!(column.column_name, name);
    assert_eq!(column.total_rows, total_rows);
    assert_eq!(column.unique_count, unique_count);
    assert_eq!(column.repeated_value_count, repeated_value_count);
    assert_eq!(column.average_value_length, average_value_length);
    assert_eq!(column.suggested_encoding_strategy, strategy);
}

fn assert_text_analysis(
    metadata: &DpackMetadata,
    total_lines: usize,
    repeated_lines: usize,
    phrase: &str,
    phrase_count: usize,
) {
    let txt = metadata.txt.as_ref().expect("TXT/log analysis");
    assert_eq!(txt.total_lines, total_lines);
    assert_eq!(txt.repeated_lines, repeated_lines);
    assert_eq!(txt.repeated_phrases.len(), 1);
    assert_eq!(txt.repeated_phrases[0].phrase, phrase);
    assert_eq!(txt.repeated_phrases[0].count, phrase_count);
    assert_eq!(txt.compression_note, TXT_COMPRESSION_NOTE);
}
