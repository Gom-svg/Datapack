use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use datapack::storage;
use datapack::storage::chunked::{self, V2_HEADER_LEN};
use serde_json::Value;

const V2_GLOBAL_HASH_OFFSET: usize = 16;
const V2_ORIGINAL_SIZE_OFFSET: usize = 8;
const V2_ENTRY_ORIGINAL_SIZE_OFFSET: usize = 16;
const V2_ENTRY_COMPRESSED_OFFSET: usize = 24;
const V1_METADATA_LENGTH_OFFSET: usize = 8;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("compatibility")
        .join(name)
}

fn run_validate(archive: &Path, extra: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_datapack"))
        .arg("validate")
        .arg(archive)
        .args(extra)
        .output()
        .expect("run datapack validate")
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "validation unexpectedly failed: {}",
        combined_output(output)
    );
    assert!(
        output.stderr.is_empty(),
        "unexpected stderr: {}",
        combined_output(output)
    );
}

fn assert_validation_failure(output: &Output, code: &str) -> Value {
    assert_eq!(
        output.status.code(),
        Some(1),
        "validation failure must exit 1: {}",
        combined_output(output)
    );
    let report = parse_report(output);
    assert_eq!(report["valid"], false);
    assert_eq!(report["diagnostics"][0]["code"], code);
    report
}

fn parse_report(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "stdout was not a validation JSON document: {error}; output: {}",
            combined_output(output)
        )
    })
}

