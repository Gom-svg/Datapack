use std::io::{Cursor, Read, Write};

use crate::application::control::{CancellationToken, OperationResult};
use crate::error::{DatapackError, Result};

pub const DEFAULT_LEVEL: i32 = 3;
const DECOMPRESSION_BUFFER_BYTES: usize = 64 * 1024;

/// A reusable zstd compressor that delegates parallel compression to zstd.
///
/// Each call emits an ordinary zstd frame, so callers can use the standard
/// [`decompress`] path without recording the compressor backend in an archive.
pub struct NativeMtCompressor {
    compressor: zstd::bulk::Compressor<'static>,
}

impl NativeMtCompressor {
    /// Creates a compressor with the requested number of native zstd workers.
    pub fn new(threads: usize) -> Result<Self> {
        if threads == 0 {
            return Err(DatapackError::InvalidFormat(
                "native zstd thread count must be greater than zero".to_string(),
            ));
        }
        let workers = u32::try_from(threads).map_err(|_| {
            DatapackError::InvalidFormat(
                "native zstd thread count exceeds the supported maximum".to_string(),
            )
        })?;

        let mut compressor = zstd::bulk::Compressor::new(DEFAULT_LEVEL)?;
        compressor.multithread(workers)?;
        Ok(Self { compressor })
    }

    /// Compresses one independent frame at the default compression level.
    pub fn compress(&mut self, bytes: &[u8]) -> Result<Vec<u8>> {
        self.compress_with_level(bytes, DEFAULT_LEVEL)
    }

    /// Compresses one independent frame at `level` while reusing the native
    /// compression context and worker pool between calls.
    pub fn compress_with_level(&mut self, bytes: &[u8], level: i32) -> Result<Vec<u8>> {
        self.compressor.set_compression_level(level)?;
        Ok(self.compressor.compress(bytes)?)
    }
}

pub fn compress(bytes: &[u8]) -> Result<Vec<u8>> {
    compress_with_level(bytes, DEFAULT_LEVEL)
}

pub fn compress_with_level(bytes: &[u8], level: i32) -> Result<Vec<u8>> {
    Ok(zstd::stream::encode_all(Cursor::new(bytes), level)?)
}

pub fn decompress(bytes: &[u8]) -> Result<Vec<u8>> {
    Ok(zstd::stream::decode_all(Cursor::new(bytes))?)
}

/// Decompresses into memory without allowing the restored bytes to exceed
/// `max_output_size`.
///
/// The limit is checked before decoded bytes are appended to the result. This
/// keeps a valid-looking zstd frame from using an untrusted size declaration to
/// grow an output buffer without a bound.
pub fn decompress_with_limit(bytes: &[u8], max_output_size: usize) -> Result<Vec<u8>> {
    let mut decoder = zstd::stream::read::Decoder::new(Cursor::new(bytes))?;
    let mut output = Vec::new();
    let initial_capacity = max_output_size.min(DECOMPRESSION_BUFFER_BYTES);
    output
        .try_reserve_exact(initial_capacity)
        .map_err(|error| {
            DatapackError::InvalidFormat(format!(
                "cannot reserve memory for bounded decompression: {error}"
            ))
        })?;

    let mut buffer = [0u8; DECOMPRESSION_BUFFER_BYTES];
    let mut restored_size = 0usize;
    loop {
        let read = decoder.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        let next_size = restored_size.checked_add(read).ok_or_else(|| {
            DatapackError::InvalidFormat("decompressed size exceeds platform capacity".to_string())
        })?;
        if next_size > max_output_size {
            return Err(DatapackError::InvalidFormat(format!(
                "decompressed output exceeds configured limit of {max_output_size} bytes"
            )));
        }
        output.try_reserve(read).map_err(|error| {
            DatapackError::InvalidFormat(format!(
                "cannot reserve memory for bounded decompression: {error}"
            ))
        })?;
        output.extend_from_slice(&buffer[..read]);
        restored_size = next_size;
    }

    Ok(output)
}

pub fn compress_stream<R: Read, W: Write>(
    reader: &mut R,
    writer: &mut W,
    level: i32,
) -> Result<u64> {
    let mut encoder = zstd::stream::write::Encoder::new(writer, level)?;
    let input_bytes = std::io::copy(reader, &mut encoder)?;
    encoder.finish()?;
    Ok(input_bytes)
}

