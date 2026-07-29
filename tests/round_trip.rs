use std::fs;
use std::process::Command;

use datapack::generation::{self, Profile};
use datapack::metadata::PayloadKind;
use datapack::storage;
use sha2::Digest;

#[test]
fn csv_compress_decompress_round_trip() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("input.csv");
    let archive_path = temp.path().join("input.dpack");
    let output = temp.path().join("restored.csv");
    let bytes = b"id,name,status\r\n1,Ada,active\r\n2,Grace,active\r\n3,Linus,inactive\r\n";
    fs::write(&input, bytes).unwrap();

    let encoded = storage::encode_adaptive_archive(&input, bytes).unwrap();
    fs::write(&archive_path, encoded).unwrap();

    let archive = storage::decode_archive(&fs::read(&archive_path).unwrap()).unwrap();
    let restored = storage::restore_archive(&archive).unwrap();
    fs::write(&output, &restored).unwrap();

    assert_eq!(fs::read(&output).unwrap(), bytes);
}

#[test]
fn txt_compress_decompress_round_trip() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("sample_log.txt");
    let bytes = b"INFO boot complete\nINFO boot complete\nWARN retry database connection\n";
    fs::write(&input, bytes).unwrap();

    let encoded = storage::encode_adaptive_archive(&input, bytes).unwrap();
    let archive = storage::decode_archive(&encoded).unwrap();
    let restored = storage::restore_archive(&archive).unwrap();

    assert_eq!(restored, bytes);
}

#[test]
fn fast_mode_realistic_profile_round_trips() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("realistic.csv");
    let archive_path = temp.path().join("realistic.dpack");
    let output = temp.path().join("realistic_restored.csv");
    generation::generate_to_path(Profile::Realistic, &input, 1_000, Some(2026)).unwrap();

    let status = Command::new(env!("CARGO_BIN_EXE_datapack"))
        .args([
            "compress",
            input.to_str().unwrap(),
            archive_path.to_str().unwrap(),
            "--mode",
            "fast",
        ])
        .status()
        .unwrap();
    assert!(status.success());

    let archive = storage::decode_archive(&fs::read(&archive_path).unwrap()).unwrap();
    let restored = storage::restore_archive(&archive).unwrap();
    assert_eq!(restored, fs::read(&input).unwrap());

    let status = Command::new(env!("CARGO_BIN_EXE_datapack"))
        .args([
            "decompress",
            archive_path.to_str().unwrap(),
            output.to_str().unwrap(),
        ])
        .status()
        .unwrap();
    assert!(status.success());
    assert_eq!(fs::read(&output).unwrap(), fs::read(&input).unwrap());
}

#[test]
fn benchmark_realistic_profile_exits_zero_and_validates_hash() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("realistic.csv");
    generation::generate_to_path(Profile::Realistic, &input, 1_000, Some(2026)).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_datapack"))
        .args(["benchmark", input.to_str().unwrap()])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("roundtrip_sha256_match"));
    assert!(stdout.contains("true"));
    assert!(stdout.contains("selected_mode"));
}