fn combined_output(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn write_archive(directory: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    let path = directory.join(name);
    fs::write(&path, bytes).expect("write validation archive");
    path
}

fn run_mutation(
    directory: &Path,
    name: &str,
    bytes: &[u8],
    expected_code: &str,
) -> (Output, Value) {
    let path = write_archive(directory, name, bytes);
    let output = run_validate(&path, &["--json"]);
    let report = assert_validation_failure(&output, expected_code);
    (output, report)
}

fn put_u64(bytes: &mut [u8], offset: usize, value: u64) {
    bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}

fn directory_entries(path: &Path) -> BTreeSet<String> {
    fs::read_dir(path)
        .expect("read validation directory")
        .map(|entry| {
            entry
                .expect("read validation directory entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect()
}

fn digest_hex(bytes: &[u8; 32]) -> String {
    let mut output = String::with_capacity(64);
    for byte in bytes {
        write!(&mut output, "{byte:02x}").expect("write digest hex");
    }
    output
}

#[test]
fn frozen_v1_and_v2_archives_validate_with_truthful_guarantees() {
    let cases = [
        ("v1_raw_zstd.dpack", 1, "dpack_v1", "raw_zstd", None),
        (
            "v1_csv_columnar.dpack",
            1,
            "dpack_v1",
            "csv_columnar_dictionary",
            None,
        ),
        (
            "v2_chunked_multichunk.dpack",
            2,
            "dpack_v2",
            "chunked_raw_zstd",
            Some(10),
        ),
    ];

    for (archive_name, version, format, payload_mode, chunk_count) in cases {
        let output = run_validate(&fixture(archive_name), &["--json"]);
        assert_success(&output);
        let report = parse_report(&output);

        assert_eq!(
            object_keys(&report),
            keys(&[
                "against",
                "archive",
                "checks",
                "diagnostics",
                "report_type",
                "schema_version",
                "valid",
            ])
        );
        assert_eq!(report["schema_version"], 1);
        assert_eq!(report["report_type"], "validation");
        assert_eq!(report["valid"], true);
        assert_eq!(report["archive"]["version"], version);
        assert_eq!(report["archive"]["format"], format);
        assert_eq!(report["archive"]["payload_mode"], payload_mode);
        assert_eq!(
            report["archive"]["chunk_count"],
            chunk_count.map_or(Value::Null, Value::from)
        );
        assert_eq!(report["checks"]["header"], "passed");
        assert_eq!(report["checks"]["metadata"], "passed");
        assert_eq!(report["checks"]["payload_structure"], "passed");
        assert_eq!(report["checks"]["decompression"], "passed");
        assert_eq!(report["checks"]["restored_length"], "passed");
        assert_eq!(report["against"]["status"], "not_requested");
        assert_eq!(report["against"]["source_size_bytes"], Value::Null);
        assert_eq!(report["diagnostics"], serde_json::json!([]));

        if version == 1 {
            assert_eq!(report["checks"]["chunk_table"], "not_applicable");
            assert_eq!(report["checks"]["per_chunk_sha256"], "not_available");
            assert_eq!(report["checks"]["global_sha256"], "not_available");
            assert_eq!(report["checks"]["trailing_data"], "not_available");
        } else {
            assert_eq!(report["checks"]["chunk_table"], "passed");
            assert_eq!(report["checks"]["per_chunk_sha256"], "passed");
            assert_eq!(report["checks"]["global_sha256"], "passed");
            assert_eq!(report["checks"]["trailing_data"], "passed");
        }
    }
}

#[test]
fn text_reports_distinguish_v1_unavailable_guarantees_from_v2_passes() {
    let v1 = run_validate(&fixture("v1_raw_zstd.dpack"), &[]);
    assert_success(&v1);
    let v1_text = String::from_utf8(v1.stdout).expect("v1 text is UTF-8");
    assert!(v1_text.contains("Archive validation: VALID"));
    assert!(v1_text.contains("Format: dpack_v1"));
    assert!(v1_text.contains("Per-chunk SHA-256: not_available (not stored by .dpack v1)"));
    assert!(v1_text.contains("Global SHA-256: not_available (not stored by .dpack v1)"));
    assert!(v1_text
        .contains("Trailing data: not_available (v1 has no authenticated payload-end field)"));
    assert!(v1_text.ends_with("No restored output was created.\n"));

    let v2 = run_validate(&fixture("v2_chunked_multichunk.dpack"), &[]);
    assert_success(&v2);
    let v2_text = String::from_utf8(v2.stdout).expect("v2 text is UTF-8");
    assert!(v2_text.contains("Format: dpack_v2"));
    assert!(v2_text.contains("Chunks: 10"));
    assert!(v2_text.contains("Per-chunk SHA-256: passed"));
    assert!(v2_text.contains("Global SHA-256: passed"));
    assert!(v2_text.contains("Trailing data: passed"));
    assert!(v2_text.ends_with("No restored output was created.\n"));
}

#[test]
fn compact_and_pretty_json_have_identical_semantics() {
    let archive = fixture("v2_chunked_multichunk.dpack");
    let compact = run_validate(&archive, &["--json"]);
    let pretty = run_validate(&archive, &["--json", "--pretty"]);
    assert_success(&compact);
    assert_success(&pretty);

    assert_eq!(parse_report(&compact), parse_report(&pretty));
    assert_eq!(
        compact.stdout.iter().filter(|byte| **byte == b'\n').count(),
        1
    );
    assert!(pretty.stdout.iter().filter(|byte| **byte == b'\n').count() > 1);
    assert_eq!(compact.stdout.last(), Some(&b'\n'));
    assert_eq!(pretty.stdout.last(), Some(&b'\n'));
}

#[test]
fn pretty_requires_json() {
    let output = run_validate(&fixture("v1_raw_zstd.dpack"), &["--pretty"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("--json"), "unexpected error: {stderr}");
}

#[test]
fn against_compares_complete_restored_identity() {
    let matching_cases = [
        ("v1_raw_zstd.dpack", "v1_raw_zstd_source.csv"),
        ("v1_csv_columnar.dpack", "v1_csv_columnar_source.csv"),
        ("v2_chunked_multichunk.dpack", "v2_chunked_source.bin"),
    ];

    for (archive_name, source_name) in matching_cases {
        let source = fixture(source_name);
        let source_arg = source.to_string_lossy();
        let output = run_validate(
            &fixture(archive_name),
            &["--against", source_arg.as_ref(), "--json"],
        );
        assert_success(&output);
        let report = parse_report(&output);
        assert_eq!(report["valid"], true);
        assert_eq!(report["against"]["status"], "matched");
        assert_eq!(
            report["against"]["source_size_bytes"],
            fs::metadata(source).expect("source metadata").len()
        );
    }

    let directory = tempfile::tempdir().expect("validation mismatch directory");
    let mismatch = directory.path().join("different-source.bin");
    fs::write(&mismatch, b"not the archived source").expect("write mismatching source");
    let mismatch_arg = mismatch.to_string_lossy();
    let output = run_validate(
        &fixture("v1_raw_zstd.dpack"),
        &["--against", mismatch_arg.as_ref(), "--json"],
    );
    let report = assert_validation_failure(&output, "AGAINST_MISMATCH");
    assert_eq!(report["against"]["status"], "mismatched");
    assert_eq!(report["checks"]["decompression"], "passed");
    assert_eq!(report["checks"]["restored_length"], "passed");
}

#[test]
fn bad_magic_version_and_truncated_headers_are_stable_failures() {
    let directory = tempfile::tempdir().expect("header mutation directory");
    let original = fs::read(fixture("v2_chunked_multichunk.dpack")).expect("read v2 fixture");

    let mut bad_magic = original.clone();
    bad_magic[0] = b'X';
    let (_, report) = run_mutation(
        directory.path(),
        "bad-magic.dpack",
        &bad_magic,
        "ARCHIVE_HEADER_INVALID",
    );
    assert_eq!(report["archive"]["version"], Value::Null);
    assert_eq!(report["checks"]["header"], "failed");

    let mut bad_version = original;
    bad_version[5..7].copy_from_slice(&3u16.to_le_bytes());
    let (_, report) = run_mutation(
        directory.path(),
        "bad-version.dpack",
        &bad_version,
        "UNSUPPORTED_ARCHIVE_VERSION",
    );
    assert_eq!(report["archive"]["version"], 3);
    assert_eq!(report["checks"]["header"], "failed");

    let (_, report) = run_mutation(
        directory.path(),
        "truncated-header.dpack",
        b"DPACK\x02",
        "ARCHIVE_HEADER_INVALID",
    );
    assert_eq!(report["archive"]["version"], Value::Null);
    assert_eq!(report["checks"]["header"], "failed");

    let mut bad_mode =
        fs::read(fixture("v2_chunked_multichunk.dpack")).expect("read v2 fixture again");
    bad_mode[7] = 2;
    let (_, report) = run_mutation(
        directory.path(),
        "bad-v2-mode.dpack",
        &bad_mode,
        "V2_HEADER_OR_CHUNK_TABLE_INVALID",
    );
    assert_eq!(report["checks"]["header"], "failed");
    assert_eq!(report["checks"]["chunk_table"], "not_completed");

    let mut empty_bad_hash = vec![0u8; V2_HEADER_LEN];
    empty_bad_hash[..5].copy_from_slice(b"DPACK");
    empty_bad_hash[5..7].copy_from_slice(&2u16.to_le_bytes());
    empty_bad_hash[7] = 1;
    put_u64(&mut empty_bad_hash, 56, 64);
    let (_, report) = run_mutation(
        directory.path(),
        "empty-v2-bad-global-hash.dpack",
        &empty_bad_hash,
        "V2_HEADER_OR_CHUNK_TABLE_INVALID",
    );
    assert_eq!(report["checks"]["header"], "passed");
    assert_eq!(report["checks"]["metadata"], "failed");

    let mut truncated_metadata = fs::read(fixture("v1_raw_zstd.dpack")).expect("read v1 fixture");
    truncated_metadata.truncate(20);
    let (_, report) = run_mutation(
        directory.path(),
        "truncated-v1-metadata.dpack",
        &truncated_metadata,
        "V1_HEADER_OR_METADATA_INVALID",
    );
    assert_eq!(report["checks"]["header"], "passed");
    assert_eq!(report["checks"]["metadata"], "failed");
}

#[test]
fn truncated_v1_and_v2_payloads_are_rejected() {
    let directory = tempfile::tempdir().expect("payload truncation directory");

    let mut v1 = fs::read(fixture("v1_raw_zstd.dpack")).expect("read v1 fixture");
    v1.truncate(v1.len() - 8);
    let (_, v1_report) = run_mutation(
        directory.path(),
        "truncated-v1.dpack",
        &v1,
        "V1_PAYLOAD_INVALID",
    );
    assert_eq!(v1_report["checks"]["decompression"], "failed");

    let mut v2 = fs::read(fixture("v2_chunked_multichunk.dpack")).expect("read v2 fixture");
    v2.truncate(v2.len() - 8);
    let (_, v2_report) = run_mutation(
        directory.path(),
        "truncated-v2.dpack",
        &v2,
        "V2_HEADER_OR_CHUNK_TABLE_INVALID",
    );
    assert_eq!(v2_report["checks"]["chunk_table"], "failed");
    assert_eq!(v2_report["checks"]["trailing_data"], "not_completed");
}

#[test]
fn v2_offset_payload_hash_and_trailing_corruption_matrix_is_detected() {
    let directory = tempfile::tempdir().expect("v2 corruption directory");
    let original = fs::read(fixture("v2_chunked_multichunk.dpack")).expect("read v2 fixture");

    let mut bad_offset = original.clone();
    put_u64(
        &mut bad_offset,
        V2_HEADER_LEN + V2_ENTRY_COMPRESSED_OFFSET,
        0,
    );
    let (_, report) = run_mutation(
        directory.path(),
        "bad-offset.dpack",
        &bad_offset,
        "V2_HEADER_OR_CHUNK_TABLE_INVALID",
    );
    assert_eq!(report["checks"]["header"], "passed");
    assert_eq!(report["checks"]["metadata"], "passed");
    assert_eq!(report["checks"]["chunk_table"], "failed");
    assert_eq!(report["checks"]["trailing_data"], "not_completed");

    let info = chunked::read_v2_archive_info(&mut Cursor::new(&original)).expect("read v2 info");
    let first_chunk = &info.chunks[0];
    let payload_offset =
        usize::try_from(first_chunk.compressed_offset + first_chunk.compressed_size / 2)
            .expect("payload offset fits usize");
    let mut bad_payload = original.clone();
    bad_payload[payload_offset] ^= 0x5a;
    let bad_payload_path = write_archive(directory.path(), "bad-payload.dpack", &bad_payload);
    let bad_payload_output = run_validate(&bad_payload_path, &["--json"]);
    assert_eq!(bad_payload_output.status.code(), Some(1));
    let bad_payload_report = parse_report(&bad_payload_output);
    let payload_code = bad_payload_report["diagnostics"][0]["code"]
        .as_str()
        .expect("payload diagnostic code");
    assert!(
        matches!(
            payload_code,
            "CHUNK_DECOMPRESSION_FAILED" | "CHUNK_HASH_MISMATCH"
        ),
        "unexpected payload corruption code: {payload_code}"
    );

    let mut bad_chunk_hash = original.clone();
    let chunk_hash_offset = chunked::chunk_hash_table_offset(0);
    bad_chunk_hash[chunk_hash_offset] ^= 0x5a;
    let (chunk_hash_output, report) = run_mutation(
        directory.path(),
        "bad-chunk-hash.dpack",
        &bad_chunk_hash,
        "CHUNK_HASH_MISMATCH",
    );
    assert_eq!(report["checks"]["per_chunk_sha256"], "failed");
    assert_eq!(report["checks"]["global_sha256"], "not_completed");
    assert_eq!(report["checks"]["decompression"], "not_completed");
    assert_eq!(report["checks"]["restored_length"], "not_completed");

    // Hash values fingerprint user data and are intentionally absent from the
    // machine-readable contract and its accompanying error output.
    let actual_chunk_hash = digest_hex(&info.chunks[0].chunk_sha256);
    let mut mutated_chunk_hash = info.chunks[0].chunk_sha256;
    mutated_chunk_hash[0] ^= 0x5a;
    let mutated_chunk_hash = digest_hex(&mutated_chunk_hash);
    let chunk_hash_text = combined_output(&chunk_hash_output);
    assert!(!chunk_hash_text.contains(&actual_chunk_hash));
    assert!(!chunk_hash_text.contains(&mutated_chunk_hash));

    let mut bad_global_hash = original.clone();
    bad_global_hash[V2_GLOBAL_HASH_OFFSET] ^= 0x5a;
    let (global_hash_output, report) = run_mutation(
        directory.path(),
        "bad-global-hash.dpack",
        &bad_global_hash,
        "GLOBAL_HASH_MISMATCH",
    );
    assert_eq!(report["checks"]["per_chunk_sha256"], "passed");
    assert_eq!(report["checks"]["global_sha256"], "failed");
    assert_eq!(report["checks"]["decompression"], "passed");
    assert_eq!(report["checks"]["restored_length"], "passed");
    let actual_global_hash = digest_hex(&info.global_sha256);
    let mut mutated_global_hash = info.global_sha256;
    mutated_global_hash[0] ^= 0x5a;
    let mutated_global_hash = digest_hex(&mutated_global_hash);
    let global_hash_text = combined_output(&global_hash_output);
    assert!(!global_hash_text.contains(&actual_global_hash));
    assert!(!global_hash_text.contains(&mutated_global_hash));

    let mut trailing = original;
    trailing.extend_from_slice(b"PRIVATE_TRAILING_GARBAGE_7A29");
    let (trailing_output, report) = run_mutation(
        directory.path(),
        "trailing.dpack",
        &trailing,
        "V2_HEADER_OR_CHUNK_TABLE_INVALID",
    );
    assert_eq!(report["checks"]["trailing_data"], "failed");
    assert_eq!(report["checks"]["header"], "passed");
    assert_eq!(report["checks"]["chunk_table"], "passed");
    assert!(!combined_output(&trailing_output).contains("PRIVATE_TRAILING_GARBAGE_7A29"));

    let mut bad_length =
        fs::read(fixture("v2_chunked_multichunk.dpack")).expect("read v2 fixture for length");
    let bad_total = info.original_size_bytes - 1;
    put_u64(&mut bad_length, V2_ORIGINAL_SIZE_OFFSET, bad_total);
    let last_index = usize::try_from(info.chunk_count - 1).expect("last chunk index fits usize");
    let last_size_offset =
        V2_HEADER_LEN + last_index * chunked::V2_CHUNK_ENTRY_LEN + V2_ENTRY_ORIGINAL_SIZE_OFFSET;
    put_u64(
        &mut bad_length,
        last_size_offset,
        info.chunks[last_index].original_size - 1,
    );
    let (_, report) = run_mutation(
        directory.path(),
        "bad-restored-length.dpack",
        &bad_length,
        "RESTORED_LENGTH_MISMATCH",
    );
    assert_eq!(report["checks"]["restored_length"], "failed");
    assert_eq!(report["checks"]["decompression"], "not_completed");
}

#[test]
fn output_memory_and_chunk_limits_fail_before_unbounded_work() {
    let directory = tempfile::tempdir().expect("validation limits directory");
    let large_source = vec![b'x'; 1024 * 1024 + 1];
    let large_source_path = directory.path().join("large-source.bin");
    let large_archive = storage::encode_raw_zstd_archive(&large_source_path, &large_source)
        .expect("encode large v1 archive");
    let large_archive_path = write_archive(directory.path(), "large-v1.dpack", &large_archive);
    let output = run_validate(&large_archive_path, &["--max-output-mb", "1", "--json"]);
    let report = assert_validation_failure(&output, "DECLARED_OUTPUT_LIMIT_REACHED");
    assert_eq!(report["archive"]["original_size_bytes"], large_source.len());
    assert_eq!(report["checks"]["decompression"], "not_completed");

    let output = run_validate(
        &fixture("v1_csv_columnar.dpack"),
        &["--max-memory-mb", "1", "--json"],
    );
    let report = assert_validation_failure(&output, "VALIDATION_MEMORY_LIMIT_REACHED");
    assert_eq!(report["archive"]["payload_mode"], "csv_columnar_dictionary");
    assert_eq!(report["checks"]["decompression"], "not_completed");

    let mut oversized_metadata =
        fs::read(fixture("v1_raw_zstd.dpack")).expect("read v1 raw fixture");
    put_u64(
        &mut oversized_metadata,
        V1_METADATA_LENGTH_OFFSET,
        2 * 1024 * 1024,
    );
    let oversized_metadata_path = write_archive(
        directory.path(),
        "oversized-v1-metadata.dpack",
        &oversized_metadata,
    );
    let output = run_validate(
        &oversized_metadata_path,
        &["--max-memory-mb", "1", "--json"],
    );
    let report = assert_validation_failure(&output, "VALIDATION_MEMORY_LIMIT_REACHED");
    assert_eq!(report["checks"]["metadata"], "not_completed");

    let output = run_validate(
        &fixture("v2_chunked_multichunk.dpack"),
        &["--max-chunks", "1", "--json"],
    );
    let report = assert_validation_failure(&output, "CHUNK_COUNT_LIMIT_REACHED");
    assert!(report["diagnostics"][0]["message"]
        .as_str()
        .expect("limit diagnostic message")
        .contains("--max-chunks"));
}

#[test]
fn validation_is_deterministic_private_and_does_not_modify_files() {
    let directory = tempfile::tempdir().expect("private validation directory");
    let secret_bytes = b"PRIVATE_RESTORED_CONTENT_4B88\nPRIVATE_SECOND_ROW_D157\n";
    let secret_source = directory.path().join("PRIVATE_SOURCE_PATH_8C42.bin");
    let secret_archive = directory.path().join("PRIVATE_ARCHIVE_PATH_9D53.dpack");
    fs::write(&secret_source, secret_bytes).expect("write private source");
    let archive_bytes = storage::encode_raw_zstd_archive(&secret_source, secret_bytes)
        .expect("encode private v1 archive");
    fs::write(&secret_archive, &archive_bytes).expect("write private archive");

    let entries_before = directory_entries(directory.path());
    let source_before = fs::read(&secret_source).expect("read source before validation");
    let archive_before = fs::read(&secret_archive).expect("read archive before validation");
    let source_arg = secret_source.to_string_lossy();

    let first = run_validate(
        &secret_archive,
        &["--against", source_arg.as_ref(), "--json"],
    );
    let second = run_validate(
        &secret_archive,
        &["--against", source_arg.as_ref(), "--json"],
    );
    assert_success(&first);
    assert_success(&second);
    assert_eq!(first.stdout, second.stdout);
    assert_eq!(parse_report(&first)["against"]["status"], "matched");

    let output = combined_output(&first);
    for secret in [
        "PRIVATE_SOURCE_PATH_8C42",
        "PRIVATE_ARCHIVE_PATH_9D53",
        "PRIVATE_RESTORED_CONTENT_4B88",
        "PRIVATE_SECOND_ROW_D157",
    ] {
        assert!(
            !output.contains(secret),
            "validation output disclosed {secret}"
        );
    }
    assert_eq!(directory_entries(directory.path()), entries_before);
    assert_eq!(
        fs::read(&secret_source).expect("read source after validation"),
        source_before
    );
    assert_eq!(
        fs::read(&secret_archive).expect("read archive after validation"),
        archive_before
    );
    assert!(directory_entries(directory.path())
        .iter()
        .all(|name| !name.ends_with(".partial")));
}

fn object_keys(value: &Value) -> BTreeSet<String> {
    value
        .as_object()
        .expect("value is a JSON object")
        .keys()
        .cloned()
        .collect()
}

fn keys(values: &[&str]) -> BTreeSet<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}
