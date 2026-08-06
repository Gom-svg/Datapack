use std::io::{Read, Write};

use bincode::Options;

use crate::compression::zstd_backend;
use crate::error::{DatapackError, Result};
use crate::formats::csv::columnar;
use crate::metadata::{DpackMetadata, FileType, PayloadKind, CURRENT_VERSION};
use crate::planning::ColumnExecutionPlan;

pub mod chunked;
pub mod output;

pub const MAGIC: &[u8; 5] = b"DPACK";
pub(crate) const V1_FIXED_HEADER_LEN: usize = 5 + 2 + 1 + 8;
/// V1 metadata is descriptive and should remain small even for large inputs.
/// This generous ceiling prevents an archive-controlled allocation while
/// retaining compatibility with metadata-rich legacy archives.
pub const MAX_V1_METADATA_BYTES: u64 = 64 * 1024 * 1024;
const V1_COLUMNAR_PAYLOAD_EXPANSION_FACTOR: u64 = 16;
const V1_COLUMNAR_PAYLOAD_FIXED_ALLOWANCE: u64 = 1024 * 1024;

#[derive(Debug, Clone)]
pub struct DpackArchive {
    pub metadata: DpackMetadata,
    pub payload: Vec<u8>,
}

pub fn encode_archive(metadata: &DpackMetadata, original_bytes: &[u8]) -> Result<Vec<u8>> {
    let payload = zstd_backend::compress(original_bytes)?;
    encode_archive_with_payload(metadata, &payload)
}

pub fn encode_adaptive_archive(
    input_path: &std::path::Path,
    original_bytes: &[u8],
) -> Result<Vec<u8>> {
    let raw_zstd = zstd_backend::compress(original_bytes)?;
    let mut metadata =
        DpackMetadata::minimal(FileType::from_path(input_path), original_bytes.len() as u64);
    metadata.payload_kind = PayloadKind::RawZstd;
    let mut best_payload = raw_zstd;

    if metadata.original_file_type == FileType::Csv {
        if let Ok(Some(encoded_csv)) = columnar::encode(original_bytes) {
            let compressed_csv = zstd_backend::compress(&encoded_csv)?;
            if compressed_csv.len() < best_payload.len() {
                metadata.payload_kind = PayloadKind::CsvColumnarDictionary;
                best_payload = compressed_csv;
            }
        }
    }

    encode_archive_with_payload(&metadata, &best_payload)
}

pub fn encode_raw_zstd_archive(
    input_path: &std::path::Path,
    original_bytes: &[u8],
) -> Result<Vec<u8>> {
    let mut metadata =
        DpackMetadata::minimal(FileType::from_path(input_path), original_bytes.len() as u64);
    metadata.payload_kind = PayloadKind::RawZstd;
    let payload = zstd_backend::compress(original_bytes)?;
    encode_archive_with_payload(&metadata, &payload)
}

pub fn encode_columnar_dictionary_archive(
    input_path: &std::path::Path,
    original_bytes: &[u8],
) -> Result<Option<Vec<u8>>> {
    Ok(encode_columnar_dictionary_archive_detailed(input_path, original_bytes)?.0)
}

pub fn encode_columnar_dictionary_archive_detailed(
    input_path: &std::path::Path,
    original_bytes: &[u8],
) -> Result<(Option<Vec<u8>>, Option<String>)> {
    encode_columnar_dictionary_archive_from_payload(
        input_path,
        original_bytes,
        columnar::encode(original_bytes),
    )
}

pub(crate) fn encode_columnar_dictionary_archive_with_plan_detailed(
    input_path: &std::path::Path,
    original_bytes: &[u8],
    delimiter: u8,
    execution_plan: &ColumnExecutionPlan,
) -> Result<(Option<Vec<u8>>, Option<String>)> {
    encode_columnar_dictionary_archive_from_payload(
        input_path,
        original_bytes,
        columnar::encode_with_execution_plan(original_bytes, delimiter, execution_plan),
    )
}