#[test]
fn benchmark_quick_validates_sha256_and_reports_one_run() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("repetitive.csv");
    generation::generate_to_path(Profile::Repetitive, &input, 500, Some(42)).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_datapack"))
        .args(["benchmark", input.to_str().unwrap(), "--quick"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert_eq!(metric_value(&stdout, "runs_used"), Some("1"));
    assert_eq!(
        metric_value(&stdout, "roundtrip_sha256_match"),
        Some("true")
    );
}

#[test]
fn benchmark_runs_one_reports_runs_used_one() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("repetitive.csv");
    generation::generate_to_path(Profile::Repetitive, &input, 500, Some(42)).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_datapack"))
        .args(["benchmark", input.to_str().unwrap(), "--runs", "1"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert_eq!(metric_value(&stdout, "runs_used"), Some("1"));
}

#[test]
fn benchmark_partial_flags_are_explicitly_non_validating() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("high_cardinality.csv");
    generation::generate_to_path(Profile::HighCardinality, &input, 500, Some(9)).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_datapack"))
        .args([
            "benchmark",
            input.to_str().unwrap(),
            "--quick",
            "--no-zstd-baseline",
            "--no-roundtrip",
            "--no-hash",
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert_eq!(metric_value(&stdout, "benchmark_scope"), Some("partial"));
    assert_eq!(
        metric_value(&stdout, "validation_status"),
        Some("not_validated")
    );
    assert_eq!(
        metric_value(&stdout, "zstd_baseline_performed"),
        Some("false")
    );
    assert_eq!(
        metric_value(&stdout, "roundtrip_sha256_match"),
        Some("unavailable")
    );
    assert!(stdout.contains("output identity not validated"));
}

#[test]
fn benchmark_estimate_only_is_marked_non_validating() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("estimate.csv");
    generation::generate_to_path(Profile::Realistic, &input, 200, Some(12)).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_datapack"))
        .args(["benchmark", input.to_str().unwrap(), "--estimate-only"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert_eq!(
        metric_value(&stdout, "benchmark_scope"),
        Some("estimate_only")
    );
    assert_eq!(
        metric_value(&stdout, "validation_status"),
        Some("not_validated")
    );
    assert_eq!(
        metric_value(&stdout, "datapack_size_bytes"),
        Some("unavailable")
    );
}

#[test]
fn benchmark_without_zstd_is_partial_but_still_sha256_validated() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("validated_partial.csv");
    generation::generate_to_path(Profile::Realistic, &input, 250, Some(13)).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_datapack"))
        .args([
            "benchmark",
            input.to_str().unwrap(),
            "--quick",
            "--no-zstd-baseline",
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert_eq!(metric_value(&stdout, "benchmark_scope"), Some("partial"));
    assert_eq!(
        metric_value(&stdout, "validation_status"),
        Some("validated")
    );
    assert_eq!(
        metric_value(&stdout, "roundtrip_sha256_match"),
        Some("true")
    );
}

#[test]
fn benchmark_max_input_mb_limits_measured_prefix() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("prefix.csv");
    fs::write(&input, repeated_bytes(1_100_000)).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_datapack"))
        .args([
            "benchmark",
            input.to_str().unwrap(),
            "--quick",
            "--max-input-mb",
            "1",
            "--no-zstd-baseline",
            "--no-roundtrip",
            "--no-hash",
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert_eq!(metric_value(&stdout, "source_size_bytes"), Some("1100000"));
    assert_eq!(
        metric_value(&stdout, "original_size_bytes"),
        Some("1048576")
    );
    assert_eq!(
        metric_value(&stdout, "measured_input_size_bytes"),
        Some("1048576")
    );
    assert_eq!(metric_value(&stdout, "input_sampled"), Some("true"));
    assert_eq!(metric_value(&stdout, "benchmark_scope"), Some("sampled"));
    assert!(stdout.contains("configured input prefix"));
}

#[test]
fn benchmark_sampled_roundtrip_is_partially_validated() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("validated_prefix.csv");
    fs::write(&input, repeated_bytes(1_100_000)).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_datapack"))
        .args([
            "benchmark",
            input.to_str().unwrap(),
            "--quick",
            "--max-input-mb",
            "1",
            "--no-zstd-baseline",
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert_eq!(metric_value(&stdout, "benchmark_scope"), Some("sampled"));
    assert_eq!(
        metric_value(&stdout, "validation_status"),
        Some("partially_validated")
    );
    assert_eq!(
        metric_value(&stdout, "roundtrip_sha256_match"),
        Some("true")
    );
}

#[test]
fn benchmark_no_roundtrip_alone_is_non_validating() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("no_roundtrip.csv");
    generation::generate_to_path(Profile::Realistic, &input, 100, Some(31)).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_datapack"))
        .args([
            "benchmark",
            input.to_str().unwrap(),
            "--quick",
            "--no-zstd-baseline",
            "--no-roundtrip",
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert_eq!(
        metric_value(&stdout, "validation_status"),
        Some("not_validated")
    );
    assert_eq!(metric_value(&stdout, "roundtrip_performed"), Some("false"));
    assert_eq!(metric_value(&stdout, "hash_performed"), Some("false"));
}

#[test]
fn benchmark_no_hash_alone_decompresses_but_does_not_validate() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("no_hash.csv");
    generation::generate_to_path(Profile::Realistic, &input, 100, Some(32)).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_datapack"))
        .args([
            "benchmark",
            input.to_str().unwrap(),
            "--quick",
            "--no-zstd-baseline",
            "--no-hash",
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert_eq!(
        metric_value(&stdout, "validation_status"),
        Some("not_validated")
    );
    assert_eq!(metric_value(&stdout, "roundtrip_performed"), Some("true"));
    assert_eq!(metric_value(&stdout, "hash_performed"), Some("false"));
}

#[test]
fn benchmark_json_is_parseable_and_reports_backend_throughput() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("json.csv");
    generation::generate_to_path(Profile::HighCardinality, &input, 100, Some(33)).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_datapack"))
        .args([
            "benchmark",
            input.to_str().unwrap(),
            "--quick",
            "--json",
            "--no-zstd-baseline",
            "--chunked",
            "--chunk-size-mb",
            "1",
            "--threads",
            "1",
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["benchmark_scope"], "partial");
    assert_eq!(report["validation_status"], "validated");
    assert_eq!(
        report["measured_input_size_bytes"],
        report["original_size_bytes"]
    );
    assert!(report["chunked_compression_mb_per_sec"].is_number());
    assert!(report["chunked_decompression_mb_per_sec"].is_number());
    assert!(report["zstd_only_compression_mb_per_sec"].is_null());
}

#[test]
fn direct_raw_zstd_profiles_and_round_trips_v1() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("raw.csv");
    let archive_path = temp.path().join("raw.dpack");
    let restored_path = temp.path().join("raw_restored.csv");
    generation::generate_to_path(Profile::HighCardinality, &input, 750, Some(21)).unwrap();

    let compressed = Command::new(env!("CARGO_BIN_EXE_datapack"))
        .args([
            "compress",
            input.to_str().unwrap(),
            archive_path.to_str().unwrap(),
            "--mode",
            "fast",
            "--profile",
        ])
        .output()
        .unwrap();
    assert!(compressed.status.success());
    let compress_stderr = String::from_utf8(compressed.stderr).unwrap();
    assert!(compress_stderr.contains("operation=compress"));
    assert!(compress_stderr.contains("selected_mode=RawZstd"));
    assert_eq!(
        storage::archive_version_from_path(&archive_path).unwrap(),
        1
    );

    let decompressed = Command::new(env!("CARGO_BIN_EXE_datapack"))
        .args([
            "decompress",
            archive_path.to_str().unwrap(),
            restored_path.to_str().unwrap(),
            "--profile",
        ])
        .output()
        .unwrap();
    assert!(decompressed.status.success());
    let decompress_stderr = String::from_utf8(decompressed.stderr).unwrap();
    assert!(decompress_stderr.contains("operation=decompress"));
    assert!(decompress_stderr.contains("selected_mode=RawZstd"));
    assert_eq!(fs::read(restored_path).unwrap(), fs::read(input).unwrap());
}

#[test]
fn quoted_csv_sha256_round_trip() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("quoted.csv");
    let bytes = "id,note,empty\r\n1,\"hello, world\",\r\n2,\"said \"\"hi\"\"\",東京\r\n".as_bytes();
    fs::write(&input, bytes).unwrap();

    let archive_bytes = storage::encode_columnar_dictionary_archive(&input, bytes)
        .unwrap()
        .unwrap();
    let archive = storage::decode_archive(&archive_bytes).unwrap();
    let restored = storage::restore_archive(&archive).unwrap();

    assert_eq!(sha2::Sha256::digest(bytes), sha2::Sha256::digest(&restored));
}

#[test]
fn malformed_quotes_fall_back_to_raw_zstd() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("malformed.csv");
    let bytes = b"id,note\r\n1,\"unterminated\r\n2,ok\r\n";
    fs::write(&input, bytes).unwrap();

    let archive_bytes = storage::encode_adaptive_archive(&input, bytes).unwrap();
    let archive = storage::decode_archive(&archive_bytes).unwrap();
    let restored = storage::restore_archive(&archive).unwrap();

    assert_eq!(archive.metadata.payload_kind, PayloadKind::RawZstd);
    assert_eq!(restored, bytes);
}

#[test]
fn v1_archive_still_decompresses_after_v2_support() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("v1.csv");
    let archive_path = temp.path().join("v1.dpack");
    let output = temp.path().join("v1_restored.csv");
    let bytes = b"id,name\r\n1,Ada\r\n2,Grace\r\n";
    fs::write(&input, bytes).unwrap();
    let archive_bytes = storage::encode_raw_zstd_archive(&input, bytes).unwrap();
    fs::write(&archive_path, archive_bytes).unwrap();

    let status = Command::new(env!("CARGO_BIN_EXE_datapack"))
        .args([
            "decompress",
            archive_path.to_str().unwrap(),
            output.to_str().unwrap(),
        ])
        .status()
        .unwrap();

    assert!(status.success());
    assert_eq!(fs::read(output).unwrap(), bytes);
}

#[test]
fn v2_chunked_raw_zstd_round_trip_small_file() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("small.bin");
    let archive = temp.path().join("small.dpack");
    let output = temp.path().join("small.out");
    let bytes = b"alpha,beta,gamma\r\n1,2,3\r\n";
    fs::write(&input, bytes).unwrap();

    storage::chunked::encode_raw_zstd_chunked_file(&input, &archive, chunk_options(1024, 1, false))
        .unwrap();
    storage::chunked::decode_raw_zstd_chunked_file(
        &archive,
        &output,
        storage::chunked::ChunkedDecompressOptions {
            verify: true,
            ..storage::chunked::ChunkedDecompressOptions::default()
        },
    )
    .unwrap();

    assert_eq!(fs::read(output).unwrap(), bytes);
}

#[test]
fn v2_multiple_chunks_round_trip() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("multi.bin");
    let archive = temp.path().join("multi.dpack");
    let output = temp.path().join("multi.out");
    let bytes = repeated_bytes(20_000);
    fs::write(&input, &bytes).unwrap();

    let stats = storage::chunked::encode_raw_zstd_chunked_file(
        &input,
        &archive,
        chunk_options(4096, 2, false),
    )
    .unwrap();
    storage::chunked::decode_raw_zstd_chunked_file(
        &archive,
        &output,
        storage::chunked::ChunkedDecompressOptions {
            verify: true,
            ..storage::chunked::ChunkedDecompressOptions::default()
        },
    )
    .unwrap();

    assert!(stats.chunk_count > 1);
    assert_eq!(fs::read(output).unwrap(), bytes);
}

#[test]
fn v2_deterministic_output_for_same_input_and_options() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("deterministic.bin");
    let archive_a = temp.path().join("a.dpack");
    let archive_b = temp.path().join("b.dpack");
    let bytes = repeated_bytes(25_000);
    fs::write(&input, bytes).unwrap();

    let options = chunk_options(2048, 2, true);
    storage::chunked::encode_raw_zstd_chunked_file(&input, &archive_a, options).unwrap();
    storage::chunked::encode_raw_zstd_chunked_file(&input, &archive_b, options).unwrap();

    assert_eq!(fs::read(archive_a).unwrap(), fs::read(archive_b).unwrap());
}

#[test]
fn v2_corrupted_chunk_hash_fails_cleanly() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("hash.bin");
    let archive = temp.path().join("hash.dpack");
    let output = temp.path().join("hash.out");
    let bytes = repeated_bytes(10_000);
    fs::write(&input, bytes).unwrap();
    storage::chunked::encode_raw_zstd_chunked_file(&input, &archive, chunk_options(4096, 1, false))
        .unwrap();

    let mut archive_bytes = fs::read(&archive).unwrap();
    let hash_offset = storage::chunked::chunk_hash_table_offset(0);
    archive_bytes[hash_offset] ^= 0x80;
    fs::write(&archive, archive_bytes).unwrap();
    let error = storage::chunked::decode_raw_zstd_chunked_file(
        &archive,
        &output,
        storage::chunked::ChunkedDecompressOptions {
            verify: true,
            ..storage::chunked::ChunkedDecompressOptions::default()
        },
    )
    .unwrap_err();

    assert!(error.to_string().contains("chunk 0 SHA-256 mismatch"));
}

#[test]
fn cli_chunk_size_mb_writes_v2_chunked_archive() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("chunk_size.csv");
    let archive = temp.path().join("chunk_size.dpack");
    let output = temp.path().join("chunk_size.out");
    fs::write(&input, repeated_bytes(1_100_000)).unwrap();

    let status = Command::new(env!("CARGO_BIN_EXE_datapack"))
        .args([
            "compress",
            input.to_str().unwrap(),
            archive.to_str().unwrap(),
            "--mode",
            "fast",
            "--chunk-size-mb",
            "1",
        ])
        .status()
        .unwrap();
    assert!(status.success());
    assert_eq!(
        storage::archive_version_from_path(&archive).unwrap(),
        storage::chunked::CHUNKED_VERSION
    );

    let status = Command::new(env!("CARGO_BIN_EXE_datapack"))
        .args([
            "decompress",
            archive.to_str().unwrap(),
            output.to_str().unwrap(),
        ])
        .status()
        .unwrap();
    assert!(status.success());
    assert_eq!(fs::read(output).unwrap(), fs::read(input).unwrap());
}

