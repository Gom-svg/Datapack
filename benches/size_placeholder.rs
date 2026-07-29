use std::time::Instant;

fn main() {
    let sample = include_bytes!("../samples/repeated_values.csv");
    let start = Instant::now();
    let zstd_only = zstd::stream::encode_all(&sample[..], 3).expect("zstd compression");
    let compression_time = start.elapsed();

    let start = Instant::now();
    let restored = zstd::stream::decode_all(&zstd_only[..]).expect("zstd decompression");
    let decompression_time = start.elapsed();

    println!("benchmark placeholder");
    println!("raw file size: {}", sample.len());
    println!("zstd-only size: {}", zstd_only.len());
    println!("datapack size: run `cargo run -- compress samples/repeated_values.csv samples/repeated_values.dpack`");
    println!("compression time: {:?}", compression_time);
    println!("decompression time: {:?}", decompression_time);
    assert_eq!(restored, sample);
}