fn encode_columnar_dictionary_archive_from_payload(
    input_path: &std::path::Path,
    original_bytes: &[u8],
    encoded_csv: Result<Option<Vec<u8>>>,
) -> Result<(Option<Vec<u8>>, Option<String>)> {
    let encoded_csv = match encoded_csv {
        Ok(Some(encoded)) => encoded,
        Ok(None) => {
            return Ok((
                None,
                Some("columnar reconstruction validation failed".to_string()),
            ))
        }
        Err(error) => return Ok((None, Some(error.to_string()))),
    };
    let mut metadata =
        DpackMetadata::minimal(FileType::from_path(input_path), original_bytes.len() as u64);
    metadata.payload_kind = PayloadKind::CsvColumnarDictionary;
    let payload = zstd_backend::compress(&encoded_csv)?;
    Ok((
        Some(encode_archive_with_payload(&metadata, &payload)?),
        None,
    ))
}

fn encode_archive_with_payload(
    metadata: &DpackMetadata,
    compressed_payload: &[u8],
) -> Result<Vec<u8>> {
    let header = encode_v1_header(metadata)?;
    let archive_capacity = header
        .len()
        .checked_add(compressed_payload.len())
        .ok_or_else(|| {
            DatapackError::InvalidFormat("v1 archive size exceeds platform capacity".to_string())
        })?;
    let mut archive = Vec::new();
    archive
        .try_reserve_exact(archive_capacity)
        .map_err(|error| {
            DatapackError::InvalidFormat(format!(
                "cannot reserve memory for v1 archive ({archive_capacity} bytes): {error}"
            ))
        })?;
    archive.extend_from_slice(&header);
    archive.extend_from_slice(compressed_payload);

    Ok(archive)
}

fn encode_v1_header(metadata: &DpackMetadata) -> Result<Vec<u8>> {
    validate_v1_metadata_fields(metadata, metadata.original_file_type)?;
    let metadata_bytes = bincode::serialize(metadata)?;
    let metadata_len = u64::try_from(metadata_bytes.len()).map_err(|_| {
        DatapackError::InvalidFormat("v1 metadata size exceeds u64 capacity".to_string())
    })?;
    validate_v1_metadata_len(metadata_len)?;
    let header_capacity = V1_FIXED_HEADER_LEN
        .checked_add(metadata_bytes.len())
        .ok_or_else(|| {
            DatapackError::InvalidFormat("v1 header size exceeds platform capacity".to_string())
        })?;
    let mut header = Vec::new();
    header.try_reserve_exact(header_capacity).map_err(|error| {
        DatapackError::InvalidFormat(format!(
            "cannot reserve memory for v1 header ({header_capacity} bytes): {error}"
        ))
    })?;
    header.extend_from_slice(MAGIC);
    header.extend_from_slice(&CURRENT_VERSION.to_le_bytes());
    header.push(metadata.original_file_type.to_byte());
    header.extend_from_slice(&metadata_len.to_le_bytes());
    header.extend_from_slice(&metadata_bytes);
    Ok(header)
}

pub fn write_raw_zstd_archive_stream<R: Read, W: Write>(
    input_path: &std::path::Path,
    original_size: u64,
    reader: &mut R,
    writer: &mut W,
) -> Result<u64> {
    let mut metadata = DpackMetadata::minimal(FileType::from_path(input_path), original_size);
    metadata.payload_kind = PayloadKind::RawZstd;
    writer.write_all(&encode_v1_header(&metadata)?)?;
    let input_bytes = zstd_backend::compress_stream(reader, writer, zstd_backend::DEFAULT_LEVEL)?;
    if input_bytes != original_size {
        return Err(DatapackError::InvalidFormat(format!(
            "streamed input size {input_bytes} does not match expected size {original_size}"
        )));
    }
    Ok(input_bytes)
}