#[test]
fn v2_threads_one_and_two_round_trip() {
    for threads in [1usize, 2] {
        let temp = tempfile::tempdir().unwrap();
        let input = temp.path().join(format!("threads_{threads}.bin"));
        let archive = temp.path().join(format!("threads_{threads}.dpack"));
        let output = temp.path().join(format!("threads_{threads}.out"));
        let bytes = repeated_bytes(16_000);
        fs::write(&input, &bytes).unwrap();

        storage::chunked::encode_raw_zstd_chunked_file(
            &input,
            &archive,
            chunk_options(2048, threads, false),
        )
        .unwrap();
        storage::chunked::decode_raw_zstd_chunked_file(
            &archive,
            &output,
            storage::chunked::ChunkedDecompressOptions {
                verify: true,
                ..storage::chunked::ChunkedDecompressOptions::default()
            },
        )
        .unwrap();

        assert_eq!(fs::read(output).unwrap(), bytes);
    }
}

#[test]
fn v2_native_zstd_mt_is_compatible_deterministic_and_verified() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("native_mt.bin");
    let archive_a = temp.path().join("native_mt_a.dpack");
    let archive_b = temp.path().join("native_mt_b.dpack");
    let output = temp.path().join("native_mt.out");
    let bytes = repeated_bytes(3 * 1024 * 1024);
    fs::write(&input, &bytes).unwrap();

    let options = storage::chunked::ChunkedCompressOptions::new_with_max_in_flight(
        1024 * 1024,
        2,
        4,
        false,
        false,
    )
    .unwrap()
    .with_backend(storage::chunked::ChunkedBackend::ZstdMtExperimental)
    .unwrap();
    storage::chunked::encode_raw_zstd_chunked_file(&input, &archive_a, options).unwrap();
    storage::chunked::encode_raw_zstd_chunked_file(&input, &archive_b, options).unwrap();
    storage::chunked::decode_raw_zstd_chunked_file(
        &archive_a,
        &output,
        storage::chunked::ChunkedDecompressOptions {
            verify: true,
            ..storage::chunked::ChunkedDecompressOptions::default()
        },
    )
    .unwrap();

    assert_eq!(fs::read(&output).unwrap(), bytes);
    assert_eq!(fs::read(&archive_a).unwrap(), fs::read(&archive_b).unwrap());
}

