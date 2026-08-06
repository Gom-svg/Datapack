use std::fmt::Write as _;
use std::fs;
use std::io::Cursor;
use std::path::PathBuf;
use std::process::Command;

use datapack::formats::csv::columnar;
use datapack::metadata::PayloadKind;
use datapack::storage;
use sha2::{Digest, Sha256};

const V1_RAW_SOURCE_SIZE: u64 = 810;
const V1_RAW_ARCHIVE_SIZE: u64 = 331;
const V1_RAW_SOURCE_SHA256: &str =
    "81f5cc20f74dd9bea672f7bdbb5ecc1bf5fdb5379223c4af8c2cb650f54fb994";
const V1_RAW_ARCHIVE_SHA256: &str =
    "7d467f55a0927c0fafc0a75c1fdf7be5f7ad2db430a8bee65dc9312c71ddadab";
const V1_COLUMNAR_SOURCE_SIZE: u64 = 1_269;
const V1_COLUMNAR_ARCHIVE_SIZE: u64 = 265;
const V1_COLUMNAR_SOURCE_SHA256: &str =
    "320c19793eea868aa109cb798887f92394de114879a3d5df3ed55fb1f49d16bf";
const V1_COLUMNAR_ARCHIVE_SHA256: &str =
    "0210e1b969e124731cbe374fb030fc75e391949e588195b676b2a7a2b36070a5";
const V2_SOURCE_SIZE: u64 = 588;
const V2_ARCHIVE_SIZE: u64 = 1_542;
const V2_SOURCE_SHA256: &str = "f41051e6c9fcbf543208b67f26116e0f11188d997d52815f7ee24444f643f8f3";
const V2_ARCHIVE_SHA256: &str = "a0644b941c86dac0c0d0ccd37e8cd6f07fdd25d45f38672a0d0c626faa5f8cbd";
const V2_CHUNK_SIZE: u64 = 64;
const V2_CHUNK_COUNT: u64 = 10;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("compatibility")
        .join(name)
}

fn read_frozen(name: &str, expected_size: u64, expected_sha256: &str) -> Vec<u8> {
    let path = fixture(name);
    let bytes =
        fs::read(&path).unwrap_or_else(|error| panic!("read '{}': {error}", path.display()));
    assert_eq!(
        u64::try_from(bytes.len()).unwrap(),
        expected_size,
        "fixture size changed: {}",
        path.display()
    );
    assert_eq!(
        sha256_hex(&bytes),
        expected_sha256,
        "frozen fixture changed: {}",
        path.display()
    );
    bytes
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        write!(&mut hex, "{byte:02x}").unwrap();
    }
    hex
}

fn assert_v1_fixture(
    source_name: &str,
    archive_name: &str,
    source_size: u64,
    archive_size: u64,
    source_sha256: &str,
    archive_sha256: &str,
    expected_payload: PayloadKind,
) {
    let source = read_frozen(source_name, source_size, source_sha256);
    let archive_bytes = read_frozen(archive_name, archive_size, archive_sha256);
    assert_eq!(
        storage::archive_version_from_path(&fixture(archive_name)).unwrap(),
        1
    );

    let archive = storage::decode_archive(&archive_bytes).unwrap();
    assert_eq!(archive.metadata.payload_kind, expected_payload);
    assert_eq!(archive.metadata.original_size, source_size);
    assert_eq!(storage::restore_archive(&archive).unwrap(), source);

    assert_eq!(
        sha256_hex(&fs::read(fixture(archive_name)).unwrap()),
        archive_sha256,
        "decoder mutated frozen fixture"
    );
}

#[test]
fn legacy_v1_raw_zstd_fixture_remains_readable() {
    assert_v1_fixture(
        "v1_raw_zstd_source.csv",
        "v1_raw_zstd.dpack",
        V1_RAW_SOURCE_SIZE,
        V1_RAW_ARCHIVE_SIZE,
        V1_RAW_SOURCE_SHA256,
        V1_RAW_ARCHIVE_SHA256,
        PayloadKind::RawZstd,
    );
}

