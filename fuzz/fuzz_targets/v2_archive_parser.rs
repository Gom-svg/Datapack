#![no_main]

use std::io::Cursor;

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let mut archive = Cursor::new(data);
    let _ = datapack::storage::chunked::read_v2_archive_info(&mut archive);
});
