//! Internal archive-validation engine and public versioned report boundary.
//!
//! The path-based application service re-exports the stable report DTOs while
//! keeping validation options and storage implementation details crate-private.

use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom, Write};
use std::path::Path;

use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::application::control::{CancellationToken, OperationError, OperationResult};
use crate::error::{DatapackError, Result};
use crate::metadata::{PayloadKind, CURRENT_VERSION};
use crate::storage;

const HASH_BUFFER_BYTES: usize = 64 * 1024;
const V1_COLUMNAR_EXPANSION_FACTOR: u64 = 16;
const V1_COLUMNAR_FIXED_ALLOWANCE: u64 = 1024 * 1024;

#[derive(Debug, Clone, Copy)]
pub(crate) struct ValidationOptions {
    pub(crate) max_output_bytes: Option<u64>,
    pub(crate) max_chunks: Option<u64>,
    pub(crate) max_memory_bytes: u64,
}

#[derive(Debug, Serialize)]
#[non_exhaustive]
pub struct ValidationReportV1 {
    pub schema_version: u32,
    pub report_type: &'static str,
    pub valid: bool,
    pub archive: ArchiveReportV1,
    pub checks: ValidationChecksV1,
    pub against: AgainstReportV1,
    pub diagnostics: Vec<ValidationDiagnosticV1>,
}

#[derive(Debug, Serialize)]
#[non_exhaustive]
pub struct ArchiveReportV1 {
    pub version: Option<u16>,
    pub format: Option<ArchiveFormatV1>,
    pub archive_size_bytes: u64,
    pub original_size_bytes: Option<u64>,
    pub payload_mode: Option<PayloadModeV1>,
    pub chunk_count: Option<u64>,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ArchiveFormatV1 {
    DpackV1,
    DpackV2,
}

impl ArchiveFormatV1 {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::DpackV1 => "dpack_v1",
            Self::DpackV2 => "dpack_v2",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum PayloadModeV1 {
    RawZstd,
    CsvColumnarDictionary,
    ChunkedRawZstd,
}

impl PayloadModeV1 {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::RawZstd => "raw_zstd",
            Self::CsvColumnarDictionary => "csv_columnar_dictionary",
            Self::ChunkedRawZstd => "chunked_raw_zstd",
        }
    }
}

#[derive(Debug, Serialize)]
#[non_exhaustive]
pub struct ValidationChecksV1 {
    pub header: CheckStatusV1,
    pub metadata: CheckStatusV1,
    pub payload_structure: CheckStatusV1,
    pub decompression: CheckStatusV1,
    pub restored_length: CheckStatusV1,
    pub chunk_table: CheckStatusV1,
    pub per_chunk_sha256: CheckStatusV1,
    pub global_sha256: CheckStatusV1,
    pub trailing_data: CheckStatusV1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum CheckStatusV1 {
    Passed,
    Failed,
    NotAvailable,
    NotApplicable,
    NotCompleted,
}

impl CheckStatusV1 {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Passed => "passed",
            Self::Failed => "failed",
            Self::NotAvailable => "not_available",
            Self::NotApplicable => "not_applicable",
            Self::NotCompleted => "not_completed",
        }
    }
}

#[derive(Debug, Serialize)]
#[non_exhaustive]
pub struct AgainstReportV1 {
    pub status: AgainstStatusV1,
    pub source_size_bytes: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum AgainstStatusV1 {
    NotRequested,
    Matched,
    Mismatched,
    NotCompleted,
}

impl AgainstStatusV1 {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::NotRequested => "not_requested",
            Self::Matched => "matched",
            Self::Mismatched => "mismatched",
            Self::NotCompleted => "not_completed",
        }
    }
}

