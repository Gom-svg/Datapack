#![no_main]

use std::path::PathBuf;
use std::sync::OnceLock;

use libfuzzer_sys::fuzz_target;

const ORIGINAL: &[u8] = b"id,name\n1,Ada\n2,Grace\n";
const MAX_MUTATION_BYTES: usize = 4 * 1024;
const MAX_FUZZ_OUTPUT_BYTES: u64 = 1024 * 1024;
const MAX_FUZZ_MEMORY_BYTES: u64 = 2 * 1024 * 1024;
const MAX_FUZZ_CHUNKS: u64 = 64;

static SEED_ARCHIVE: OnceLock<Option<Vec<u8>>> = OnceLock::new();

fn work_dir() -> PathBuf {
    std::env::temp_dir().join(format!("datapack-v2-fuzz-{}", std::process::id()))
}

fn build_seed_archive() -> Option<Vec<u8>> {
    let directory = work_dir();
    std::fs::create_dir_all(&directory).ok()?;
    let input = directory.join("seed-input.csv");
    let archive = directory.join("seed.dpack");
    let _ = std::fs::remove_file(&archive);
    std::fs::write(&input, ORIGINAL).ok()?;

    let mut options = datapack::storage::chunked::ChunkedCompressOptions::default();
    options.chunk_size_bytes = 8;
    options.threads = 1;
    options.max_in_flight_chunks = 1;
    options.profile = false;
    datapack::storage::chunked::encode_raw_zstd_chunked_file(&input, &archive, options).ok()?;
    let bytes = std::fs::read(&archive).ok();
    let _ = std::fs::remove_file(input);
    let _ = std::fs::remove_file(archive);
    bytes
}

fn mutate(seed: &[u8], controls: &[u8]) -> Vec<u8> {
    if controls.is_empty() || seed.is_empty() {
        return seed.to_vec();
    }

    let bounded = &controls[..controls.len().min(MAX_MUTATION_BYTES)];
    let mut mutated = seed.to_vec();
    match bounded[0] % 4 {
        0 => {
            for (ordinal, byte) in bounded[1..].iter().enumerate() {
                let index = (ordinal.saturating_mul(257) + usize::from(*byte)) % mutated.len();
                mutated[index] ^= byte.wrapping_add(1);
            }
            if bounded.len() == 1 {
                let index = usize::from(bounded[0]) % mutated.len();
                mutated[index] ^= 1;
            }
        }
        1 => {
            let low = usize::from(*bounded.get(1).unwrap_or(&0));
            let high = usize::from(*bounded.get(2).unwrap_or(&0));
            let requested = low | (high << 8);
            mutated.truncate(requested % (mutated.len() + 1));
        }
        2 => {
            if bounded.len() == 1 {
                mutated.push(bounded[0]);
            } else {
                mutated.extend_from_slice(&bounded[1..]);
            }
        }
        _ => {
            for (ordinal, byte) in bounded[1..].iter().enumerate() {
                let index = (ordinal + usize::from(bounded[0])) % mutated.len();
                mutated[index] = *byte;
            }
        }
    }
    mutated
}

fuzz_target!(|data: &[u8]| {
    let seed = SEED_ARCHIVE
        .get_or_init(build_seed_archive)
        .as_ref()
        .expect("failed to initialize the v2 mutated-archive fuzz seed");

    let directory = work_dir();
    let archive_path = directory.join("mutated.dpack");
    let output_path = directory.join("restored.bin");
    if std::fs::remove_file(&output_path).is_err() && output_path.exists() {
        return;
    }
    let _ = std::fs::remove_file(&archive_path);
    if std::fs::write(&archive_path, mutate(seed, data)).is_err() {
        return;
    }

    let result = datapack::storage::chunked::decode_raw_zstd_chunked_file(
        &archive_path,
        &output_path,
        datapack::storage::chunked::ChunkedDecompressOptions {
            verify: true,
            max_output_bytes: Some(MAX_FUZZ_OUTPUT_BYTES),
            max_chunks: Some(MAX_FUZZ_CHUNKS),
            max_memory_bytes: Some(MAX_FUZZ_MEMORY_BYTES),
            force: false,
            keep_temp: false,
        },
    );

    match result {
        Ok(_) => {
            let restored = std::fs::read(&output_path).unwrap_or_default();
            assert_eq!(restored, ORIGINAL);
        }
        Err(_) => assert!(!output_path.exists()),
    }

    let _ = std::fs::remove_file(output_path);
    let _ = std::fs::remove_file(archive_path);
});
