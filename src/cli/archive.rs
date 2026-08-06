use std::path::Path;

use crate::error::Result;
use crate::metadata::PayloadKind;
use crate::planning::ArchiveMode;
use crate::storage;

pub(super) fn encode_for_plan(input: &Path, bytes: &[u8], mode: ArchiveMode) -> Result<Vec<u8>> {
    Ok(encode_for_plan_detailed(input, bytes, mode)?.0)
}

pub(super) fn encode_for_plan_detailed(
    input: &Path,
    bytes: &[u8],
    mode: ArchiveMode,
) -> Result<(Vec<u8>, ArchiveMode, Option<String>)> {
    match mode {
        ArchiveMode::RawZstd => Ok((
            storage::encode_raw_zstd_archive(input, bytes)?,
            ArchiveMode::RawZstd,
            None,
        )),
        ArchiveMode::CsvColumnarDictionary => {
            let (archive, error) =
                storage::encode_columnar_dictionary_archive_detailed(input, bytes)?;
            match archive {
                Some(archive) => Ok((archive, ArchiveMode::CsvColumnarDictionary, error)),
                None => Ok((
                    storage::encode_raw_zstd_archive(input, bytes)?,
                    ArchiveMode::RawZstd,
                    error,
                )),
            }
        }
    }
}

pub(super) fn archive_mode_for_payload(payload_kind: &PayloadKind) -> ArchiveMode {
    match payload_kind {
        PayloadKind::CsvColumnarDictionary => ArchiveMode::CsvColumnarDictionary,
        PayloadKind::RawZstd | PayloadKind::Plain | PayloadKind::Dictionary => ArchiveMode::RawZstd,
    }
}

pub(super) fn encode_best_archive(
    input: &Path,
    bytes: &[u8],
) -> Result<(Vec<u8>, ArchiveMode, usize, Option<String>)> {
    let raw = storage::encode_raw_zstd_archive(input, bytes)?;
    let (columnar, columnar_error) =
        storage::encode_columnar_dictionary_archive_detailed(input, bytes)?;
    if let Some(columnar) = columnar {
        if columnar.len() < raw.len() {
            let saved = raw.len() - columnar.len();
            return Ok((columnar, ArchiveMode::CsvColumnarDictionary, saved, None));
        }
        let saved = columnar.len().saturating_sub(raw.len());
        Ok((raw, ArchiveMode::RawZstd, saved, columnar_error))
    } else {
        Ok((raw, ArchiveMode::RawZstd, 0, columnar_error))
    }
}