#[test]
fn cli_native_zstd_mt_profile_reports_backend_and_round_trips() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("native_cli.csv");
    let archive = temp.path().join("native_cli.dpack");
    let output = temp.path().join("native_cli.out");
    fs::write(&input, repeated_bytes(2 * 1024 * 1024)).unwrap();

    let compressed = Command::new(env!("CARGO_BIN_EXE_datapack"))
        .args([
            "compress",
            input.to_str().unwrap(),
            archive.to_str().unwrap(),
            "--backend",
            "zstd-mt-experimental",
            "--threads",
            "2",
            "--chunk-size-mb",
            "1",
            "--max-in-flight-chunks",
            "2",
            "--profile",
        ])
        .output()
        .unwrap();
    assert!(compressed.status.success());
    let stderr = String::from_utf8(compressed.stderr).unwrap();
    assert!(stderr.contains("backend=zstd-mt-experimental"));
    assert!(stderr.contains("max_in_flight_chunks=2"));

    let status = Command::new(env!("CARGO_BIN_EXE_datapack"))
        .args([
            "decompress",
            archive.to_str().unwrap(),
            output.to_str().unwrap(),
        ])
        .status()
        .unwrap();
    assert!(status.success());
    assert_eq!(fs::read(output).unwrap(), fs::read(input).unwrap());
}