pub fn decode_archive(bytes: &[u8]) -> Result<DpackArchive> {
    if bytes.len() < V1_FIXED_HEADER_LEN {
        return Err(DatapackError::InvalidFormat(format!(
            "truncated v1 header: expected at least {V1_FIXED_HEADER_LEN} bytes, got {}",
            bytes.len()
        )));
    }

    validate_magic(bytes)?;
    let version_offset = MAGIC.len();
    let version = read_u16_at(bytes, version_offset, "archive version")?;
    if version == chunked::CHUNKED_VERSION {
        return Err(DatapackError::InvalidFormat(
            "v2 chunked archive requires streaming v2 decompression".to_string(),
        ));
    }
    if version != CURRENT_VERSION {
        return Err(DatapackError::InvalidFormat(format!(
            "unsupported version {version}"
        )));
    }

    let file_type_offset = version_offset
        .checked_add(2)
        .ok_or_else(|| DatapackError::InvalidFormat("v1 file type offset overflow".to_string()))?;
    let file_type_byte = *bytes
        .get(file_type_offset)
        .ok_or_else(|| DatapackError::InvalidFormat("truncated v1 file type field".to_string()))?;
    let outer_file_type = FileType::from_byte(file_type_byte).ok_or_else(|| {
        DatapackError::InvalidFormat(format!("unknown v1 file type byte {file_type_byte}"))
    })?;

    let metadata_len_offset = file_type_offset.checked_add(1).ok_or_else(|| {
        DatapackError::InvalidFormat("v1 metadata length offset overflow".to_string())
    })?;
    let metadata_len_u64 = read_u64_at(bytes, metadata_len_offset, "metadata length")?;
    let metadata_len = validate_v1_metadata_len(metadata_len_u64)?;
    let metadata_start = V1_FIXED_HEADER_LEN;
    let metadata_end = metadata_start.checked_add(metadata_len).ok_or_else(|| {
        DatapackError::InvalidFormat("v1 metadata end offset overflow".to_string())
    })?;
    let metadata_bytes = bytes.get(metadata_start..metadata_end).ok_or_else(|| {
        DatapackError::InvalidFormat(format!(
            "v1 metadata extends beyond archive length: end {metadata_end}, archive length {}",
            bytes.len()
        ))
    })?;

    let metadata = deserialize_v1_metadata(metadata_bytes, metadata_len_u64, outer_file_type)?;
    let payload_bytes = bytes.get(metadata_end..).ok_or_else(|| {
        DatapackError::InvalidFormat("v1 payload offset exceeds archive length".to_string())
    })?;
    let mut payload = Vec::new();
    payload
        .try_reserve_exact(payload_bytes.len())
        .map_err(|error| {
            DatapackError::InvalidFormat(format!(
                "cannot reserve memory for v1 payload ({} bytes): {error}",
                payload_bytes.len()
            ))
        })?;
    payload.extend_from_slice(payload_bytes);

    Ok(DpackArchive { metadata, payload })
}

pub fn archive_version_from_bytes(bytes: &[u8]) -> Result<u16> {
    let header_len = MAGIC.len().checked_add(2).ok_or_else(|| {
        DatapackError::InvalidFormat("archive version header length overflow".to_string())
    })?;
    if bytes.len() < header_len {
        return Err(DatapackError::InvalidFormat(
            "file is too small".to_string(),
        ));
    }
    validate_magic(bytes)?;
    read_u16_at(bytes, MAGIC.len(), "archive version")
}

pub fn archive_version_from_path(path: &std::path::Path) -> Result<u16> {
    let mut file = std::fs::File::open(path)?;
    let mut header = [0u8; 7];
    file.read_exact(&mut header)?;
    archive_version_from_bytes(&header)
}

pub fn read_v1_archive_header<R: Read>(reader: &mut R) -> Result<DpackMetadata> {
    read_v1_archive_header_with_memory_limit(reader, None)
}

