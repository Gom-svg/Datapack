use std::path::Path;

use crate::analysis;
use crate::error::Result;
use crate::metadata::PayloadKind;
use crate::planning::{ArchiveMode, ColumnExecutionPlan};
use crate::storage;

pub(super) fn encode_for_plan(
    input: &Path,
    bytes: &[u8],
    mode: ArchiveMode,
    delimiter: u8,
    execution_plan: &ColumnExecutionPlan,
) -> Result<Vec<u8>> {
    Ok(encode_for_plan_detailed(input, bytes, mode, delimiter, execution_plan)?.0)
}

pub(super) fn encode_for_plan_detailed(
    input: &Path,
    bytes: &[u8],
    mode: ArchiveMode,
    delimiter: u8,
    execution_plan: &ColumnExecutionPlan,
) -> Result<(Vec<u8>, ArchiveMode, Option<String>)> {
    match mode {
        ArchiveMode::RawZstd => Ok((
            storage::encode_raw_zstd_archive(input, bytes)?,
            ArchiveMode::RawZstd,
            None,
        )),
        ArchiveMode::CsvColumnarDictionary => {
            if !analysis::structured_compression_eligible_bytes(bytes, delimiter) {
                return Ok((
                    storage::encode_raw_zstd_archive(input, bytes)?,
                    ArchiveMode::RawZstd,
                    Some(
                        "canonical structured-compression eligibility was not established for the analyzed delimiter"
                            .to_string(),
                    ),
                ));
            }
            let (archive, error) = storage::encode_columnar_dictionary_archive_with_plan_detailed(
                input,
                bytes,
                delimiter,
                execution_plan,
            )?;
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
    delimiter: u8,
    execution_plan: &ColumnExecutionPlan,
) -> Result<(Vec<u8>, ArchiveMode, usize, Option<String>)> {
    let raw = storage::encode_raw_zstd_archive(input, bytes)?;
    if !analysis::structured_compression_eligible_bytes(bytes, delimiter) {
        return Ok((
            raw,
            ArchiveMode::RawZstd,
            0,
            Some(
                "canonical structured-compression eligibility was not established for the analyzed delimiter"
                    .to_string(),
            ),
        ));
    }
    let (columnar, columnar_error) =
        storage::encode_columnar_dictionary_archive_with_plan_detailed(
            input,
            bytes,
            delimiter,
            execution_plan,
        )?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::planning::{ColumnPlan, ColumnStrategy, CompressionPlan};

    #[test]
    fn malformed_execution_contract_falls_back_to_exact_raw_archive() {
        let input = Path::new("input.csv");
        let bytes = b"left,right\nA,B\n";
        let compression_plan = CompressionPlan {
            archive_mode: ArchiveMode::CsvColumnarDictionary,
            columns: vec![ColumnPlan {
                column_index: 0,
                column_name: "left".to_string(),
                strategy: ColumnStrategy::Dictionary,
                reason: "test".to_string(),
            }],
            estimated_savings_percent: 20.0,
            estimated_memory_mb: 1.0,
            planning_time_ms: 0,
            reason: "test".to_string(),
        };
        let execution_plan = ColumnExecutionPlan::from_compression_plan(&compression_plan, 100, 1);

        let (encoded, actual_mode, error) = encode_for_plan_detailed(
            input,
            bytes,
            ArchiveMode::CsvColumnarDictionary,
            b',',
            &execution_plan,
        )
        .unwrap();

        assert_eq!(actual_mode, ArchiveMode::RawZstd);
        assert!(error
            .as_deref()
            .is_some_and(|message| message.contains("plan contains 1 columns")));
        let archive = storage::decode_archive(&encoded).unwrap();
        assert_eq!(archive.metadata.payload_kind, PayloadKind::RawZstd);
        assert_eq!(storage::restore_archive(&archive).unwrap(), bytes);
    }
}
