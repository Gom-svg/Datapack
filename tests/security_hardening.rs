use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use datapack::storage;
use datapack::storage::chunked::{
    self, ChunkedArchiveInfo, ChunkedCompressOptions, ChunkedDecompressOptions, V2ArchiveLimits,
    MAX_CHUNKS_HARD, MAX_CHUNK_SIZE_MB, MAX_IN_FLIGHT_CHUNKS, MAX_THREAD_COUNT, V2_CHUNK_ENTRY_LEN,
    V2_HEADER_LEN,
};
use tempfile::TempDir;

const HEADER_ORIGINAL_SIZE: usize = 8;
const HEADER_GLOBAL_HASH: usize = 16;
const HEADER_CHUNK_COUNT: usize = 48;
const ENTRY_ORIGINAL_OFFSET: usize = 8;
const ENTRY_ORIGINAL_SIZE: usize = 16;
const ENTRY_COMPRESSED_OFFSET: usize = 24;
const ENTRY_COMPRESSED_SIZE: usize = 32;
const ENTRY_FEATURE_BITS: usize = 77;

fn compress_options(chunk_size_bytes: usize) -> ChunkedCompressOptions {
    ChunkedCompressOptions::new_with_max_in_flight(chunk_size_bytes, 1, 1, false, false)
        .expect("valid tiny-archive compression options")
}

fn make_v2_bytes(directory: &TempDir, bytes: &[u8], chunk_size: usize) -> Vec<u8> {
    let input = directory.path().join("source.bin");
    let archive = directory.path().join("valid.dpack");
    fs::write(&input, bytes).expect("write source");
    chunked::encode_raw_zstd_chunked_file(&input, &archive, compress_options(chunk_size))
        .expect("create valid v2 archive");
    fs::read(archive).expect("read valid v2 archive")
}

fn write_archive(directory: &TempDir, name: &str, bytes: &[u8]) -> PathBuf {
    let path = directory.path().join(name);
    fs::write(&path, bytes).expect("write archive fixture");
    path
}

fn info(bytes: &[u8]) -> ChunkedArchiveInfo {
    chunked::read_v2_archive_info(&mut Cursor::new(bytes)).expect("valid v2 metadata")
}

fn parse_error(bytes: &[u8]) -> String {
    chunked::read_v2_archive_info(&mut Cursor::new(bytes))
        .expect_err("mutated metadata must be rejected")
        .to_string()
}

fn parse_error_with_limits(bytes: &[u8], limits: V2ArchiveLimits) -> String {
    chunked::read_v2_archive_info_with_limits(&mut Cursor::new(bytes), limits)
        .expect_err("archive must exceed configured limit")
        .to_string()
}

fn decode_error(directory: &TempDir, bytes: &[u8], options: ChunkedDecompressOptions) -> String {
    let archive = write_archive(directory, "broken.dpack", bytes);
    let output = directory.path().join("restored.bin");
    chunked::decode_raw_zstd_chunked_file(&archive, &output, options)
        .expect_err("mutated payload must be rejected")
        .to_string()
}

fn entry_field(index: usize, field: usize) -> usize {
    V2_HEADER_LEN
        .checked_add(
            index
                .checked_mul(V2_CHUNK_ENTRY_LEN)
                .expect("test entry offset fits"),
        )
        .and_then(|offset| offset.checked_add(field))
        .expect("test field offset fits")
}

fn put_u64(bytes: &mut [u8], offset: usize, value: u64) {
    bytes
        .get_mut(offset..offset + 8)
        .expect("field lies in generated archive")
        .copy_from_slice(&value.to_le_bytes());
}

fn flip_byte(bytes: &mut [u8], offset: usize) {
    let byte = bytes
        .get_mut(offset)
        .expect("mutation lies in generated archive");
    *byte ^= 0x5a;
}

fn damage_zstd_frame(bytes: &mut [u8], chunk_index: usize) {
    let archive_info = info(bytes);
    let chunk = archive_info
        .chunks
        .get(chunk_index)
        .expect("generated archive contains requested chunk");
    let start = usize::try_from(chunk.compressed_offset).expect("test offset fits usize");
    bytes
        .get_mut(start..start + 4)
        .expect("zstd frame has a four-byte header")
        .fill(0);
}

fn datapack_command() -> Command {
    Command::new(env!("CARGO_BIN_EXE_datapack"))
}

fn run_decompress(archive: &Path, output: &Path, extra: &[&str]) -> Output {
    let mut command = datapack_command();
    command
        .arg("decompress")
        .arg(archive)
        .arg(output)
        .args(extra);
    command.output().expect("run datapack decompress")
}

fn run_compress(input: &Path, output: &Path, extra: &[&str]) -> Output {
    let mut command = datapack_command();
    command.arg("compress").arg(input).arg(output).args(extra);
    command.output().expect("run datapack compress")
}