#[test]
fn cli_rejects_same_input_and_output_and_invalid_pipeline_values() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("safety.csv");
    let archive = temp.path().join("safety.dpack");
    fs::write(&input, repeated_bytes(4096)).unwrap();

    let same_path = Command::new(env!("CARGO_BIN_EXE_datapack"))
        .args(["compress", input.to_str().unwrap(), input.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!same_path.status.success());
    assert!(String::from_utf8(same_path.stderr)
        .unwrap()
        .contains("must differ"));

    for (flag, value) in [("--threads", "0"), ("--max-in-flight-chunks", "0")] {
        let invalid = Command::new(env!("CARGO_BIN_EXE_datapack"))
            .args([
                "compress",
                input.to_str().unwrap(),
                archive.to_str().unwrap(),
                "--chunked",
                flag,
                value,
            ])
            .output()
            .unwrap();
        assert!(!invalid.status.success());
        assert!(String::from_utf8(invalid.stderr).unwrap().contains(flag));
    }
}

#[test]
fn v2_failed_verification_preserves_existing_output_and_no_verify_is_explicit() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("source.bin");
    let archive = temp.path().join("source.dpack");
    let output = temp.path().join("existing.out");
    let bytes = repeated_bytes(20_000);
    fs::write(&input, &bytes).unwrap();
    storage::chunked::encode_raw_zstd_chunked_file(&input, &archive, chunk_options(4096, 2, false))
        .unwrap();
    let mut archive_bytes = fs::read(&archive).unwrap();
    archive_bytes[storage::chunked::chunk_hash_table_offset(0)] ^= 0x40;
    fs::write(&archive, archive_bytes).unwrap();
    fs::write(&output, b"preserve me").unwrap();

    assert!(storage::chunked::decode_raw_zstd_chunked_file(
        &archive,
        &output,
        storage::chunked::ChunkedDecompressOptions {
            verify: true,
            ..storage::chunked::ChunkedDecompressOptions::default()
        },
    )
    .is_err());
    assert_eq!(fs::read(&output).unwrap(), b"preserve me");

    storage::chunked::decode_raw_zstd_chunked_file(
        &archive,
        &output,
        storage::chunked::ChunkedDecompressOptions {
            verify: false,
            force: true,
            ..storage::chunked::ChunkedDecompressOptions::default()
        },
    )
    .unwrap();
    assert_eq!(fs::read(output).unwrap(), bytes);
}

