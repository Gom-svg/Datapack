use std::fs;
use std::io::Cursor;
use std::process::{Command, Output};

use datapack::generation::{self, Profile};
use datapack::metadata::PayloadKind;
use datapack::storage;

const V2_RAW_ZSTD_MODE: u8 = 1;

#[test]
fn unsafe_or_unresolved_structured_inputs_fall_back_to_raw_zstd_exactly() {
    let cases: [(&str, &[u8]); 4] = [
        ("ambiguous.data", b"a,b|c\n1,2|3\n"),
        ("inconsistent.psv", b"a|b\n1|2\n3|4|5\n"),
        ("unterminated.psv", b"a|b\n1|\"open\n"),
        ("mixed-newlines.psv", b"a|b\n1|2\r\n2|3\n"),
    ];

    for (name, input) in cases {
        let directory = tempfile::tempdir().expect("temporary fallback directory");
        let input_path = directory.path().join(name);
        let archive_path = directory.path().join("output.dpack");
        fs::write(&input_path, input).expect("write fallback input");

        let output = run_compress(&input_path, &archive_path, &["--verify-best"]);
        assert_success(&output);
        let archive_bytes = fs::read(&archive_path).expect("read fallback archive");
        let archive = storage::decode_archive(&archive_bytes).expect("decode fallback archive");
        assert_eq!(
            archive.metadata.payload_kind,
            PayloadKind::RawZstd,
            "{name}"
        );
        assert_eq!(
            storage::restore_archive(&archive).expect("restore fallback archive"),
            input,
            "{name}"
        );
    }
}

#[test]
fn canonical_delimiter_is_propagated_past_quoted_legacy_decoys() {
    let directory = tempfile::tempdir().expect("temporary delimiter routing directory");
    let input_path = directory.path().join("quoted-decoys.psv");
    let archive_path = directory.path().join("quoted-decoys.dpack");
    let mut input = String::from("group|note|status\n");
    for _ in 0..512 {
        input.push_str("A|\"comma,comma,comma,comma,comma\"|active\n");
    }
    fs::write(&input_path, input.as_bytes()).expect("write quoted-decoy input");

    let output = run_compress(&input_path, &archive_path, &[]);
    assert_success(&output);
    let archive = storage::decode_archive(&fs::read(archive_path).expect("read routed archive"))
        .expect("decode routed archive");
    assert_eq!(
        archive.metadata.payload_kind,
        PayloadKind::CsvColumnarDictionary
    );
    let dcsv = zstd::stream::decode_all(Cursor::new(&archive.payload))
        .expect("decompress routed DCSV01 payload");
    assert_eq!(&dcsv[..6], b"DCSV01");
    assert_eq!(dcsv[6], b'|');
    assert_eq!(
        storage::restore_archive(&archive).unwrap(),
        input.as_bytes()
    );
}

#[test]
fn cli_dictionary_limits_control_default_best_and_verify_candidates() {
    let directory = tempfile::tempdir().expect("temporary execution-limit directory");
    let input_path = directory.path().join("repetitive.csv");
    generation::generate_to_path(Profile::Repetitive, &input_path, 10_000, Some(42))
        .expect("generate execution-limit corpus");
    let input = fs::read(&input_path).expect("read execution-limit corpus");
    let modes: [(&str, &[&str]); 3] = [
        (
            "default",
            &["--max-dictionary-values", "1", "--max-dictionary-mb", "0"],
        ),
        (
            "best",
            &[
                "--mode",
                "best",
                "--max-dictionary-values",
                "1",
                "--max-dictionary-mb",
                "0",
            ],
        ),
        (
            "verify",
            &[
                "--verify-best",
                "--max-dictionary-values",
                "1",
                "--max-dictionary-mb",
                "0",
            ],
        ),
    ];

    for (name, arguments) in modes {
        let archive_path = directory.path().join(format!("{name}.dpack"));
        let output = run_compress(&input_path, &archive_path, arguments);
        assert_success(&output);
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("switching to Plain"),
            "{name} did not report dictionary-plan demotion"
        );
        let archive = storage::decode_archive(&fs::read(&archive_path).unwrap()).unwrap();
        if name != "verify" {
            assert_eq!(
                archive.metadata.payload_kind,
                PayloadKind::CsvColumnarDictionary,
                "{name} did not execute the all-Plain structured plan"
            );
        }
        if archive.metadata.payload_kind == PayloadKind::CsvColumnarDictionary {
            let dcsv = zstd::stream::decode_all(Cursor::new(&archive.payload)).unwrap();
            assert_eq!(
                dcsv[21], 0,
                "{name} re-enabled Dictionary for a planned Plain column"
            );
        }
        assert_eq!(storage::restore_archive(&archive).unwrap(), input, "{name}");
    }
}

#[test]
fn full_input_dictionary_limit_drift_falls_back_to_raw_zstd() {
    let directory = tempfile::tempdir().expect("temporary full-input drift directory");
    let input_path = directory.path().join("late-unique.psv");
    let archive_path = directory.path().join("late-unique.dpack");
    let mut input = String::from("kind|status\n");
    for _ in 0..10_100 {
        input.push_str("A|open\n");
    }
    input.push_str("B|open\n");
    fs::write(&input_path, input.as_bytes()).expect("write late-unique corpus");

    let output = run_compress(
        &input_path,
        &archive_path,
        &["--sample-mb", "1", "--max-dictionary-values", "2"],
    );
    assert_success(&output);
    let archive = storage::decode_archive(&fs::read(archive_path).unwrap()).unwrap();
    assert_eq!(archive.metadata.payload_kind, PayloadKind::RawZstd);
    assert_eq!(
        storage::restore_archive(&archive).unwrap(),
        input.as_bytes()
    );
}

#[test]
fn chunked_v2_remains_raw_zstd_for_structured_pipe_input() {
    let directory = tempfile::tempdir().expect("temporary v2 directory");
    let input_path = directory.path().join("structured.psv");
    let archive_path = directory.path().join("structured.dpack");
    let restored_path = directory.path().join("restored.psv");
    let mut input = String::from("group|status|code\n");
    for _ in 0..128 {
        input.push_str("A|active|001\n");
    }
    fs::write(&input_path, input.as_bytes()).expect("write v2 structured input");

    let output = run_compress(&input_path, &archive_path, &["--chunked", "--threads", "1"]);
    assert_success(&output);
    assert_eq!(
        storage::archive_version_from_path(&archive_path).unwrap(),
        2
    );

    let archive_bytes = fs::read(&archive_path).expect("read v2 archive");
    let info = storage::chunked::read_v2_archive_info(&mut Cursor::new(&archive_bytes))
        .expect("read v2 archive info");
    assert_eq!(info.archive_mode, V2_RAW_ZSTD_MODE);
    assert!(info
        .chunks
        .iter()
        .all(|chunk| chunk.compression_mode == V2_RAW_ZSTD_MODE));

    storage::chunked::decode_raw_zstd_chunked_file(
        &archive_path,
        &restored_path,
        storage::chunked::ChunkedDecompressOptions::default(),
    )
    .expect("restore v2 archive");
    assert_eq!(
        fs::read(restored_path).expect("read v2 output"),
        input.as_bytes()
    );
}

fn run_compress(input: &std::path::Path, output: &std::path::Path, arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_datapack"))
        .arg("compress")
        .arg(input)
        .arg(output)
        .args(arguments)
        .output()
        .expect("run compression")
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "command failed with {:?}: stdout={} stderr={}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