#[test]
fn legacy_v1_columnar_fixture_remains_byte_exact() {
    assert_v1_fixture(
        "v1_csv_columnar_source.csv",
        "v1_csv_columnar.dpack",
        V1_COLUMNAR_SOURCE_SIZE,
        V1_COLUMNAR_ARCHIVE_SIZE,
        V1_COLUMNAR_SOURCE_SHA256,
        V1_COLUMNAR_ARCHIVE_SHA256,
        PayloadKind::CsvColumnarDictionary,
    );

    let source = read_frozen(
        "v1_csv_columnar_source.csv",
        V1_COLUMNAR_SOURCE_SIZE,
        V1_COLUMNAR_SOURCE_SHA256,
    );
    let archive_bytes = read_frozen(
        "v1_csv_columnar.dpack",
        V1_COLUMNAR_ARCHIVE_SIZE,
        V1_COLUMNAR_ARCHIVE_SHA256,
    );
    let archive = storage::decode_archive(&archive_bytes).unwrap();
    let frozen_columnar_payload = zstd::stream::decode_all(Cursor::new(&archive.payload)).unwrap();
    assert!(frozen_columnar_payload.starts_with(b"DCSV01"));

    let current_columnar_payload = columnar::encode(&source)
        .unwrap()
        .expect("frozen source remains eligible for columnar encoding");
    assert_eq!(
        current_columnar_payload, frozen_columnar_payload,
        "public columnar encoder no longer reproduces the frozen v1 DCSV01 payload"
    );
}

#[test]
fn v2_chunked_fixture_remains_readable_and_byte_exact() {
    let source = read_frozen("v2_chunked_source.bin", V2_SOURCE_SIZE, V2_SOURCE_SHA256);
    let archive = read_frozen(
        "v2_chunked_multichunk.dpack",
        V2_ARCHIVE_SIZE,
        V2_ARCHIVE_SHA256,
    );
    let archive_path = fixture("v2_chunked_multichunk.dpack");
    assert_eq!(
        storage::archive_version_from_path(&archive_path).unwrap(),
        2
    );

    let info = storage::chunked::read_v2_archive_info(&mut Cursor::new(&archive)).unwrap();
    assert_eq!(info.chunk_count, V2_CHUNK_COUNT);
    assert_eq!(info.chunk_size_target, V2_CHUNK_SIZE);
    assert_eq!(info.original_size_bytes, V2_SOURCE_SIZE);
    let source_digest: [u8; 32] = Sha256::digest(&source).into();
    assert_eq!(info.global_sha256, source_digest);

    let temporary = tempfile::tempdir().unwrap();
    let output = temporary.path().join("restored.bin");
    storage::chunked::decode_raw_zstd_chunked_file(
        &archive_path,
        &output,
        storage::chunked::ChunkedDecompressOptions::default(),
    )
    .unwrap();
    assert_eq!(fs::read(output).unwrap(), source);
    assert_eq!(
        sha256_hex(&fs::read(&archive_path).unwrap()),
        V2_ARCHIVE_SHA256,
        "decoder mutated frozen fixture"
    );
}

#[test]
fn corrupted_fixture_copies_never_commit_final_output() {
    let fixtures = [
        (
            "v1_raw_zstd.dpack",
            V1_RAW_ARCHIVE_SIZE,
            V1_RAW_ARCHIVE_SHA256,
        ),
        (
            "v1_csv_columnar.dpack",
            V1_COLUMNAR_ARCHIVE_SIZE,
            V1_COLUMNAR_ARCHIVE_SHA256,
        ),
        (
            "v2_chunked_multichunk.dpack",
            V2_ARCHIVE_SIZE,
            V2_ARCHIVE_SHA256,
        ),
    ];

    for (archive_name, archive_size, archive_sha256) in fixtures {
        let original = read_frozen(archive_name, archive_size, archive_sha256);
        let mut corrupted = original.clone();
        corrupted[0] ^= 0x80;

        let temporary = tempfile::tempdir().unwrap();
        let archive_copy = temporary.path().join(archive_name);
        let output_path = temporary.path().join("must-not-exist.out");
        fs::write(&archive_copy, corrupted).unwrap();

        let output = Command::new(env!("CARGO_BIN_EXE_datapack"))
            .arg("decompress")
            .arg(&archive_copy)
            .arg(&output_path)
            .output()
            .unwrap();
        assert!(
            !output.status.success(),
            "corrupted {archive_name} unexpectedly decompressed"
        );
        assert!(
            !output_path.exists(),
            "failed {archive_name} restore committed a final output"
        );
        let partial_outputs: Vec<_> = fs::read_dir(temporary.path())
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| {
                path.extension()
                    .is_some_and(|extension| extension == "partial")
            })
            .collect();
        assert!(
            partial_outputs.is_empty(),
            "failed {archive_name} restore left partial outputs: {partial_outputs:?}"
        );
        assert_eq!(
            sha256_hex(&fs::read(fixture(archive_name)).unwrap()),
            archive_sha256,
            "failure path mutated frozen fixture"
        );
    }
}