#[derive(Debug, Serialize)]
#[non_exhaustive]
pub struct ValidationDiagnosticV1 {
    pub code: &'static str,
    pub severity: ValidationSeverityV1,
    pub message: String,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ValidationSeverityV1 {
    Error,
}

impl ValidationReportV1 {
    pub(crate) fn failure_summary(&self) -> Option<&str> {
        self.diagnostics
            .first()
            .map(|diagnostic| diagnostic.message.as_str())
    }
}

pub(crate) fn validate_path_with_control(
    archive_path: &Path,
    against_path: Option<&Path>,
    options: ValidationOptions,
    cancellation: Option<&CancellationToken>,
) -> OperationResult<ValidationReportV1> {
    checkpoint(cancellation)?;
    let source_identity = against_path
        .map(|source_path| hash_regular_file(source_path, "against source", cancellation))
        .transpose()?;
    checkpoint(cancellation)?;
    let archive_file = open_regular_file(archive_path, "archive")?;
    let archive_size = archive_file.metadata()?.len();
    let mut archive_reader = BufReader::new(archive_file);
    let mut report = ValidationReportV1::new(archive_size, against_path.is_some());
    let version = match read_archive_version(&mut archive_reader) {
        Ok(version) => version,
        Err(error) => {
            report.checks.header = CheckStatusV1::Failed;
            report.fail("ARCHIVE_HEADER_INVALID", error.to_string());
            return Ok(report);
        }
    };
    report.archive.version = Some(version);
    archive_reader.seek(SeekFrom::Start(0))?;

    let restored_identity = match version {
        CURRENT_VERSION => validate_v1(
            &mut archive_reader,
            archive_size,
            options,
            &mut report,
            cancellation,
        ),
        storage::chunked::CHUNKED_VERSION => {
            validate_v2(&mut archive_reader, options, &mut report, cancellation)
        }
        unsupported => {
            report.checks.header = CheckStatusV1::Failed;
            report.fail(
                "UNSUPPORTED_ARCHIVE_VERSION",
                format!("unsupported .dpack version {unsupported}"),
            );
            return Ok(report);
        }
    };

    let restored_identity = match restored_identity {
        Ok(identity) => identity,
        Err(ValidationControlError::Cancelled) => return Err(OperationError::Cancelled),
        Err(ValidationControlError::Failed(failure))
            if failure.stage == FailureStage::Operational =>
        {
            return Err(DatapackError::InvalidFormat(format!(
                "archive became unreadable during validation: {}",
                failure.message
            ))
            .into())
        }
        Err(ValidationControlError::Failed(failure)) => {
            failure.apply(&mut report);
            return Ok(report);
        }
    };

    checkpoint(cancellation)?;
    if let Some(source_identity) = source_identity {
        report.against.source_size_bytes = Some(source_identity.size);
        if source_identity == restored_identity {
            report.against.status = AgainstStatusV1::Matched;
        } else {
            report.against.status = AgainstStatusV1::Mismatched;
            report.fail(
                "AGAINST_MISMATCH",
                "verified restored bytes do not match the against source".to_string(),
            );
            return Ok(report);
        }
    }

    checkpoint(cancellation)?;
    report.valid = true;
    Ok(report)
}

fn checkpoint(cancellation: Option<&CancellationToken>) -> OperationResult<()> {
    match cancellation {
        Some(cancellation) => cancellation.checkpoint(),
        None => Ok(()),
    }
}

enum ValidationControlError {
    Cancelled,
    Failed(ValidationFailure),
}

impl From<ValidationFailure> for ValidationControlError {
    fn from(error: ValidationFailure) -> Self {
        Self::Failed(error)
    }
}

impl ValidationReportV1 {
    fn new(archive_size_bytes: u64, against_requested: bool) -> Self {
        Self {
            schema_version: 1,
            report_type: "validation",
            valid: false,
            archive: ArchiveReportV1 {
                version: None,
                format: None,
                archive_size_bytes,
                original_size_bytes: None,
                payload_mode: None,
                chunk_count: None,
            },
            checks: ValidationChecksV1 {
                header: CheckStatusV1::NotCompleted,
                metadata: CheckStatusV1::NotCompleted,
                payload_structure: CheckStatusV1::NotCompleted,
                decompression: CheckStatusV1::NotCompleted,
                restored_length: CheckStatusV1::NotCompleted,
                chunk_table: CheckStatusV1::NotCompleted,
                per_chunk_sha256: CheckStatusV1::NotCompleted,
                global_sha256: CheckStatusV1::NotCompleted,
                trailing_data: CheckStatusV1::NotCompleted,
            },
            against: AgainstReportV1 {
                status: if against_requested {
                    AgainstStatusV1::NotCompleted
                } else {
                    AgainstStatusV1::NotRequested
                },
                source_size_bytes: None,
            },
            diagnostics: Vec::new(),
        }
    }