pub(crate) fn compress_stream_with_control<R: Read, W: Write>(
    reader: &mut R,
    writer: &mut W,
    level: i32,
    cancellation: Option<&CancellationToken>,
) -> OperationResult<u64> {
    checkpoint(cancellation)?;
    let mut encoder = zstd::stream::write::Encoder::new(writer, level)?;
    let mut buffer = [0u8; DECOMPRESSION_BUFFER_BYTES];
    let mut input_bytes = 0u64;
    loop {
        checkpoint(cancellation)?;
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        encoder.write_all(&buffer[..read])?;
        input_bytes = input_bytes
            .checked_add(u64::try_from(read).map_err(|_| {
                DatapackError::InvalidFormat(
                    "compressed read size exceeds u64 capacity".to_string(),
                )
            })?)
            .ok_or_else(|| {
                DatapackError::InvalidFormat(
                    "compressed input size exceeds u64 capacity".to_string(),
                )
            })?;
    }
    checkpoint(cancellation)?;
    encoder.finish()?;
    Ok(input_bytes)
}

pub fn decompress_stream<R: Read, W: Write>(reader: &mut R, writer: &mut W) -> Result<u64> {
    let mut decoder = zstd::stream::read::Decoder::new(reader)?;
    Ok(std::io::copy(&mut decoder, writer)?)
}

/// Streams decompressed bytes while enforcing the exact expected output size.
///
/// No bytes beyond `expected_size` are written. The decoder is still read to
/// its end, so a truncated frame or invalid trailing data remains an error even
/// if the frame emitted the expected number of bytes before the fault.
pub fn decompress_stream_exact<R: Read, W: Write>(
    reader: &mut R,
    writer: &mut W,
    expected_size: u64,
) -> Result<u64> {
    let mut decoder = zstd::stream::read::Decoder::new(reader)?;
    let mut buffer = [0u8; DECOMPRESSION_BUFFER_BYTES];
    let mut restored_size = 0u64;

    loop {
        let read = decoder.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        let read_u64 = u64::try_from(read).map_err(|_| {
            DatapackError::InvalidFormat("decompressed read size exceeds u64 capacity".to_string())
        })?;
        let next_size = restored_size.checked_add(read_u64).ok_or_else(|| {
            DatapackError::InvalidFormat("decompressed size exceeds u64 capacity".to_string())
        })?;
        if next_size > expected_size {
            return Err(DatapackError::InvalidFormat(format!(
                "decompressed output exceeds expected size of {expected_size} bytes"
            )));
        }
        writer.write_all(&buffer[..read])?;
        restored_size = next_size;
    }

    if restored_size != expected_size {
        return Err(DatapackError::InvalidFormat(format!(
            "decompressed size mismatch: expected {expected_size}, got {restored_size}"
        )));
    }

    Ok(restored_size)
}

pub(crate) fn decompress_stream_exact_with_control<R: Read, W: Write>(
    reader: &mut R,
    writer: &mut W,
    expected_size: u64,
    cancellation: Option<&CancellationToken>,
) -> OperationResult<u64> {
    checkpoint(cancellation)?;
    let mut decoder = zstd::stream::read::Decoder::new(reader)?;
    let mut buffer = [0u8; DECOMPRESSION_BUFFER_BYTES];
    let mut restored_size = 0u64;

    loop {
        checkpoint(cancellation)?;
        let read = decoder.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        let read_u64 = u64::try_from(read).map_err(|_| {
            DatapackError::InvalidFormat("decompressed read size exceeds u64 capacity".to_string())
        })?;
        let next_size = restored_size.checked_add(read_u64).ok_or_else(|| {
            DatapackError::InvalidFormat("decompressed size exceeds u64 capacity".to_string())
        })?;
        if next_size > expected_size {
            return Err(DatapackError::InvalidFormat(format!(
                "decompressed output exceeds expected size of {expected_size} bytes"
            ))
            .into());
        }
        writer.write_all(&buffer[..read])?;
        restored_size = next_size;
    }

    if restored_size != expected_size {
        return Err(DatapackError::InvalidFormat(format!(
            "decompressed size mismatch: expected {expected_size}, got {restored_size}"
        ))
        .into());
    }

    Ok(restored_size)
}

