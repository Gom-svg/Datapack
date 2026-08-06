use std::fs;
use std::process::{Command, Output};

use datapack::metadata::PayloadKind;
use datapack::storage;

const COLUMNAR_MODE: &str = "CsvColumnarDictionary";
const V2_RAW_ZSTD_MODE: u8 = 1;

#[test]
fn phase_1_consumers_agree_and_v1_output_is_deterministic() {
    let directory = tempfile::tempdir().expect("create consistency test directory");
    let input = directory.path().join("repetitive.csv");
    let first_archive_path = directory.path().join("first.dpack");
    let second_archive_path = directory.path().join("second.dpack");
    let input_bytes = repetitive_csv(512);
    fs::write(&input, &input_bytes).expect("write deterministic repetitive CSV");

    let analyze = Command::new(env!("CARGO_BIN_EXE_datapack"))
        .arg("analyze")
        .arg(&input)
        .args(["--plan", "--sample-mb", "1"])
        .output()
        .expect("run analyze --plan");
    assert_success(&analyze, "analyze --plan");
    let analyze_stdout = String::from_utf8(analyze.stdout).expect("analyze stdout is UTF-8");
    let analyze_mode = analyze_stdout
        .lines()
        .find_map(|line| line.strip_prefix("Archive mode:").map(str::trim))
        .expect("analyze --plan reports Archive mode");
    assert_eq!(analyze_mode, COLUMNAR_MODE);

    let benchmark = Command::new(env!("CARGO_BIN_EXE_datapack"))
        .arg("benchmark")
        .arg(&input)
        .arg("--estimate-only")
        .output()
        .expect("run benchmark --estimate-only");
    assert_success(&benchmark, "benchmark --estimate-only");
    let benchmark_stdout = String::from_utf8(benchmark.stdout).expect("benchmark stdout is UTF-8");
    let benchmark_mode = metric_value(&benchmark_stdout, "estimated_mode")
        .expect("estimate-only benchmark reports estimated_mode");
    assert_eq!(benchmark_mode, analyze_mode);

    for archive_path in [&first_archive_path, &second_archive_path] {
        let compress = Command::new(env!("CARGO_BIN_EXE_datapack"))
            .arg("compress")
            .arg(&input)
            .arg(archive_path)
            .args(["--mode", "fast", "--sample-mb", "1"])
            .output()
            .expect("run non-chunked v1 compress");
        assert_success(&compress, "non-chunked v1 compress");
        assert_eq!(storage::archive_version_from_path(archive_path).unwrap(), 1);
    }

    let first_archive = fs::read(&first_archive_path).expect("read first v1 archive");
    let second_archive = fs::read(&second_archive_path).expect("read second v1 archive");
    assert_eq!(
        first_archive, second_archive,
        "same-build v1 output must be deterministic for identical input and options"
    );

    let decoded = storage::decode_archive(&first_archive).expect("decode v1 archive");
    assert_eq!(
        decoded.metadata.payload_kind,
        PayloadKind::CsvColumnarDictionary
    );
    assert_eq!(
        storage::restore_archive(&decoded).expect("restore v1 archive"),
        input_bytes
    );
}

#[test]
fn chunked_compress_bypasses_analysis_for_non_csv_binary_input() {
    let directory = tempfile::tempdir().expect("create chunked bypass test directory");
    let input = directory.path().join("invalid-analysis-input.bin");
    let archive_path = directory.path().join("binary-v2.dpack");
    let restored_path = directory.path().join("restored.bin");
    let input_bytes = invalid_analysis_bytes();
    fs::write(&input, &input_bytes).expect("write invalid analysis input");

    let compress = Command::new(env!("CARGO_BIN_EXE_datapack"))
        .arg("compress")
        .arg(&input)
        .arg(&archive_path)
        .args(["--chunked", "--threads", "1"])
        .output()
        .expect("run chunked compression");
    assert_success(&compress, "chunked compression of non-CSV binary input");
    assert_eq!(
        storage::archive_version_from_path(&archive_path).unwrap(),
        2
    );

    let archive_bytes = fs::read(&archive_path).expect("read v2 archive");
    let mut archive_cursor = std::io::Cursor::new(&archive_bytes);
    let archive_info =
        storage::chunked::read_v2_archive_info(&mut archive_cursor).expect("read v2 archive info");
    assert_eq!(archive_info.original_size_bytes, input_bytes.len() as u64);
    assert_eq!(archive_info.archive_mode, V2_RAW_ZSTD_MODE);
    assert!(!archive_info.chunks.is_empty());
    assert!(archive_info
        .chunks
        .iter()
        .all(|chunk| chunk.compression_mode == V2_RAW_ZSTD_MODE));

    storage::chunked::decode_raw_zstd_chunked_file(
        &archive_path,
        &restored_path,
        storage::chunked::ChunkedDecompressOptions::default(),
    )
    .expect("verify and restore v2 archive");
    assert_eq!(
        fs::read(restored_path).expect("read restored binary"),
        input_bytes
    );
}

#[test]
fn legacy_renderer_keeps_duplicate_long_columns_separate_and_truncated() {
    const LONG_NAME: &str = "abcdefghijklmnop";
    const TRUNCATED_NAME: &str = "abcdefghijklmn";

    let directory = tempfile::tempdir().expect("create renderer test directory");
    let input = directory.path().join("duplicate-long-names.csv");
    fs::write(
        &input,
        format!("{LONG_NAME},{LONG_NAME}\nleft,right\nother,values\n"),
    )
    .expect("write duplicate-name CSV");

    let analyze = Command::new(env!("CARGO_BIN_EXE_datapack"))
        .arg("analyze")
        .arg(&input)
        .output()
        .expect("run analyze renderer case");
    assert_success(&analyze, "analyze duplicate long columns");
    assert!(analyze.stderr.is_empty());

    let stdout = String::from_utf8(analyze.stdout).expect("analyze stdout is UTF-8");
    assert_eq!(
        stdout
            .lines()
            .filter(|line| line.starts_with(TRUNCATED_NAME))
            .count(),
        2
    );
    assert!(!stdout.contains(LONG_NAME));
}

fn repetitive_csv(rows: usize) -> Vec<u8> {
    let regions = ["north", "south", "east", "west"];
    let statuses = ["active", "inactive"];
    let mut csv = String::from("region,status,category\n");
    for index in 0..rows {
        csv.push_str(regions[index % regions.len()]);
        csv.push(',');
        csv.push_str(statuses[index % statuses.len()]);
        csv.push_str(",fixed\n");
    }
    csv.into_bytes()
}

fn invalid_analysis_bytes() -> Vec<u8> {
    let pattern = [0, 0xff, 0xfe, 0x80, b'X', b'Y', b'Z'];
    pattern.into_iter().cycle().take(8 * 1024).collect()
}

fn metric_value<'a>(stdout: &'a str, metric: &str) -> Option<&'a str> {
    stdout
        .lines()
        .find(|line| line.split_whitespace().next() == Some(metric))
        .and_then(|line| line.split_whitespace().nth(1))
}

fn assert_success(output: &Output, operation: &str) {
    assert!(
        output.status.success(),
        "{operation} failed with status {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
