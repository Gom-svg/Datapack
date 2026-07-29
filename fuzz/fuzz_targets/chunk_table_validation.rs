#![no_main]

use std::io::Cursor;

use libfuzzer_sys::fuzz_target;

const HEADER_LEN: usize = 64;
const ENTRY_LEN: usize = 80;
const MAX_ENTRIES: usize = 16;
const MAX_PAYLOAD_BYTES: usize = 4 * 1024;

fn field(data: &[u8], start: usize) -> [u8; 8] {
    let mut bytes = [0u8; 8];
    for (destination, source) in bytes.iter_mut().zip(data.get(start..).unwrap_or_default()) {
        *destination = *source;
    }
    bytes
}

fuzz_target!(|data: &[u8]| {
    let table_source = data.get(16..).unwrap_or_default();
    let entry_count = table_source
        .len()
        .saturating_add(ENTRY_LEN - 1)
        .checked_div(ENTRY_LEN)
        .unwrap_or(0)
        .min(MAX_ENTRIES);

    let mut archive = vec![0u8; HEADER_LEN];
    archive[0..5].copy_from_slice(b"DPACK");
    archive[5..7].copy_from_slice(&2u16.to_le_bytes());
    archive[7] = 1;
    archive[8..16].copy_from_slice(&field(data, 0));
    archive[16] = 1;
    archive[48..56].copy_from_slice(&(entry_count as u64).to_le_bytes());
    archive[56..64].copy_from_slice(&field(data, 8));

    for index in 0..entry_count {
        let source_start = index.saturating_mul(ENTRY_LEN);
        let mut entry = [0u8; ENTRY_LEN];
        if let Some(source) = table_source.get(source_start..) {
            for (destination, source) in entry.iter_mut().zip(source) {
                *destination = *source;
            }
        }

        // Normalize the fields that would otherwise reject almost every input
        // immediately, leaving offsets and sizes fully fuzzer-controlled.
        entry[0..8].copy_from_slice(&(index as u64).to_le_bytes());
        entry[40] = 1;
        entry[77..80].fill(0);
        archive.extend_from_slice(&entry);
    }

    archive.extend_from_slice(&data[..data.len().min(MAX_PAYLOAD_BYTES)]);
    let mut cursor = Cursor::new(archive);
    let _ = datapack::storage::chunked::read_v2_archive_info(&mut cursor);
});