#[test]
fn tune_cli_writes_expected_csv_and_cleans_temporary_files() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("tune.csv");
    let report = temp.path().join("tune-report.csv");
    fs::write(&input, repeated_bytes(64 * 1024)).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_datapack"))
        .args([
            "tune",
            input.to_str().unwrap(),
            "--output",
            report.to_str().unwrap(),
            "--chunk-sizes-mb",
            "1",
            "--threads-list",
            "1",
            "--runs",
            "1",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("Best throughput:"));
    assert!(stdout.contains("Best compression ratio:"));
    assert!(stdout.contains("Balanced:"));

    let csv = fs::read_to_string(&report).unwrap();
    let mut lines = csv.lines();
    let header = lines.next().unwrap();
    let row = lines.next().unwrap();
    for required in [
        "timestamp",
        "datapack_version",
        "measured_input_size_bytes",
        "max_in_flight_chunks",
        "compression_mb_per_sec",
        "validation_status",
        "benchmark_scope",
        "error",
    ] {
        assert!(header.split(',').any(|field| field == required));
    }
    assert_eq!(
        csv_column(header, row, "validation_status"),
        Some("validated")
    );
    assert_eq!(csv_column(header, row, "benchmark_scope"), Some("full"));
    assert_eq!(csv_column(header, row, "sha256_match"), Some("true"));

    let unexpected: Vec<_> = fs::read_dir(temp.path())
        .unwrap()
        .filter_map(|entry| {
            let name = entry.ok()?.file_name().to_string_lossy().into_owned();
            name.starts_with(".datapack-tune").then_some(name)
        })
        .collect();
    assert!(
        unexpected.is_empty(),
        "temporary files remain: {unexpected:?}"
    );
}

