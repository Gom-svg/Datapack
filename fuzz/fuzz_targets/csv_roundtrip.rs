#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let path = std::path::Path::new("fuzz.csv");
    if let Ok(archive_bytes) = datapack::storage::encode_adaptive_archive(path, data) {
        let archive =
            datapack::storage::decode_archive(&archive_bytes).expect("encoded archive decodes");
        let restored = datapack::storage::restore_archive(&archive).expect("archive restores");
        assert_eq!(restored, data);
    }
});