pub(crate) fn read_v1_archive_header_with_memory_limit<R: Read>(
    reader: &mut R,
    max_memory_bytes: Option<u64>,
) -> Result<DpackMetadata> {
    let mut header = [0u8; V1_FIXED_HEADER_LEN];
    reader.read_exact(&mut header)?;
    validate_magic(&header)?;

    let version_offset = MAGIC.len();
    let version = read_u16_at(&header, version_offset, "archive version")?;
    if version != CURRENT_VERSION {
        return Err(DatapackError::InvalidFormat(format!(
            "expected v1 archive, found version {version}"
        )));
    }

    let file_type_offset = version_offset
        .checked_add(2)
        .ok_or_else(|| DatapackError::InvalidFormat("v1 file type offset overflow".to_string()))?;
    let file_type_byte = *header
        .get(file_type_offset)
        .ok_or_else(|| DatapackError::InvalidFormat("truncated v1 file type field".to_string()))?;
    let outer_file_type = FileType::from_byte(file_type_byte).ok_or_else(|| {
        DatapackError::InvalidFormat(format!("unknown v1 file type byte {file_type_byte}"))
    })?;
    let metadata_len_offset = file_type_offset.checked_add(1).ok_or_else(|| {
        DatapackError::InvalidFormat("v1 metadata length offset overflow".to_string())
    })?;
    let metadata_len_u64 = read_u64_at(&header, metadata_len_offset, "metadata length")?;
    let metadata_len = validate_v1_metadata_len(metadata_len_u64)?;
    if let Some(maximum) = max_memory_bytes {
        let required = (V1_FIXED_HEADER_LEN as u64)
            .checked_add(metadata_len_u64)
            .ok_or_else(|| {
                DatapackError::InvalidFormat(
                    "v1 header and metadata memory estimate overflowed".to_string(),
                )
            })?;
        if required > maximum {
            return Err(DatapackError::InvalidFormat(format!(
                "v1 header and metadata require {required} bytes, exceeding --max-memory-mb limit of {maximum} bytes"
            )));
        }
    }
    let mut metadata_bytes = Vec::new();
    metadata_bytes
        .try_reserve_exact(metadata_len)
        .map_err(|error| {
            DatapackError::InvalidFormat(format!(
                "cannot reserve memory for v1 metadata ({metadata_len} bytes): {error}"
            ))
        })?;
    metadata_bytes.resize(metadata_len, 0);
    reader.read_exact(&mut metadata_bytes)?;
    deserialize_v1_metadata(&metadata_bytes, metadata_len_u64, outer_file_type)
}

fn validate_magic(bytes: &[u8]) -> Result<()> {
    let actual = bytes.get(..MAGIC.len()).ok_or_else(|| {
        DatapackError::InvalidFormat(format!(
            "truncated archive magic: expected {} bytes, got {}",
            MAGIC.len(),
            bytes.len()
        ))
    })?;
    if actual != MAGIC {
        return Err(DatapackError::InvalidFormat(
            "bad archive magic bytes; expected DPACK".to_string(),
        ));
    }
    Ok(())
}

fn read_u16_at(bytes: &[u8], offset: usize, field: &str) -> Result<u16> {
    let end = offset
        .checked_add(2)
        .ok_or_else(|| DatapackError::InvalidFormat(format!("{field} offset overflow")))?;
    let source = bytes
        .get(offset..end)
        .ok_or_else(|| DatapackError::InvalidFormat(format!("truncated {field} field")))?;
    let mut value = [0u8; 2];
    value.copy_from_slice(source);
    Ok(u16::from_le_bytes(value))
}

fn read_u64_at(bytes: &[u8], offset: usize, field: &str) -> Result<u64> {
    let end = offset
        .checked_add(8)
        .ok_or_else(|| DatapackError::InvalidFormat(format!("{field} offset overflow")))?;
    let source = bytes
        .get(offset..end)
        .ok_or_else(|| DatapackError::InvalidFormat(format!("truncated {field} field")))?;
    let mut value = [0u8; 8];
    value.copy_from_slice(source);
    Ok(u64::from_le_bytes(value))
}

fn validate_v1_metadata_len(metadata_len: u64) -> Result<usize> {
    if metadata_len > MAX_V1_METADATA_BYTES {
        return Err(DatapackError::InvalidFormat(format!(
            "v1 metadata length {metadata_len} exceeds the safety ceiling of {MAX_V1_METADATA_BYTES} bytes"
        )));
    }
    usize::try_from(metadata_len).map_err(|_| {
        DatapackError::InvalidFormat(format!(
            "v1 metadata length {metadata_len} exceeds platform capacity"
        ))
    })
}