    fn fail(&mut self, code: &'static str, message: String) {
        self.valid = false;
        self.diagnostics.push(ValidationDiagnosticV1 {
            code,
            severity: ValidationSeverityV1::Error,
            message,
        });
    }
}

fn validate_v1<R: Read + Seek>(
    reader: &mut R,
    archive_size: u64,
    options: ValidationOptions,
    report: &mut ValidationReportV1,
    cancellation: Option<&CancellationToken>,
) -> std::result::Result<ContentIdentity, ValidationControlError> {
    if cancellation.is_some_and(CancellationToken::is_cancelled) {
        return Err(ValidationControlError::Cancelled);
    }
    report.archive.format = Some(ArchiveFormatV1::DpackV1);
    report.checks.chunk_table = CheckStatusV1::NotApplicable;
    report.checks.per_chunk_sha256 = CheckStatusV1::NotAvailable;
    report.checks.global_sha256 = CheckStatusV1::NotAvailable;
    // Frozen v1 has neither a payload length nor an authenticated end offset.
    // Successful decoding proves that the payload is consumable, but cannot
    // independently prove that every accepted trailing frame was intentional.
    report.checks.trailing_data = CheckStatusV1::NotAvailable;

    let metadata =
        storage::read_v1_archive_header_with_memory_limit(reader, Some(options.max_memory_bytes))
            .map_err(|error| classify_v1_header_failure(error, archive_size))?;
    report.checks.header = CheckStatusV1::Passed;
    report.checks.metadata = CheckStatusV1::Passed;
    report.archive.original_size_bytes = Some(metadata.original_size);
    report.archive.payload_mode = Some(payload_mode(&metadata.payload_kind));
    enforce_output_limit(metadata.original_size, options.max_output_bytes)?;

    let identity = if matches!(
        metadata.payload_kind,
        PayloadKind::RawZstd | PayloadKind::Plain | PayloadKind::Dictionary
    ) {
        let mut hashing_writer = HashingWriter::default();
        match storage::restore_raw_zstd_stream_with_control(
            &metadata,
            reader,
            &mut hashing_writer,
            cancellation,
        ) {
            Ok(_) => {}
            Err(OperationError::Cancelled) => return Err(ValidationControlError::Cancelled),
            Err(OperationError::Failed(error)) => {
                return Err(classify_v1_payload_failure(error).into())
            }
        }
        hashing_writer.finish()
    } else {
        let archive_size = report.archive.archive_size_bytes;
        enforce_v1_columnar_memory_limit(
            archive_size,
            metadata.original_size,
            options.max_memory_bytes,
        )?;
        reader.seek(SeekFrom::Start(0)).map_err(|error| {
            ValidationFailure::new(
                "VALIDATION_IO_ERROR",
                error.to_string(),
                FailureStage::Operational,
            )
        })?;
        let bytes = read_reader_bounded(reader, options.max_memory_bytes).map_err(|error| {
            let message = error.to_string();
            if message.contains("validation memory")
                || message.contains("configured validation memory")
            {
                ValidationFailure::new(
                    "VALIDATION_MEMORY_LIMIT_REACHED",
                    message,
                    FailureStage::Limits,
                )
            } else {
                ValidationFailure::new("VALIDATION_IO_ERROR", message, FailureStage::Operational)
            }
        })?;
        let archive = storage::decode_archive(&bytes).map_err(|error| {
            ValidationFailure::new(
                "V1_PAYLOAD_INVALID",
                error.to_string(),
                FailureStage::V1Payload,
            )
        })?;
        let restored = storage::restore_archive(&archive).map_err(classify_v1_payload_failure)?;
        ContentIdentity::from_bytes(&restored)
    };

    report.checks.payload_structure = CheckStatusV1::Passed;
    report.checks.decompression = CheckStatusV1::Passed;
    report.checks.restored_length = CheckStatusV1::Passed;
    if cancellation.is_some_and(CancellationToken::is_cancelled) {
        return Err(ValidationControlError::Cancelled);
    }
    Ok(identity)
}

fn validate_v2<R: Read + Seek>(
    reader: &mut R,
    options: ValidationOptions,
    report: &mut ValidationReportV1,
    cancellation: Option<&CancellationToken>,
) -> std::result::Result<ContentIdentity, ValidationControlError> {
    report.archive.format = Some(ArchiveFormatV1::DpackV2);
    report.archive.payload_mode = Some(PayloadModeV1::ChunkedRawZstd);

    let limits = storage::chunked::V2ArchiveLimits {
        max_output_bytes: options.max_output_bytes,
        max_chunks: options.max_chunks,
        max_memory_bytes: Some(options.max_memory_bytes),
    };
    let info = storage::chunked::read_v2_archive_info_with_limits(reader, limits)
        .map_err(classify_v2_structure_failure)?;
    report.checks.header = CheckStatusV1::Passed;
    report.checks.metadata = CheckStatusV1::Passed;
    report.checks.chunk_table = CheckStatusV1::Passed;
    report.checks.trailing_data = CheckStatusV1::Passed;
    report.archive.original_size_bytes = Some(info.original_size_bytes);
    report.archive.chunk_count = Some(info.chunk_count);

    match storage::chunked::validate_raw_zstd_chunked_payload_with_control(
        reader,
        &info,
        cancellation,
    ) {
        Ok(()) => {}
        Err(storage::chunked::ControlledV2ValidationError::Cancelled) => {
            return Err(ValidationControlError::Cancelled)
        }
        Err(storage::chunked::ControlledV2ValidationError::Failed(error)) => {
            use storage::chunked::V2ValidationStage;

            let stage = error.stage;
            let storage_message = error.into_datapack_error().to_string();
            let failure = match stage {
                V2ValidationStage::ChunkPayload => ValidationFailure::new(
                    "CHUNK_DECOMPRESSION_FAILED",
                    storage_message,
                    FailureStage::V2Payload,
                ),
                V2ValidationStage::ChunkLength => ValidationFailure::new(
                    "RESTORED_LENGTH_MISMATCH",
                    storage_message,
                    FailureStage::ChunkLength,
                ),
                V2ValidationStage::ChunkHash => ValidationFailure::new(
                    "CHUNK_HASH_MISMATCH",
                    "a v2 chunk SHA-256 does not match the stored digest".to_string(),
                    FailureStage::ChunkHash,
                ),
                V2ValidationStage::RestoredTotal => ValidationFailure::new(
                    "RESTORED_LENGTH_MISMATCH",
                    storage_message,
                    FailureStage::RestoredLength,
                ),
                V2ValidationStage::GlobalHash => ValidationFailure::new(
                    "GLOBAL_HASH_MISMATCH",
                    "the v2 global SHA-256 does not match the restored byte stream".to_string(),
                    FailureStage::GlobalHash,
                ),
            };
            return Err(ValidationControlError::Failed(failure));
        }
    }

    report.checks.payload_structure = CheckStatusV1::Passed;
    report.checks.decompression = CheckStatusV1::Passed;
    report.checks.restored_length = CheckStatusV1::Passed;
    report.checks.per_chunk_sha256 = CheckStatusV1::Passed;
    report.checks.global_sha256 = CheckStatusV1::Passed;
    Ok(ContentIdentity {
        size: info.original_size_bytes,
        sha256: info.global_sha256,
    })
}

fn classify_v2_structure_failure(error: DatapackError) -> ValidationFailure {
    let message = error.to_string();
    if message.contains("--max-output-mb") {
        ValidationFailure::new(
            "DECLARED_OUTPUT_LIMIT_REACHED",
            message,
            FailureStage::Limits,
        )
    } else if message.contains("--max-chunks") {
        ValidationFailure::new("CHUNK_COUNT_LIMIT_REACHED", message, FailureStage::Limits)
    } else if message.contains("--max-memory-mb") {
        ValidationFailure::new(
            "VALIDATION_MEMORY_LIMIT_REACHED",
            message,
            FailureStage::Limits,
        )
    } else {
        let stage = if message.contains("trailing bytes") {
            FailureStage::TrailingData
        } else if message.contains("chunk table")
            || message.contains("chunk-table")
            || message.contains("chunk count")
            || message.contains("chunk ")
            || message.contains("chunks")
            || message.contains("compressed range")
            || message.contains("original offset")
            || message.contains("unreferenced gap")
        {
            FailureStage::ChunkTable
        } else if message.contains("empty archive has an incorrect global SHA-256")
            || message.contains("chunk_size_target")
        {
            FailureStage::Metadata
        } else {
            FailureStage::Header
        };
        ValidationFailure::new("V2_HEADER_OR_CHUNK_TABLE_INVALID", message, stage)
    }
}

fn classify_v1_header_failure(error: DatapackError, archive_size: u64) -> ValidationFailure {
    let message = error.to_string();
    if message.contains("--max-memory-mb") {
        return ValidationFailure::new(
            "VALIDATION_MEMORY_LIMIT_REACHED",
            message,
            FailureStage::MetadataLimit,
        );
    }
    let fixed_header_complete = archive_size >= storage::V1_FIXED_HEADER_LEN as u64;
    let stage = if message.contains("metadata")
        || message.contains("extension")
        || (fixed_header_complete && message.contains("I/O error"))
    {
        FailureStage::Metadata
    } else {
        FailureStage::Header
    };
    ValidationFailure::new("V1_HEADER_OR_METADATA_INVALID", message, stage)
}

fn classify_v1_payload_failure(error: DatapackError) -> ValidationFailure {
    let message = error.to_string();
    if message.contains("decompressed size mismatch")
        || message.contains("decompressed output exceeds expected size")
        || (message.contains("restored size") && message.contains("does not match original size"))
    {
        ValidationFailure::new("RESTORED_LENGTH_MISMATCH", message, FailureStage::V1Length)
    } else {
        ValidationFailure::new("V1_PAYLOAD_INVALID", message, FailureStage::V1Payload)
    }
}

fn payload_mode(kind: &PayloadKind) -> PayloadModeV1 {
    match kind {
        PayloadKind::CsvColumnarDictionary => PayloadModeV1::CsvColumnarDictionary,
        PayloadKind::RawZstd | PayloadKind::Plain | PayloadKind::Dictionary => {
            PayloadModeV1::RawZstd
        }
    }
}

fn read_archive_version<R: Read>(reader: &mut R) -> Result<u16> {
    let mut prefix = [0u8; 7];
    reader.read_exact(&mut prefix).map_err(|error| {
        if error.kind() == std::io::ErrorKind::UnexpectedEof {
            DatapackError::InvalidFormat(
                "truncated .dpack header: expected at least 7 bytes".into(),
            )
        } else {
            error.into()
        }
    })?;
    storage::archive_version_from_bytes(&prefix)
}

fn enforce_output_limit(
    original_size: u64,
    maximum: Option<u64>,
) -> std::result::Result<(), ValidationFailure> {
    if maximum.is_some_and(|limit| original_size > limit) {
        return Err(ValidationFailure::new(
            "DECLARED_OUTPUT_LIMIT_REACHED",
            format!(
                "archive declares {original_size} restored bytes, exceeding the configured output limit"
            ),
            FailureStage::Limits,
        ));
    }
    Ok(())
}

fn enforce_v1_columnar_memory_limit(
    archive_size: u64,
    original_size: u64,
    maximum: u64,
) -> std::result::Result<(), ValidationFailure> {
    let encoded_limit = original_size
        .checked_mul(V1_COLUMNAR_EXPANSION_FACTOR)
        .and_then(|value| value.checked_add(V1_COLUMNAR_FIXED_ALLOWANCE))
        .ok_or_else(|| {
            ValidationFailure::new(
                "VALIDATION_MEMORY_LIMIT_REACHED",
                "v1 columnar validation memory estimate overflowed".to_string(),
                FailureStage::Limits,
            )
        })?;
    let estimate = archive_size
        .checked_mul(2)
        .and_then(|value| value.checked_add(encoded_limit))
        .and_then(|value| value.checked_add(original_size))
        .ok_or_else(|| {
            ValidationFailure::new(
                "VALIDATION_MEMORY_LIMIT_REACHED",
                "v1 columnar validation memory estimate overflowed".to_string(),
                FailureStage::Limits,
            )
        })?;
    if estimate > maximum {
        return Err(ValidationFailure::new(
            "VALIDATION_MEMORY_LIMIT_REACHED",
            format!(
                "estimated v1 columnar validation memory {estimate} bytes exceeds the configured limit of {maximum} bytes"
            ),
            FailureStage::Limits,
        ));
    }
    Ok(())
}

fn read_reader_bounded<R: Read>(reader: &mut R, maximum: u64) -> Result<Vec<u8>> {
    let read_limit = maximum.checked_add(1).ok_or_else(|| {
        DatapackError::InvalidFormat("validation memory limit is too large".to_string())
    })?;
    let mut limited_reader = reader.take(read_limit);
    let mut bytes = Vec::new();
    let initial = usize::try_from(maximum.min(HASH_BUFFER_BYTES as u64)).map_err(|_| {
        DatapackError::InvalidFormat(
            "validation memory limit exceeds platform capacity".to_string(),
        )
    })?;
    bytes.try_reserve_exact(initial).map_err(|error| {
        DatapackError::InvalidFormat(format!("cannot reserve validation memory: {error}"))
    })?;
    limited_reader.read_to_end(&mut bytes)?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > maximum {
        return Err(DatapackError::InvalidFormat(format!(
            "archive exceeds the configured validation memory limit of {maximum} bytes"
        )));
    }
    Ok(bytes)
}

fn open_regular_file(path: &Path, purpose: &str) -> Result<File> {
    let file = File::open(path).map_err(|error| {
        DatapackError::InvalidFormat(format!(
            "{purpose} path '{}' was not found or is not readable: {error}",
            path.display()
        ))
    })?;
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(DatapackError::InvalidFormat(format!(
            "{purpose} path '{}' is not a regular file",
            path.display()
        )));
    }
    Ok(file)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ContentIdentity {
    size: u64,
    sha256: [u8; 32],
}

impl ContentIdentity {
    fn from_bytes(bytes: &[u8]) -> Self {
        Self {
            size: bytes.len() as u64,
            sha256: Sha256::digest(bytes).into(),
        }
    }
}

#[derive(Default)]
struct HashingWriter {
    size: u64,
    hasher: Sha256,
}

impl HashingWriter {
    fn finish(self) -> ContentIdentity {
        ContentIdentity {
            size: self.size,
            sha256: self.hasher.finalize().into(),
        }
    }
}

impl Write for HashingWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let length = u64::try_from(bytes.len())
            .map_err(|_| std::io::Error::other("hash input length exceeds u64 capacity"))?;
        self.size = self
            .size
            .checked_add(length)
            .ok_or_else(|| std::io::Error::other("hash input size overflow"))?;
        self.hasher.update(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn hash_regular_file(
    path: &Path,
    purpose: &str,
    cancellation: Option<&CancellationToken>,
) -> OperationResult<ContentIdentity> {
    let mut reader = BufReader::new(open_regular_file(path, purpose)?);
    hash_reader(&mut reader, cancellation)
}

fn hash_reader<R: Read>(
    reader: &mut R,
    cancellation: Option<&CancellationToken>,
) -> OperationResult<ContentIdentity> {
    let mut writer = HashingWriter::default();
    let mut buffer = [0u8; HASH_BUFFER_BYTES];
    loop {
        checkpoint(cancellation)?;
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        writer.write_all(&buffer[..read])?;
    }
    Ok(writer.finish())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FailureStage {
    Header,
    Metadata,
    MetadataLimit,
    ChunkTable,
    TrailingData,
    V1Payload,
    V1Length,
    V2Payload,
    ChunkLength,
    ChunkHash,
    RestoredLength,
    GlobalHash,
    Limits,
    Operational,
}

struct ValidationFailure {
    code: &'static str,
    message: String,
    stage: FailureStage,
}

impl ValidationFailure {
    fn new(code: &'static str, message: String, stage: FailureStage) -> Self {
        Self {
            code,
            message,
            stage,
        }
    }

    fn apply(self, report: &mut ValidationReportV1) {
        match self.stage {
            FailureStage::Header => {
                report.checks.header = CheckStatusV1::Failed;
            }
            FailureStage::Metadata => {
                report.checks.header = CheckStatusV1::Passed;
                report.checks.metadata = CheckStatusV1::Failed;
            }
            FailureStage::MetadataLimit => {
                report.checks.header = CheckStatusV1::Passed;
            }
            FailureStage::ChunkTable => {
                report.checks.header = CheckStatusV1::Passed;
                report.checks.metadata = CheckStatusV1::Passed;
                report.checks.chunk_table = CheckStatusV1::Failed;
            }
            FailureStage::TrailingData => {
                report.checks.header = CheckStatusV1::Passed;
                report.checks.metadata = CheckStatusV1::Passed;
                report.checks.chunk_table = CheckStatusV1::Passed;
                report.checks.trailing_data = CheckStatusV1::Failed;
            }
            FailureStage::V1Payload => {
                report.checks.payload_structure = CheckStatusV1::Failed;
                report.checks.decompression = CheckStatusV1::Failed;
            }
            FailureStage::V1Length => {
                report.checks.restored_length = CheckStatusV1::Failed;
            }
            FailureStage::V2Payload => {
                mark_v2_structure_passed(&mut report.checks);
                report.checks.decompression = CheckStatusV1::Failed;
            }
            FailureStage::ChunkLength => {
                mark_v2_structure_passed(&mut report.checks);
                report.checks.restored_length = CheckStatusV1::Failed;
            }
            FailureStage::ChunkHash => {
                mark_v2_structure_passed(&mut report.checks);
                report.checks.per_chunk_sha256 = CheckStatusV1::Failed;
            }
            FailureStage::RestoredLength => {
                mark_v2_structure_passed(&mut report.checks);
                report.checks.decompression = CheckStatusV1::Passed;
                report.checks.restored_length = CheckStatusV1::Failed;
                report.checks.per_chunk_sha256 = CheckStatusV1::Passed;
            }
            FailureStage::GlobalHash => {
                mark_v2_structure_passed(&mut report.checks);
                report.checks.decompression = CheckStatusV1::Passed;
                report.checks.restored_length = CheckStatusV1::Passed;
                report.checks.per_chunk_sha256 = CheckStatusV1::Passed;
                report.checks.global_sha256 = CheckStatusV1::Failed;
            }
            FailureStage::Limits => {}
            FailureStage::Operational => {}
        }
        report.fail(self.code, self.message);
    }
}

fn mark_v2_structure_passed(checks: &mut ValidationChecksV1) {
    checks.header = CheckStatusV1::Passed;
    checks.metadata = CheckStatusV1::Passed;
    checks.payload_structure = CheckStatusV1::Passed;
    checks.chunk_table = CheckStatusV1::Passed;
    checks.trailing_data = CheckStatusV1::Passed;
}

#[cfg(test)]
mod tests {
    use std::io::{Cursor, Read};

    use super::*;

    struct CancelAfterFirstRead {
        inner: Cursor<Vec<u8>>,
        cancellation: CancellationToken,
        cancelled: bool,
    }

    impl Read for CancelAfterFirstRead {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            let read = self.inner.read(buffer)?;
            if read > 0 && !self.cancelled {
                self.cancellation.cancel();
                self.cancelled = true;
            }
            Ok(read)
        }
    }

    #[test]
    fn hashing_observes_cancellation_after_a_bounded_read() {
        let cancellation = CancellationToken::new();
        let mut reader = CancelAfterFirstRead {
            inner: Cursor::new(vec![0x5a; HASH_BUFFER_BYTES * 2]),
            cancellation: cancellation.clone(),
            cancelled: false,
        };

        let result = hash_reader(&mut reader, Some(&cancellation));

        assert!(matches!(result, Err(OperationError::Cancelled)));
        assert!(cancellation.is_cancelled());
        assert_eq!(reader.inner.position(), HASH_BUFFER_BYTES as u64);
    }
}