fn combined_output(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn assert_cli_failure(output: Output, expected: &[&str]) -> String {
    assert!(!output.status.success(), "command unexpectedly succeeded");
    let text = combined_output(&output);
    for needle in expected {
        assert!(
            text.contains(needle),
            "expected output to contain {needle:?}, got: {text}"
        );
    }
    text
}

fn partial_files(directory: &TempDir) -> Vec<PathBuf> {
    fs::read_dir(directory.path())
        .expect("read temporary directory")
        .map(|entry| entry.expect("read directory entry").path())
        .filter(|path| {
            path.file_name()
                .is_some_and(|name| name.to_string_lossy().ends_with(".partial"))
        })
        .collect()
}

fn cli_source(directory: &TempDir) -> PathBuf {
    let input = directory.path().join("input.csv");
    fs::write(&input, b"id,value\n1,alpha\n2,beta\n").expect("write CLI input");
    input
}

#[test]
fn bad_magic_rejected() {
    let directory = tempfile::tempdir().expect("tempdir");
    let mut archive = make_v2_bytes(&directory, b"tiny", 1024);
    archive[0] = b'X';
    assert!(parse_error(&archive).contains("bad magic bytes"));
}

#[test]
fn unsupported_version_rejected() {
    let directory = tempfile::tempdir().expect("tempdir");
    let mut archive = make_v2_bytes(&directory, b"tiny", 1024);
    archive[5..7].copy_from_slice(&3u16.to_le_bytes());
    assert!(parse_error(&archive).contains("expected v2 archive, found version 3"));
}

#[test]
fn truncated_header_rejected() {
    let error = parse_error(&[0u8; V2_HEADER_LEN / 2]);
    assert!(error.contains("truncated v2 header"));
}

#[test]
fn truncated_chunk_table_rejected() {
    let directory = tempfile::tempdir().expect("tempdir");
    let archive = make_v2_bytes(&directory, b"tiny", 1024);
    let error = parse_error(&archive[..V2_HEADER_LEN + V2_CHUNK_ENTRY_LEN / 2]);
    assert!(error.contains("chunk table extends beyond file length"));
}

#[test]
fn chunk_table_too_large_rejected() {
    let directory = tempfile::tempdir().expect("tempdir");
    let mut archive = make_v2_bytes(&directory, b"x", 1024);
    put_u64(&mut archive, HEADER_CHUNK_COUNT, MAX_CHUNKS_HARD);
    assert!(parse_error(&archive).contains("chunk table extends beyond file length"));
}

#[test]
fn chunk_count_overflow_rejected() {
    let directory = tempfile::tempdir().expect("tempdir");
    let mut archive = make_v2_bytes(&directory, b"x", 1024);
    put_u64(&mut archive, HEADER_CHUNK_COUNT, u64::MAX);
    let error = parse_error(&archive);
    assert!(error.contains("internal safety ceiling"));
}

#[test]
fn malicious_huge_chunk_count_rejected_before_allocation() {
    let directory = tempfile::tempdir().expect("tempdir");
    let mut archive = make_v2_bytes(&directory, b"x", 1024);
    put_u64(&mut archive, HEADER_CHUNK_COUNT, MAX_CHUNKS_HARD + 1);
    let error = parse_error(&archive);
    assert!(error.contains("exceeding the internal safety ceiling"));
    assert!(!error.contains("cannot be allocated"));
}

#[test]
fn compressed_offset_beyond_eof_rejected() {
    let directory = tempfile::tempdir().expect("tempdir");
    let mut archive = make_v2_bytes(&directory, b"offset", 1024);
    let beyond_eof = u64::try_from(archive.len()).expect("length fits") + 1;
    put_u64(
        &mut archive,
        entry_field(0, ENTRY_COMPRESSED_OFFSET),
        beyond_eof,
    );
    assert!(parse_error(&archive).contains("compressed range exceeds file length for chunk 0"));
}

#[test]
fn compressed_offset_plus_size_overflow_rejected() {
    let directory = tempfile::tempdir().expect("tempdir");
    let mut archive = make_v2_bytes(&directory, b"overflow", 1024);
    put_u64(
        &mut archive,
        entry_field(0, ENTRY_COMPRESSED_SIZE),
        u64::MAX,
    );
    assert!(parse_error(&archive).contains("compressed offset plus size overflows for chunk 0"));
}

#[test]
fn compressed_range_beyond_eof_rejected() {
    let directory = tempfile::tempdir().expect("tempdir");
    let mut archive = make_v2_bytes(&directory, b"range", 1024);
    let archive_info = info(&archive);
    let size = archive_info.chunks[0].compressed_size + 1;
    put_u64(&mut archive, entry_field(0, ENTRY_COMPRESSED_SIZE), size);
    assert!(parse_error(&archive).contains("compressed range exceeds file length for chunk 0"));
}

#[test]
fn overlapping_compressed_ranges_rejected() {
    let directory = tempfile::tempdir().expect("tempdir");
    let mut archive = make_v2_bytes(&directory, b"0123456789abcdef", 8);
    let archive_info = info(&archive);
    let first_offset = archive_info.chunks[0].compressed_offset;
    put_u64(
        &mut archive,
        entry_field(1, ENTRY_COMPRESSED_OFFSET),
        first_offset,
    );
    assert!(parse_error(&archive).contains("compressed range for chunk 1 overlaps"));
}

#[test]
fn original_offset_overflow_rejected() {
    let directory = tempfile::tempdir().expect("tempdir");
    let mut archive = make_v2_bytes(&directory, b"overflow", 1024);
    put_u64(
        &mut archive,
        entry_field(0, ENTRY_ORIGINAL_OFFSET),
        u64::MAX,
    );
    assert!(parse_error(&archive).contains("original offset plus size overflows for chunk 0"));
}

#[test]
fn chunk_sizes_not_matching_declared_original_size_rejected() {
    let directory = tempfile::tempdir().expect("tempdir");
    let mut archive = make_v2_bytes(&directory, b"size total", 1024);
    let declared = info(&archive).original_size_bytes + 1;
    put_u64(&mut archive, HEADER_ORIGINAL_SIZE, declared);
    let error = parse_error(&archive);
    assert!(error.contains("chunk table covers"));
    assert!(error.contains(&format!("expected {declared}")));
}

#[test]
fn corrupted_compressed_payload_rejected() {
    let directory = tempfile::tempdir().expect("tempdir");
    let mut archive = make_v2_bytes(&directory, b"payload corruption", 1024);
    damage_zstd_frame(&mut archive, 0);
    let error = decode_error(&directory, &archive, ChunkedDecompressOptions::default());
    assert!(error.contains("zstd decompression failed for chunk 0"));
}

#[test]
fn wrong_per_chunk_sha256_rejected() {
    let directory = tempfile::tempdir().expect("tempdir");
    let mut archive = make_v2_bytes(&directory, b"chunk hash", 1024);
    flip_byte(&mut archive, chunked::chunk_hash_table_offset(0));
    let error = decode_error(&directory, &archive, ChunkedDecompressOptions::default());
    assert!(error.contains("chunk 0 SHA-256 mismatch: expected "));
    assert!(error.contains(", got "));
}

#[test]
fn wrong_global_sha256_rejected() {
    let directory = tempfile::tempdir().expect("tempdir");
    let mut archive = make_v2_bytes(&directory, b"global hash", 1024);
    flip_byte(&mut archive, HEADER_GLOBAL_HASH);
    let error = decode_error(&directory, &archive, ChunkedDecompressOptions::default());
    assert!(error.contains("global SHA-256 mismatch: expected "));
    assert!(error.contains(", got "));
}

#[test]
fn wrong_chunk_hash_names_correct_chunk_index() {
    let directory = tempfile::tempdir().expect("tempdir");
    let mut archive = make_v2_bytes(&directory, b"0123456789abcdef", 8);
    flip_byte(&mut archive, chunked::chunk_hash_table_offset(1));
    let error = decode_error(&directory, &archive, ChunkedDecompressOptions::default());
    assert!(error.contains("chunk 1 SHA-256 mismatch"));
    assert!(!error.contains("chunk 0 SHA-256 mismatch"));
}

#[test]
fn wrong_global_hash_gives_clear_error_message() {
    let directory = tempfile::tempdir().expect("tempdir");
    let mut archive = make_v2_bytes(&directory, b"clear global hash error", 1024);
    flip_byte(&mut archive, HEADER_GLOBAL_HASH + 1);
    let error = decode_error(&directory, &archive, ChunkedDecompressOptions::default());
    assert!(error.starts_with("invalid dpack file: global SHA-256 mismatch"));
}

#[test]
fn wrong_decompressed_size_gives_clear_error_with_chunk_id() {
    let directory = tempfile::tempdir().expect("tempdir");
    let source = b"declared length";
    let mut archive = make_v2_bytes(&directory, source, 1024);
    let wrong_size = u64::try_from(source.len()).expect("length fits") + 1;
    put_u64(&mut archive, HEADER_ORIGINAL_SIZE, wrong_size);
    put_u64(
        &mut archive,
        entry_field(0, ENTRY_ORIGINAL_SIZE),
        wrong_size,
    );
    let error = decode_error(&directory, &archive, ChunkedDecompressOptions::default());
    assert!(error.contains(&format!(
        "decompressed size mismatch for chunk 0: expected {wrong_size}, got {}",
        source.len()
    )));
}

#[test]
fn no_verify_behavior_is_explicit() {
    let directory = tempfile::tempdir().expect("tempdir");
    let source = b"hash checks may be explicitly disabled";
    let mut archive = make_v2_bytes(&directory, source, 1024);
    flip_byte(&mut archive, HEADER_GLOBAL_HASH);
    flip_byte(&mut archive, chunked::chunk_hash_table_offset(0));
    let archive_path = write_archive(&directory, "no-verify.dpack", &archive);
    let output = directory.path().join("no-verify.out");
    chunked::decode_raw_zstd_chunked_file(
        &archive_path,
        &output,
        ChunkedDecompressOptions {
            verify: false,
            ..ChunkedDecompressOptions::default()
        },
    )
    .expect("--no-verify skips only SHA-256 checks");
    assert_eq!(fs::read(output).expect("read restored bytes"), source);
}

#[test]
fn no_verify_does_not_suppress_zstd_decompression_errors() {
    let directory = tempfile::tempdir().expect("tempdir");
    let mut archive = make_v2_bytes(&directory, b"zstd is always enforced", 1024);
    damage_zstd_frame(&mut archive, 0);
    let error = decode_error(
        &directory,
        &archive,
        ChunkedDecompressOptions {
            verify: false,
            ..ChunkedDecompressOptions::default()
        },
    );
    assert!(error.contains("zstd decompression failed for chunk 0"));
}

#[test]
fn empty_file_archive_behavior_explicit() {
    let directory = tempfile::tempdir().expect("tempdir");
    let archive = make_v2_bytes(&directory, b"", 1024);
    let archive_info = info(&archive);
    assert_eq!(archive_info.original_size_bytes, 0);
    assert_eq!(archive_info.chunk_count, 0);
    assert_eq!(archive.len(), V2_HEADER_LEN);

    let archive_path = write_archive(&directory, "empty.dpack", &archive);
    let output = directory.path().join("empty.out");
    chunked::decode_raw_zstd_chunked_file(
        &archive_path,
        &output,
        ChunkedDecompressOptions::default(),
    )
    .expect("empty v2 archive is intentionally supported");
    assert_eq!(
        fs::metadata(output).expect("empty output metadata").len(),
        0
    );
}

#[test]
fn tiny_valid_archive_passes() {
    let directory = tempfile::tempdir().expect("tempdir");
    let source = b"x";
    let archive = make_v2_bytes(&directory, source, 1024);
    let archive_path = write_archive(&directory, "tiny.dpack", &archive);
    let output = directory.path().join("tiny.out");
    chunked::decode_raw_zstd_chunked_file(
        &archive_path,
        &output,
        ChunkedDecompressOptions::default(),
    )
    .expect("tiny v2 archive passes");
    assert_eq!(fs::read(output).expect("read tiny output"), source);
}

#[test]
fn existing_v2_archive_passes() {
    let directory = tempfile::tempdir().expect("tempdir");
    let archive = make_v2_bytes(&directory, b"v2 compatibility", 8);
    let archive_info = info(&archive);
    assert_eq!(archive_info.chunk_count, 2);
    assert_eq!(
        storage::archive_version_from_bytes(&archive).expect("version"),
        2
    );
}

#[test]
fn existing_v1_archive_passes() {
    let directory = tempfile::tempdir().expect("tempdir");
    let input = directory.path().join("legacy.csv");
    let archive = directory.path().join("legacy.dpack");
    let output = directory.path().join("legacy.out");
    let source = b"id,value\n1,legacy\n";
    fs::write(&input, source).expect("write v1 source");
    fs::write(
        &archive,
        storage::encode_raw_zstd_archive(&input, source).expect("encode v1 archive"),
    )
    .expect("write v1 archive");
    let result = run_decompress(&archive, &output, &[]);
    assert!(result.status.success(), "{}", combined_output(&result));
    assert_eq!(fs::read(output).expect("read v1 restored output"), source);
}

#[test]
fn multi_chunk_archive_passes() {
    let directory = tempfile::tempdir().expect("tempdir");
    let source = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let archive = make_v2_bytes(&directory, source, 7);
    assert!(info(&archive).chunk_count > 1);
    let archive_path = write_archive(&directory, "multi.dpack", &archive);
    let output = directory.path().join("multi.out");
    chunked::decode_raw_zstd_chunked_file(
        &archive_path,
        &output,
        ChunkedDecompressOptions::default(),
    )
    .expect("multi-chunk archive passes");
    assert_eq!(fs::read(output).expect("read multi output"), source);
}

#[test]
fn flip_one_byte_in_header_clean_error() {
    let directory = tempfile::tempdir().expect("tempdir");
    let mut archive = make_v2_bytes(&directory, b"header", 1024);
    archive[7] ^= 0x7f;
    assert!(parse_error(&archive).contains("unsupported v2 archive mode"));
}

#[test]
fn flip_one_byte_in_chunk_table_clean_error() {
    let directory = tempfile::tempdir().expect("tempdir");
    let mut archive = make_v2_bytes(&directory, b"table", 1024);
    flip_byte(&mut archive, entry_field(0, ENTRY_FEATURE_BITS));
    assert!(parse_error(&archive).contains("unsupported v2 chunk-table feature bits"));
}

#[test]
fn flip_one_byte_in_compressed_payload_clean_error() {
    let directory = tempfile::tempdir().expect("tempdir");
    let mut archive = make_v2_bytes(&directory, b"payload mutation", 1024);
    let archive_info = info(&archive);
    let chunk = &archive_info.chunks[0];
    let midpoint = chunk.compressed_offset + chunk.compressed_size / 2;
    flip_byte(
        &mut archive,
        usize::try_from(midpoint).expect("payload midpoint fits"),
    );
    let error = decode_error(&directory, &archive, ChunkedDecompressOptions::default());
    assert!(
        error.contains("zstd decompression failed for chunk 0")
            || error.contains("chunk 0 SHA-256 mismatch"),
        "unexpected payload mutation error: {error}"
    );
}

#[test]
fn truncate_at_header_midpoint_clean_error() {
    assert!(parse_error(&[0u8; V2_HEADER_LEN / 2]).contains("truncated v2 header"));
}

#[test]
fn truncate_at_chunk_table_midpoint_clean_error() {
    let directory = tempfile::tempdir().expect("tempdir");
    let archive = make_v2_bytes(&directory, b"table truncation", 1024);
    let truncated = &archive[..V2_HEADER_LEN + V2_CHUNK_ENTRY_LEN / 2];
    assert!(parse_error(truncated).contains("chunk table extends beyond file length"));
}

#[test]
fn truncate_at_payload_midpoint_clean_error() {
    let directory = tempfile::tempdir().expect("tempdir");
    let archive = make_v2_bytes(&directory, b"payload truncation", 1024);
    let archive_info = info(&archive);
    let chunk = &archive_info.chunks[0];
    let end = chunk.compressed_offset + chunk.compressed_size / 2;
    let truncated = &archive[..usize::try_from(end).expect("truncation offset fits")];
    assert!(parse_error(truncated).contains("compressed range exceeds file length"));
}

#[test]
fn cli_truncated_archive_fails_without_committing_output() {
    let directory = tempfile::tempdir().expect("tempdir");
    let mut archive = make_v2_bytes(&directory, b"CLI payload truncation", 1024);
    archive.pop().expect("generated archive contains payload bytes");
    let archive_path = write_archive(&directory, "truncated.dpack", &archive);
    let output = directory.path().join("truncated.out");

    let result = run_decompress(&archive_path, &output, &[]);
    assert_cli_failure(
        result,
        &[
            "compressed range exceeds file length for chunk 0",
            "file length is",
            "output status: no final output was committed",
        ],
    );
    assert!(!output.exists());
    assert!(partial_files(&directory).is_empty());
}

#[test]
fn append_trailing_garbage_rejected() {
    let directory = tempfile::tempdir().expect("tempdir");
    let mut archive = make_v2_bytes(&directory, b"trailing bytes", 1024);
    archive.extend_from_slice(b"garbage");
    assert!(parse_error(&archive).contains("trailing bytes after compressed payloads"));
}

#[test]
fn malicious_archive_absurd_output_size_rejected_with_max_output_mb() {
    let directory = tempfile::tempdir().expect("tempdir");
    let mut archive = make_v2_bytes(&directory, b"small", 1024);
    put_u64(&mut archive, HEADER_ORIGINAL_SIZE, 2 * 1024 * 1024);
    let archive_path = write_archive(&directory, "absurd-output.dpack", &archive);
    let output = directory.path().join("absurd-output.out");
    let result = run_decompress(&archive_path, &output, &["--max-output-mb", "1"]);
    assert_cli_failure(
        result,
        &[
            "--max-output-mb",
            "exceeding",
            "output status: no final output was committed",
        ],
    );
    assert!(!output.exists());
}

#[test]
fn normal_archive_passes_under_max_output_mb_limit() {
    let directory = tempfile::tempdir().expect("tempdir");
    let source = b"normal bounded output";
    let archive = make_v2_bytes(&directory, source, 1024);
    let archive_path = write_archive(&directory, "bounded.dpack", &archive);
    let output = directory.path().join("bounded.out");
    let result = run_decompress(&archive_path, &output, &["--max-output-mb", "1"]);
    assert!(result.status.success(), "{}", combined_output(&result));
    assert_eq!(fs::read(output).expect("read bounded output"), source);
}

#[test]
fn small_archive_passes_without_explicit_limits() {
    let directory = tempfile::tempdir().expect("tempdir");
    let source = b"unlimited by default";
    let archive = make_v2_bytes(&directory, source, 1024);
    let archive_path = write_archive(&directory, "default-limits.dpack", &archive);
    let output = directory.path().join("default-limits.out");
    let result = run_decompress(&archive_path, &output, &[]);
    assert!(result.status.success(), "{}", combined_output(&result));
    assert_eq!(fs::read(output).expect("read default output"), source);
}

#[test]
fn max_chunks_limit_rejects_before_table_allocation() {
    let directory = tempfile::tempdir().expect("tempdir");
    let mut archive = make_v2_bytes(&directory, b"one chunk", 1024);
    put_u64(&mut archive, HEADER_CHUNK_COUNT, 100);
    let error = parse_error_with_limits(
        &archive,
        V2ArchiveLimits {
            max_chunks: Some(10),
            ..V2ArchiveLimits::default()
        },
    );
    assert!(error.contains("exceeding --max-chunks limit of 10"));
    assert!(!error.contains("cannot be allocated"));
}

#[test]
fn max_chunks_cli_limit_is_enforced() {
    let directory = tempfile::tempdir().expect("tempdir");
    let archive = make_v2_bytes(&directory, b"0123456789abcdef", 8);
    let archive_path = write_archive(&directory, "chunks.dpack", &archive);
    let output = directory.path().join("chunks.out");
    let result = run_decompress(&archive_path, &output, &["--max-chunks", "1"]);
    assert_cli_failure(result, &["--max-chunks", "2 chunks"]);
    assert!(!output.exists());
}

#[test]
fn max_memory_cli_limit_is_enforced() {
    let directory = tempfile::tempdir().expect("tempdir");
    let mut archive = make_v2_bytes(&directory, b"small compressed payload", 1_000_000);
    put_u64(&mut archive, HEADER_ORIGINAL_SIZE, 900_000);
    put_u64(&mut archive, entry_field(0, ENTRY_ORIGINAL_SIZE), 900_000);
    let archive_path = write_archive(&directory, "memory.dpack", &archive);
    let output = directory.path().join("memory.out");
    let result = run_decompress(&archive_path, &output, &["--max-memory-mb", "1"]);
    assert_cli_failure(
        result,
        &["estimated decompression memory", "--max-memory-mb"],
    );
    assert!(!output.exists());
}

#[test]
fn invalid_chunk_size_fails_cleanly() {
    let directory = tempfile::tempdir().expect("tempdir");
    let input = cli_source(&directory);
    let output = directory.path().join("bad-chunk.dpack");
    let value = (MAX_CHUNK_SIZE_MB + 1).to_string();
    let result = run_compress(&input, &output, &["--chunked", "--chunk-size-mb", &value]);
    assert_cli_failure(result, &["--chunk-size-mb", "must not exceed"]);
    assert!(!output.exists());
}

#[test]
fn invalid_thread_count_fails_cleanly() {
    let directory = tempfile::tempdir().expect("tempdir");
    let input = cli_source(&directory);
    let output = directory.path().join("bad-threads.dpack");
    let value = (MAX_THREAD_COUNT + 1).to_string();
    let result = run_compress(&input, &output, &["--chunked", "--threads", &value]);
    assert_cli_failure(result, &["--threads", "between 1"]);
    assert!(!output.exists());
}

#[test]
fn invalid_max_in_flight_fails_cleanly() {
    let directory = tempfile::tempdir().expect("tempdir");
    let input = cli_source(&directory);
    let output = directory.path().join("bad-in-flight.dpack");
    let value = (MAX_IN_FLIGHT_CHUNKS + 1).to_string();
    let result = run_compress(
        &input,
        &output,
        &["--chunked", "--max-in-flight-chunks", &value],
    );
    assert_cli_failure(result, &["--max-in-flight-chunks", "between 1"]);
    assert!(!output.exists());
}

#[test]
fn zero_max_in_flight_fails_cleanly() {
    let directory = tempfile::tempdir().expect("tempdir");
    let input = cli_source(&directory);
    let output = directory.path().join("zero-in-flight.dpack");
    let result = run_compress(
        &input,
        &output,
        &["--chunked", "--max-in-flight-chunks", "0"],
    );
    assert_cli_failure(result, &["--max-in-flight-chunks", "between 1"]);
    assert!(!output.exists());
}

#[test]
fn max_in_flight_validation_works() {
    let directory = tempfile::tempdir().expect("tempdir");
    let input = cli_source(&directory);
    let output = directory.path().join("memory-product.dpack");
    let result = run_compress(
        &input,
        &output,
        &[
            "--chunked",
            "--chunk-size-mb",
            "2",
            "--max-in-flight-chunks",
            "2",
            "--max-memory-mb",
            "1",
        ],
    );
    assert_cli_failure(
        result,
        &[
            "--max-in-flight-chunks * --chunk-size-mb",
            "--max-memory-mb",
        ],
    );
    assert!(!output.exists());
}

#[test]
fn input_path_not_found_fails_cleanly() {
    let directory = tempfile::tempdir().expect("tempdir");
    let missing = directory.path().join("missing.csv");
    let output = directory.path().join("missing.dpack");
    let result = run_compress(&missing, &output, &["--chunked"]);
    let missing_text = missing.display().to_string();
    assert_cli_failure(result, &["input path", &missing_text, "not found"]);
    assert!(!output.exists());
}

#[test]
fn output_equals_input_fails_cleanly() {
    let directory = tempfile::tempdir().expect("tempdir");
    let input = cli_source(&directory);
    let original = fs::read(&input).expect("read original input");
    let result = run_compress(&input, &input, &["--chunked"]);
    assert_cli_failure(result, &["must differ from input/archive path"]);
    assert_eq!(fs::read(input).expect("input preserved"), original);
}

#[test]
fn output_equals_archive_fails_cleanly() {
    let directory = tempfile::tempdir().expect("tempdir");
    let archive = make_v2_bytes(&directory, b"collision", 1024);
    let archive_path = write_archive(&directory, "collision.dpack", &archive);
    let original = fs::read(&archive_path).expect("read original archive");
    let result = run_decompress(&archive_path, &archive_path, &[]);
    assert_cli_failure(result, &["must differ from input/archive path"]);
    assert_eq!(fs::read(archive_path).expect("archive preserved"), original);
}

#[test]
fn output_directory_nonexistent_fails_cleanly() {
    let directory = tempfile::tempdir().expect("tempdir");
    let input = cli_source(&directory);
    let output = directory.path().join("missing-dir").join("archive.dpack");
    let result = run_compress(&input, &output, &["--chunked"]);
    assert_cli_failure(result, &["output parent directory", "does not exist"]);
    assert!(!output.exists());
}

#[test]
fn output_file_exists_without_force_fails_cleanly() {
    let directory = tempfile::tempdir().expect("tempdir");
    let archive = make_v2_bytes(&directory, b"protected", 1024);
    let archive_path = write_archive(&directory, "protected.dpack", &archive);
    let output = directory.path().join("protected.out");
    fs::write(&output, b"previous").expect("write previous output");
    let result = run_decompress(&archive_path, &output, &[]);
    assert_cli_failure(result, &["already exists", "--force"]);
    assert_eq!(
        fs::read(output).expect("previous output preserved"),
        b"previous"
    );
}

#[test]
fn invalid_backend_name_fails_cleanly() {
    let directory = tempfile::tempdir().expect("tempdir");
    let input = cli_source(&directory);
    let output = directory.path().join("backend.dpack");
    let result = run_compress(
        &input,
        &output,
        &["--chunked", "--backend", "hostile-backend"],
    );
    assert_cli_failure(result, &["--backend", "hostile-backend"]);
    assert!(!output.exists());
}

#[test]
fn invalid_mode_fails_cleanly() {
    let directory = tempfile::tempdir().expect("tempdir");
    let input = cli_source(&directory);
    let output = directory.path().join("mode.dpack");
    let result = run_compress(&input, &output, &["--mode", "hostile-mode"]);
    assert_cli_failure(result, &["--mode", "hostile-mode"]);
    assert!(!output.exists());
}

#[test]
fn invalid_max_input_mb_fails_cleanly() {
    let directory = tempfile::tempdir().expect("tempdir");
    let input = cli_source(&directory);
    let result = datapack_command()
        .arg("benchmark")
        .arg(&input)
        .args(["--estimate-only", "--max-input-mb", "0"])
        .output()
        .expect("run benchmark validation");
    assert_cli_failure(result, &["--max-input-mb", "greater than zero"]);
}

#[test]
fn invalid_tune_grid_fails_cleanly() {
    let directory = tempfile::tempdir().expect("tempdir");
    let input = cli_source(&directory);
    let result = datapack_command()
        .arg("tune")
        .arg(&input)
        .args(["--chunk-sizes-mb", "1,,2"])
        .output()
        .expect("run tune validation");
    assert_cli_failure(result, &["--chunk-sizes-mb", "without empty values"]);
}

#[test]
fn failed_decompress_leaves_no_final_output() {
    let directory = tempfile::tempdir().expect("tempdir");
    let mut archive = make_v2_bytes(&directory, b"failed restore", 1024);
    damage_zstd_frame(&mut archive, 0);
    let archive_path = write_archive(&directory, "failed.dpack", &archive);
    let output = directory.path().join("failed.out");
    let result = run_decompress(&archive_path, &output, &[]);
    assert_cli_failure(result, &["zstd decompression failed for chunk 0"]);
    assert!(!output.exists());
}

#[test]
fn failed_compress_leaves_no_final_archive() {
    let directory = tempfile::tempdir().expect("tempdir");
    let input = cli_source(&directory);
    let output = directory.path().join("failed-compress.dpack");
    let result = run_compress(&input, &output, &["--chunked", "--chunk-size-mb", "0"]);
    assert_cli_failure(result, &["--chunk-size-mb", "greater than zero"]);
    assert!(!output.exists());
    assert!(partial_files(&directory).is_empty());
}

#[test]
fn existing_output_not_destroyed_on_failure() {
    let directory = tempfile::tempdir().expect("tempdir");
    let mut archive = make_v2_bytes(&directory, b"preserve previous", 1024);
    damage_zstd_frame(&mut archive, 0);
    let archive_path = write_archive(&directory, "preserve.dpack", &archive);
    let output = directory.path().join("preserve.out");
    fs::write(&output, b"previous valid output").expect("write previous output");
    let result = run_decompress(&archive_path, &output, &["--force"]);
    assert_cli_failure(result, &["previous output preserved"]);
    assert_eq!(
        fs::read(output).expect("read previous output"),
        b"previous valid output"
    );
}

#[test]
fn temp_file_cleaned_up_on_failure() {
    let directory = tempfile::tempdir().expect("tempdir");
    let mut archive = make_v2_bytes(&directory, b"cleanup partial", 1024);
    damage_zstd_frame(&mut archive, 0);
    let archive_path = write_archive(&directory, "cleanup.dpack", &archive);
    let output = directory.path().join("cleanup.out");
    let result = run_decompress(&archive_path, &output, &[]);
    assert!(!result.status.success());
    assert!(partial_files(&directory).is_empty());
    assert!(!output.exists());
}

#[test]
fn keep_temp_preserves_temp_file() {
    let directory = tempfile::tempdir().expect("tempdir");
    let mut archive = make_v2_bytes(&directory, b"preserve partial", 1024);
    damage_zstd_frame(&mut archive, 0);
    let archive_path = write_archive(&directory, "keep-temp.dpack", &archive);
    let output = directory.path().join("keep-temp.out");
    let result = run_decompress(&archive_path, &output, &["--keep-temp"]);
    assert_cli_failure(result, &["zstd decompression failed for chunk 0"]);
    let partials = partial_files(&directory);
    assert_eq!(partials.len(), 1, "expected one preserved partial file");
    assert!(!output.exists());
}

#[test]
fn force_overwrites_correctly() {
    let directory = tempfile::tempdir().expect("tempdir");
    let source = b"verified replacement";
    let archive = make_v2_bytes(&directory, source, 1024);
    let archive_path = write_archive(&directory, "force.dpack", &archive);
    let output = directory.path().join("force.out");
    fs::write(&output, b"previous").expect("write previous output");
    let result = run_decompress(&archive_path, &output, &["--force"]);
    assert!(result.status.success(), "{}", combined_output(&result));
    assert_eq!(fs::read(output).expect("read replacement"), source);
}

#[test]
fn input_output_same_path_fails() {
    let directory = tempfile::tempdir().expect("tempdir");
    let input = cli_source(&directory);
    let result = run_compress(&input, &input, &["--chunked"]);
    assert_cli_failure(result, &["must differ from input/archive path"]);
}

#[test]
fn archive_output_same_path_fails() {
    let directory = tempfile::tempdir().expect("tempdir");
    let archive = make_v2_bytes(&directory, b"same archive", 1024);
    let archive_path = write_archive(&directory, "same.dpack", &archive);
    let result = run_decompress(&archive_path, &archive_path, &[]);
    assert_cli_failure(result, &["must differ from input/archive path"]);
}

#[test]
fn invalid_parent_directory_fails_cleanly() {
    let directory = tempfile::tempdir().expect("tempdir");
    let archive = make_v2_bytes(&directory, b"missing parent", 1024);
    let archive_path = write_archive(&directory, "parent.dpack", &archive);
    let output = directory.path().join("absent").join("restored.out");
    let result = run_decompress(&archive_path, &output, &[]);
    assert_cli_failure(result, &["output parent directory", "does not exist"]);
    assert!(!output.exists());
}

#[test]
fn archive_parse_error_includes_path() {
    let directory = tempfile::tempdir().expect("tempdir");
    let mut archive = make_v2_bytes(&directory, b"bad parse", 1024);
    archive[0] = b'X';
    let archive_path = write_archive(&directory, "path-context.dpack", &archive);
    let output = directory.path().join("path-context.out");
    let result = run_decompress(&archive_path, &output, &[]);
    let path = archive_path.display().to_string();
    assert_cli_failure(result, &["archive parse error", &path, "bad archive magic"]);
}

#[test]
fn chunk_error_includes_chunk_id() {
    let directory = tempfile::tempdir().expect("tempdir");
    let mut archive = make_v2_bytes(&directory, b"0123456789abcdef", 8);
    damage_zstd_frame(&mut archive, 1);
    let error = decode_error(&directory, &archive, ChunkedDecompressOptions::default());
    assert!(error.contains("zstd decompression failed for chunk 1"));
}

#[test]
fn hash_mismatch_error_is_specific() {
    let directory = tempfile::tempdir().expect("tempdir");
    let mut archive = make_v2_bytes(&directory, b"specific hash", 1024);
    flip_byte(&mut archive, chunked::chunk_hash_table_offset(0));
    let error = decode_error(&directory, &archive, ChunkedDecompressOptions::default());
    assert!(error.contains("chunk 0 SHA-256 mismatch: expected"));
    assert!(error.contains("got"));
}

#[test]
fn cli_validation_error_includes_flag_name() {
    let directory = tempfile::tempdir().expect("tempdir");
    let input = cli_source(&directory);
    let output = directory.path().join("flag.dpack");
    let result = run_compress(&input, &output, &["--chunked", "--threads", "0"]);
    assert_cli_failure(result, &["--threads", "between 1"]);
}

#[test]
fn file_operation_error_includes_path() {
    let directory = tempfile::tempdir().expect("tempdir");
    let missing = directory.path().join("specific-missing.dpack");
    let output = directory.path().join("missing.out");
    let result = run_decompress(&missing, &output, &[]);
    let path = missing.display().to_string();
    assert_cli_failure(result, &["input path", &path, "not found"]);
}

#[test]
fn failed_operation_reports_output_status() {
    let directory = tempfile::tempdir().expect("tempdir");
    let mut archive = make_v2_bytes(&directory, b"status context", 1024);
    damage_zstd_frame(&mut archive, 0);
    let archive_path = write_archive(&directory, "status.dpack", &archive);
    let output = directory.path().join("status.out");
    let result = run_decompress(&archive_path, &output, &[]);
    assert_cli_failure(
        result,
        &[
            "decompression failed for input",
            "output status: no final output was committed",
        ],
    );
    assert!(!output.exists());
}