fn deserialize_v1_metadata(
    metadata_bytes: &[u8],
    metadata_len: u64,
    outer_file_type: FileType,
) -> Result<DpackMetadata> {
    let metadata: DpackMetadata = bincode::DefaultOptions::new()
        .with_fixint_encoding()
        .with_limit(metadata_len)
        .reject_trailing_bytes()
        .deserialize(metadata_bytes)?;

    validate_v1_metadata_fields(&metadata, outer_file_type)?;
    Ok(metadata)
}

fn validate_v1_metadata_fields(metadata: &DpackMetadata, outer_file_type: FileType) -> Result<()> {
    if metadata.version != CURRENT_VERSION {
        return Err(DatapackError::InvalidFormat(format!(
            "v1 metadata version {} does not match archive version {CURRENT_VERSION}",
            metadata.version
        )));
    }
    if metadata.original_file_type != outer_file_type {
        return Err(DatapackError::InvalidFormat(format!(
            "v1 metadata file type {:?} does not match header file type {:?}",
            metadata.original_file_type, outer_file_type
        )));
    }
    if let Some((index, _)) = metadata
        .future_extensions
        .iter()
        .enumerate()
        .find(|(_, extension)| extension.enabled)
    {
        return Err(DatapackError::InvalidFormat(format!(
            "v1 metadata enables unsupported extension at index {index}; all v1 extensions must be disabled"
        )));
    }

    Ok(())
}

pub fn restore_raw_zstd_stream<R: Read, W: Write>(
    metadata: &DpackMetadata,
    reader: &mut R,
    writer: &mut W,
) -> Result<u64> {
    if !matches!(
        metadata.payload_kind,
        PayloadKind::RawZstd | PayloadKind::Plain | PayloadKind::Dictionary
    ) {
        return Err(DatapackError::InvalidFormat(
            "streaming v1 restore only supports raw-byte payloads".to_string(),
        ));
    }
    zstd_backend::decompress_stream_exact(reader, writer, metadata.original_size)
}

pub fn write_archive<W: Write>(
    writer: &mut W,
    metadata: &DpackMetadata,
    original_bytes: &[u8],
) -> Result<()> {
    writer.write_all(&encode_archive(metadata, original_bytes)?)?;
    Ok(())
}

pub fn write_adaptive_archive<W: Write>(
    writer: &mut W,
    input_path: &std::path::Path,
    original_bytes: &[u8],
) -> Result<()> {
    writer.write_all(&encode_adaptive_archive(input_path, original_bytes)?)?;
    Ok(())
}

pub fn restore_archive(archive: &DpackArchive) -> Result<Vec<u8>> {
    let restored = match archive.metadata.payload_kind {
        PayloadKind::RawZstd | PayloadKind::Plain | PayloadKind::Dictionary => {
            let output_limit =
                v1_in_memory_limit(archive.metadata.original_size, "declared v1 original size")?;
            zstd_backend::decompress_with_limit(&archive.payload, output_limit)?
        }
        PayloadKind::CsvColumnarDictionary => {
            let encoded_limit = v1_columnar_payload_limit(archive.metadata.original_size)?;
            let encoded_memory_limit =
                v1_in_memory_limit(encoded_limit, "bounded v1 columnar payload size")?;
            let decompressed_payload =
                zstd_backend::decompress_with_limit(&archive.payload, encoded_memory_limit)?;
            columnar::decode_with_output_limit(
                &decompressed_payload,
                archive.metadata.original_size,
            )?
        }
    };

    let restored_len = u64::try_from(restored.len()).map_err(|_| {
        DatapackError::InvalidFormat("restored output size exceeds u64 capacity".to_string())
    })?;
    if restored_len != archive.metadata.original_size {
        return Err(DatapackError::InvalidFormat(format!(
            "restored size {} does not match original size {}",
            restored_len, archive.metadata.original_size
        )));
    }

    Ok(restored)
}