#[test]
fn tune_cli_sample_and_skip_roundtrip_are_labeled_correctly() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("sample.csv");
    let sampled_report = temp.path().join("sampled.csv");
    let partial_report = temp.path().join("partial.csv");
    fs::write(&input, repeated_bytes(1_100_000)).unwrap();

    let sampled = Command::new(env!("CARGO_BIN_EXE_datapack"))
        .args([
            "tune",
            input.to_str().unwrap(),
            "--output",
            sampled_report.to_str().unwrap(),
            "--chunk-sizes-mb",
            "1",
            "--threads-list",
            "1",
            "--max-input-mb",
            "1",
        ])
        .output()
        .unwrap();
    assert!(sampled.status.success());
    let csv = fs::read_to_string(sampled_report).unwrap();
    let mut lines = csv.lines();
    let header = lines.next().unwrap();
    let row = lines.next().unwrap();
    assert_eq!(csv_column(header, row, "input_sampled"), Some("true"));
    assert_eq!(csv_column(header, row, "benchmark_scope"), Some("sampled"));
    assert_eq!(
        csv_column(header, row, "validation_status"),
        Some("partially_validated")
    );

    let partial = Command::new(env!("CARGO_BIN_EXE_datapack"))
        .args([
            "tune",
            input.to_str().unwrap(),
            "--output",
            partial_report.to_str().unwrap(),
            "--chunk-sizes-mb",
            "1",
            "--threads-list",
            "1",
            "--skip-roundtrip",
        ])
        .output()
        .unwrap();
    assert!(partial.status.success());
    let csv = fs::read_to_string(partial_report).unwrap();
    let mut lines = csv.lines();
    let header = lines.next().unwrap();
    let row = lines.next().unwrap();
    assert_eq!(csv_column(header, row, "benchmark_scope"), Some("partial"));
    assert_eq!(
        csv_column(header, row, "validation_status"),
        Some("not_validated")
    );
    assert_eq!(csv_column(header, row, "sha256_match"), Some(""));
}

#[test]
fn tune_cli_keep_temp_force_and_invalid_grids_are_explicit() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("grid.csv");
    let report = temp.path().join("grid-report.csv");
    fs::write(&input, repeated_bytes(32 * 1024)).unwrap();

    let kept = Command::new(env!("CARGO_BIN_EXE_datapack"))
        .args([
            "tune",
            input.to_str().unwrap(),
            "--output",
            report.to_str().unwrap(),
            "--chunk-sizes-mb",
            "1",
            "--threads-list",
            "1",
            "--skip-roundtrip",
            "--keep-temp",
        ])
        .output()
        .unwrap();
    assert!(kept.status.success());
    assert!(fs::read_dir(temp.path()).unwrap().any(|entry| entry
        .unwrap()
        .file_name()
        .to_string_lossy()
        .starts_with(".datapack-tune")));

    let refuses_overwrite = Command::new(env!("CARGO_BIN_EXE_datapack"))
        .args([
            "tune",
            input.to_str().unwrap(),
            "--output",
            report.to_str().unwrap(),
            "--chunk-sizes-mb",
            "1",
            "--threads-list",
            "1",
        ])
        .output()
        .unwrap();
    assert!(!refuses_overwrite.status.success());
    assert!(String::from_utf8(refuses_overwrite.stderr)
        .unwrap()
        .contains("--force"));

    for (flag, value) in [("--chunk-sizes-mb", "0"), ("--threads-list", "0")] {
        let invalid_report = temp.path().join(format!("invalid-{}.csv", &flag[2..]));
        let invalid = Command::new(env!("CARGO_BIN_EXE_datapack"))
            .args([
                "tune",
                input.to_str().unwrap(),
                "--output",
                invalid_report.to_str().unwrap(),
                flag,
                value,
            ])
            .output()
            .unwrap();
        assert!(!invalid.status.success());
        assert!(String::from_utf8(invalid.stderr)
            .unwrap()
            .contains("greater than zero"));
    }
}

fn csv_column<'a>(header: &'a str, row: &'a str, name: &str) -> Option<&'a str> {
    let index = header.split(',').position(|field| field == name)?;
    row.split(',').nth(index)
}

fn metric_value<'a>(stdout: &'a str, metric: &str) -> Option<&'a str> {
    stdout
        .lines()
        .find(|line| line.split_whitespace().next() == Some(metric))
        .and_then(|line| line.split_whitespace().nth(1))
}

fn chunk_options(
    chunk_size_bytes: usize,
    threads: usize,
    adaptive_level: bool,
) -> storage::chunked::ChunkedCompressOptions {
    storage::chunked::ChunkedCompressOptions::new(chunk_size_bytes, threads, adaptive_level, false)
        .unwrap()
}

fn repeated_bytes(len: usize) -> Vec<u8> {
    let pattern = b"id,name,status\r\n1,Ada,active\r\n2,Grace,inactive\r\n";
    let mut output = Vec::with_capacity(len);
    while output.len() < len {
        let take = (len - output.len()).min(pattern.len());
        output.extend_from_slice(&pattern[..take]);
    }
    output
}