fn checkpoint(cancellation: Option<&CancellationToken>) -> OperationResult<()> {
    match cancellation {
        Some(cancellation) => cancellation.checkpoint(),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn native_mt_test_input() -> Vec<u8> {
        let pattern = b"id,category,value\r\n1,alpha,12345\r\n2,beta,67890\r\n";
        pattern.iter().copied().cycle().take(1024 * 1024).collect()
    }

    #[test]
    fn zstd_round_trip() {
        let input = b"alpha alpha alpha\nbeta beta beta\n";
        let compressed = compress(input).unwrap();
        let restored = decompress(&compressed).unwrap();

        assert_eq!(restored, input);
    }

    #[test]
    fn streaming_zstd_round_trip() {
        let input = b"alpha alpha alpha\nbeta beta beta\n";
        let mut compressed = Vec::new();
        let compressed_input =
            compress_stream(&mut Cursor::new(input), &mut compressed, DEFAULT_LEVEL).unwrap();
        let mut restored = Vec::new();
        let restored_size = decompress_stream(&mut Cursor::new(compressed), &mut restored).unwrap();

        assert_eq!(compressed_input, input.len() as u64);
        assert_eq!(restored_size, input.len() as u64);
        assert_eq!(restored, input);
    }

    #[test]
    fn bounded_decompression_rejects_output_larger_than_limit() {
        let input = vec![b'x'; 128 * 1024];
        let compressed = compress(&input).unwrap();

        let error = decompress_with_limit(&compressed, input.len() - 1).unwrap_err();

        assert!(error
            .to_string()
            .contains("decompressed output exceeds configured limit"));
    }

    #[test]
    fn bounded_decompression_accepts_output_at_limit() {
        let input = b"bounded zstd output";
        let compressed = compress(input).unwrap();

        let restored = decompress_with_limit(&compressed, input.len()).unwrap();

        assert_eq!(restored, input);
    }

    #[test]
    fn exact_streaming_decompression_never_writes_past_expected_size() {
        let input = vec![b'y'; 128 * 1024];
        let compressed = compress(&input).unwrap();
        let expected_size = 1024u64;
        let mut restored = Vec::new();

        let error =
            decompress_stream_exact(&mut Cursor::new(compressed), &mut restored, expected_size)
                .unwrap_err();

        assert!(error.to_string().contains("exceeds expected size"));
        assert!(u64::try_from(restored.len()).unwrap() <= expected_size);
    }

    #[test]
    fn exact_streaming_decompression_rejects_short_output() {
        let input = b"short output";
        let compressed = compress(input).unwrap();
        let mut restored = Vec::new();

        let error = decompress_stream_exact(
            &mut Cursor::new(compressed),
            &mut restored,
            u64::try_from(input.len()).unwrap() + 1,
        )
        .unwrap_err();

        assert!(error.to_string().contains("decompressed size mismatch"));
        assert_eq!(restored, input);
    }

    #[test]
    fn native_mt_one_worker_round_trip() {
        let input = native_mt_test_input();
        let mut compressor = NativeMtCompressor::new(1).unwrap();
        let compressed = compressor.compress(&input).unwrap();

        assert_eq!(decompress(&compressed).unwrap(), input);
    }

    #[test]
    fn native_mt_two_workers_round_trip_across_level_changes() {
        let input = native_mt_test_input();
        let mut compressor = NativeMtCompressor::new(2).unwrap();

        for level in [1, 6] {
            let compressed = compressor.compress_with_level(&input, level).unwrap();
            assert_eq!(decompress(&compressed).unwrap(), input);
        }
    }

    #[test]
    fn native_mt_rejects_invalid_worker_counts() {
        assert!(matches!(
            NativeMtCompressor::new(0),
            Err(DatapackError::InvalidFormat(message))
                if message.contains("greater than zero")
        ));

        #[cfg(target_pointer_width = "64")]
        assert!(matches!(
            NativeMtCompressor::new(u32::MAX as usize + 1),
            Err(DatapackError::InvalidFormat(message))
                if message.contains("supported maximum")
        ));
    }
}