fn v1_columnar_payload_limit(original_size: u64) -> Result<u64> {
    // The established DCSV01 encoder stores at most one copy of each field plus
    // fixed per-cell lengths/codes. Sixteen times the reconstructed size plus a
    // fixed allowance comfortably covers valid legacy payloads while bounding
    // zstd expansion before the columnar parser sees it.
    original_size
        .checked_mul(V1_COLUMNAR_PAYLOAD_EXPANSION_FACTOR)
        .and_then(|size| size.checked_add(V1_COLUMNAR_PAYLOAD_FIXED_ALLOWANCE))
        .ok_or_else(|| {
            DatapackError::InvalidFormat(format!(
                "declared original size {original_size} is too large to bound the v1 columnar payload safely"
            ))
        })
}

fn v1_in_memory_limit(limit: u64, purpose: &str) -> Result<usize> {
    usize::try_from(limit).map_err(|_| {
        DatapackError::InvalidFormat(format!(
            "{purpose} ({limit} bytes) exceeds platform capacity"
        ))
    })
}

pub fn read_archive<R: Read>(reader: &mut R) -> Result<DpackArchive> {
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes)?;
    decode_archive(&bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metadata::{ExtensionPoint, FileType};

    fn v1_bytes_with_metadata(
        metadata: &DpackMetadata,
        outer_file_type: FileType,
        metadata_trailer: &[u8],
    ) -> Vec<u8> {
        let mut metadata_bytes = bincode::serialize(metadata).unwrap();
        metadata_bytes.extend_from_slice(metadata_trailer);
        let metadata_len = u64::try_from(metadata_bytes.len()).unwrap();
        let mut archive = Vec::new();
        archive.extend_from_slice(MAGIC);
        archive.extend_from_slice(&CURRENT_VERSION.to_le_bytes());
        archive.push(outer_file_type.to_byte());
        archive.extend_from_slice(&metadata_len.to_le_bytes());
        archive.extend_from_slice(&metadata_bytes);
        archive
    }

    #[test]
    fn dpack_archive_round_trip() {
        let input = b"id,name\n1,Ada\n2,Grace\n";
        let metadata = DpackMetadata::new(FileType::Csv, input.len() as u64);
        let encoded = encode_archive(&metadata, input).unwrap();
        let archive = decode_archive(&encoded).unwrap();
        let restored = restore_archive(&archive).unwrap();

        assert_eq!(archive.metadata.version, CURRENT_VERSION);
        assert_eq!(archive.metadata.original_file_type, FileType::Csv);
        assert_eq!(restored, input);
    }

    #[test]
    fn adaptive_archive_falls_back_to_raw_zstd_for_unhelpful_csv() {
        let input = b"id,value\n1,alpha\n2,beta\n3,gamma\n";
        let encoded = encode_adaptive_archive(std::path::Path::new("sample.csv"), input).unwrap();
        let archive = decode_archive(&encoded).unwrap();

        assert_eq!(archive.metadata.payload_kind, PayloadKind::RawZstd);
        assert_eq!(restore_archive(&archive).unwrap(), input);
    }

    #[test]
    fn streaming_v1_raw_zstd_archive_round_trip() {
        let input = b"id,value\r\n1,alpha\r\n2,beta\r\n";
        let mut archive_bytes = Vec::new();
        write_raw_zstd_archive_stream(
            std::path::Path::new("sample.csv"),
            input.len() as u64,
            &mut std::io::Cursor::new(input),
            &mut archive_bytes,
        )
        .unwrap();

        let mut reader = std::io::Cursor::new(&archive_bytes);
        let metadata = read_v1_archive_header(&mut reader).unwrap();
        let mut restored = Vec::new();
        restore_raw_zstd_stream(&metadata, &mut reader, &mut restored).unwrap();

        assert_eq!(
            archive_version_from_bytes(&archive_bytes).unwrap(),
            CURRENT_VERSION
        );
        assert_eq!(metadata.payload_kind, PayloadKind::RawZstd);
        assert_eq!(restored, input);
    }

    #[test]
    fn v1_empty_raw_archive_round_trip_is_explicit() {
        let encoded = encode_raw_zstd_archive(std::path::Path::new("empty.txt"), b"").unwrap();
        let archive = decode_archive(&encoded).unwrap();
        let restored = restore_archive(&archive).unwrap();

        assert_eq!(archive.metadata.original_size, 0);
        assert!(restored.is_empty());
    }

    #[test]
    fn v1_metadata_length_over_ceiling_is_rejected_before_allocation() {
        let mut archive = Vec::new();
        archive.extend_from_slice(MAGIC);
        archive.extend_from_slice(&CURRENT_VERSION.to_le_bytes());
        archive.push(FileType::Csv.to_byte());
        archive.extend_from_slice(&(MAX_V1_METADATA_BYTES + 1).to_le_bytes());

        let error = decode_archive(&archive).unwrap_err();

        assert!(error.to_string().contains("metadata length"));
        assert!(error.to_string().contains("safety ceiling"));
    }

    #[test]
    fn v1_metadata_with_trailing_bytes_is_rejected() {
        let metadata = DpackMetadata::minimal(FileType::Csv, 1);
        let archive = v1_bytes_with_metadata(&metadata, FileType::Csv, &[0xff]);

        let error = decode_archive(&archive).unwrap_err();

        assert!(error.to_string().contains("metadata serialization error"));
    }

    #[test]
    fn v1_metadata_version_must_match_header_version() {
        let mut metadata = DpackMetadata::minimal(FileType::Csv, 1);
        metadata.version = CURRENT_VERSION + 1;
        let archive = v1_bytes_with_metadata(&metadata, FileType::Csv, &[]);

        let error = decode_archive(&archive).unwrap_err();

        assert!(error.to_string().contains("metadata version"));
    }

    #[test]
    fn v1_metadata_file_type_must_match_outer_header() {
        let metadata = DpackMetadata::minimal(FileType::Csv, 1);
        let archive = v1_bytes_with_metadata(&metadata, FileType::Txt, &[]);

        let error = decode_archive(&archive).unwrap_err();

        assert!(error
            .to_string()
            .contains("does not match header file type"));
    }

    #[test]
    fn v1_enabled_extension_metadata_is_rejected() {
        let mut metadata = DpackMetadata::minimal(FileType::Csv, 1);
        metadata
            .future_extensions
            .push(ExtensionPoint::new("unsupported", true));
        let archive = v1_bytes_with_metadata(&metadata, FileType::Csv, &[]);

        let error = decode_archive(&archive).unwrap_err();

        assert!(error.to_string().contains("unsupported extension"));
        assert!(error.to_string().contains("must be disabled"));
    }

    #[test]
    fn v1_in_memory_restore_rejects_output_beyond_declared_size() {
        let input = b"restored bytes exceed the declaration";
        let declared_size = u64::try_from(input.len() - 1).unwrap();
        let metadata = DpackMetadata::minimal(FileType::Txt, declared_size);
        let archive = DpackArchive {
            metadata,
            payload: zstd_backend::compress(input).unwrap(),
        };

        let error = restore_archive(&archive).unwrap_err();

        assert!(error.to_string().contains("configured limit"));
    }

    #[test]
    fn v1_streaming_restore_never_writes_beyond_declared_size() {
        let input = b"streamed restored bytes exceed the declaration";
        let declared_size = u64::try_from(input.len() - 1).unwrap();
        let metadata = DpackMetadata::minimal(FileType::Txt, declared_size);
        let compressed = zstd_backend::compress(input).unwrap();
        let mut restored = Vec::new();

        let error = restore_raw_zstd_stream(
            &metadata,
            &mut std::io::Cursor::new(compressed),
            &mut restored,
        )
        .unwrap_err();

        assert!(error.to_string().contains("exceeds expected size"));
        assert!(u64::try_from(restored.len()).unwrap() <= declared_size);
    }
}
