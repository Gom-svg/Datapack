use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::Path;
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};

use crate::analysis;
use crate::compression::{planned, zstd_backend};
use crate::error::{DatapackError, Result};
use crate::metadata::PayloadKind;
use crate::planning::{self, ArchiveMode, ColumnExecutionPlan};
use crate::storage;

use super::model::{
    comparison_winners, compression_ratio, median_duration, timing_report, ComparisonLimitationV1,
    ComparisonMethodologyV1, ComparisonReportV1, ComparisonScopeV1, CompetitorReportV1,
    CompetitorValidationV1, SCHEMA_VERSION,
};
use super::temp::ComparisonWorkspace;

const MIB: u64 = 1024 * 1024;
const DEFAULT_QUICK_MAX_INPUT_MB: u64 = 64;
const DEFAULT_PLANNING_SAMPLE_MB: u64 = 64;
const DEFAULT_MAX_DICTIONARY_VALUES: u64 = 65_535;
const DEFAULT_MAX_DICTIONARY_MB: u64 = 64;
const MAX_RUNS: usize = 25;
const IO_BUFFER_BYTES: usize = 256 * 1024;
const MAX_BUFFERED_STRUCTURED_BYTES: u64 = 64 * MIB;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ComparisonMode {
    Quick,
    Full,
}

impl ComparisonMode {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Quick => "quick",
            Self::Full => "full",
        }
    }

    const fn scope_kind(self) -> &'static str {
        match self {
            Self::Quick => "partial",
            Self::Full => "full",
        }
    }

    const fn validation_status(self) -> &'static str {
        match self {
            Self::Quick => "partially_validated",
            Self::Full => "validated",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct CompareOptions {
    pub(crate) mode: ComparisonMode,
    pub(crate) runs: usize,
    pub(crate) max_input_mb: Option<u64>,
}

enum DataPackEncoding {
    RawZstd,
    Structured {
        delimiter: u8,
        execution_plan: ColumnExecutionPlan,
    },
}

struct PreparedDataPack {
    encoding: DataPackEncoding,
    limitations: Vec<ComparisonLimitationV1>,
}

struct CompressionRun {
    elapsed: Duration,
    artifact_size: u64,
    selected_mode: ArchiveMode,
    structured_fallback: bool,
}

pub(crate) fn compare_path(input: &Path, options: CompareOptions) -> Result<ComparisonReportV1> {
    validate_options(options)?;
    let source_metadata = std::fs::metadata(input)?;
    if !source_metadata.is_file() {
        return Err(DatapackError::InvalidFormat(
            "comparison input must be a regular file".to_string(),
        ));
    }
    let source_size = source_metadata.len();
    let compared_size = compared_size(source_size, options)?;
    let prefix_limited = compared_size < source_size;

    let mut workspace = ComparisonWorkspace::create(input)?;
    materialize_snapshot(
        input,
        workspace.measured_input(),
        compared_size,
        compared_size == source_size,
    )?;
    let source_hash = sha256_path_exact(workspace.measured_input(), compared_size)?;

    let planning_started = Instant::now();
    let mut prepared = prepare_datapack(workspace.measured_input(), compared_size)?;
    let planning_elapsed = planning_started.elapsed();
    if options.mode == ComparisonMode::Quick {
        prepared.limitations.insert(
            0,
            ComparisonLimitationV1::new(
                "QUICK_MODE_PARTIAL",
                "Quick mode is a bounded comparison and is not a full-input certification.",
            ),
        );
    }
    if prefix_limited {
        prepared.limitations.push(ComparisonLimitationV1::new(
            "INPUT_PREFIX_LIMITED",
            "Only the configured input prefix was compared.",
        ));
    }

    let mut datapack_compression = Vec::with_capacity(options.runs);
    let mut zstd_compression = Vec::with_capacity(options.runs);
    let mut datapack_size = None;
    let mut zstd_size = None;
    let mut selected_mode = None;
    let mut encoder_fell_back = false;
    let mut datapack_artifact_hash = None;
    let mut zstd_artifact_hash = None;

    for run in 0..options.runs {
        if run % 2 == 0 {
            let datapack = measure_datapack_compression(
                workspace.measured_input(),
                workspace.datapack_archive(),
                compared_size,
                &prepared.encoding,
            )?;
            record_datapack_compression(
                datapack,
                &mut datapack_compression,
                &mut datapack_size,
                &mut selected_mode,
                &mut encoder_fell_back,
            )?;
            record_stable_size(
                "standalone zstd",
                measure_zstd_compression(
                    workspace.measured_input(),
                    workspace.zstd_archive(),
                    compared_size,
                )?,
                &mut zstd_compression,
                &mut zstd_size,
            )?;
        } else {
            record_stable_size(
                "standalone zstd",
                measure_zstd_compression(
                    workspace.measured_input(),
                    workspace.zstd_archive(),
                    compared_size,
                )?,
                &mut zstd_compression,
                &mut zstd_size,
            )?;
            let datapack = measure_datapack_compression(
                workspace.measured_input(),
                workspace.datapack_archive(),
                compared_size,
                &prepared.encoding,
            )?;
            record_datapack_compression(
                datapack,
                &mut datapack_compression,
                &mut datapack_size,
                &mut selected_mode,
                &mut encoder_fell_back,
            )?;
        }

        let current_datapack_size = std::fs::metadata(workspace.datapack_archive())?.len();
        let current_zstd_size = std::fs::metadata(workspace.zstd_archive())?.len();
        if run % 2 == 0 {
            record_stable_artifact_hash(
                "DataPack",
                workspace.datapack_archive(),
                current_datapack_size,
                &mut datapack_artifact_hash,
            )?;
            record_stable_artifact_hash(
                "standalone zstd",
                workspace.zstd_archive(),
                current_zstd_size,
                &mut zstd_artifact_hash,
            )?;
        } else {
            record_stable_artifact_hash(
                "standalone zstd",
                workspace.zstd_archive(),
                current_zstd_size,
                &mut zstd_artifact_hash,
            )?;
            record_stable_artifact_hash(
                "DataPack",
                workspace.datapack_archive(),
                current_datapack_size,
                &mut datapack_artifact_hash,
            )?;
        }
    }

    if encoder_fell_back {
        prepared.limitations.push(ComparisonLimitationV1::new(
            "STRUCTURED_ENCODER_FALLBACK",
            "The structured encoder could not safely honor the plan; DataPack used RawZstd.",
        ));
    }

    let selected_mode = selected_mode.ok_or_else(|| {
        DatapackError::InvalidFormat("comparison did not execute DataPack compression".to_string())
    })?;
    let datapack_size = datapack_size.ok_or_else(|| {
        DatapackError::InvalidFormat("comparison did not produce a DataPack archive".to_string())
    })?;
    let zstd_size = zstd_size.ok_or_else(|| {
        DatapackError::InvalidFormat(
            "comparison did not produce a standalone zstd artifact".to_string(),
        )
    })?;

    let mut datapack_decompression = Vec::with_capacity(options.runs);
    let mut zstd_decompression = Vec::with_capacity(options.runs);
    for run in 0..options.runs {
        if run % 2 == 0 {
            zstd_decompression.push(measure_zstd_decompression(
                workspace.zstd_archive(),
                workspace.zstd_restore(),
                compared_size,
            )?);
            datapack_decompression.push(measure_datapack_decompression(
                workspace.datapack_archive(),
                workspace.datapack_restore(),
                compared_size,
            )?);
        } else {
            datapack_decompression.push(measure_datapack_decompression(
                workspace.datapack_archive(),
                workspace.datapack_restore(),
                compared_size,
            )?);
            zstd_decompression.push(measure_zstd_decompression(
                workspace.zstd_archive(),
                workspace.zstd_restore(),
                compared_size,
            )?);
        }
        if run % 2 == 0 {
            validate_restored_identity(
                "standalone zstd",
                workspace.zstd_restore(),
                compared_size,
                &source_hash,
            )?;
            validate_restored_identity(
                "DataPack",
                workspace.datapack_restore(),
                compared_size,
                &source_hash,
            )?;
        } else {
            validate_restored_identity(
                "DataPack",
                workspace.datapack_restore(),
                compared_size,
                &source_hash,
            )?;
            validate_restored_identity(
                "standalone zstd",
                workspace.zstd_restore(),
                compared_size,
                &source_hash,
            )?;
        }
    }

    let datapack_compression_median = median_duration(&datapack_compression);
    let zstd_compression_median = median_duration(&zstd_compression);
    let datapack_decompression_median = median_duration(&datapack_decompression);
    let zstd_decompression_median = median_duration(&zstd_decompression);
    let validation_status = options.mode.validation_status();

    let report = ComparisonReportV1 {
        schema_version: SCHEMA_VERSION,
        report_type: "comparison",
        mode: options.mode.as_str(),
        scope: ComparisonScopeV1 {
            kind: options.mode.scope_kind(),
            source_size_bytes: source_size,
            compared_size_bytes: compared_size,
            prefix_limited,
        },
        methodology: ComparisonMethodologyV1 {
            runs: options.runs,
            aggregation: "median",
            timing_boundary: "file_to_file",
            planning_included: false,
            planning_time_ms: planning_elapsed.as_secs_f64() * 1_000.0,
            zstd_level: zstd_backend::DEFAULT_LEVEL,
            artifact_stability: "sha256_per_run",
            validation: "sha256_roundtrip_per_run",
        },
        datapack: CompetitorReportV1 {
            artifact_format: "dpack_v1",
            selected_mode: Some(archive_mode_label(selected_mode)),
            artifact_size_bytes: datapack_size,
            compression_ratio: compression_ratio(compared_size, datapack_size),
            compression: timing_report(compared_size, &datapack_compression),
            decompression: timing_report(compared_size, &datapack_decompression),
            validation: CompetitorValidationV1 {
                status: validation_status,
                restored_size_bytes: compared_size,
                sha256_match: true,
            },
        },
        standalone_zstd: CompetitorReportV1 {
            artifact_format: "zstd",
            selected_mode: None,
            artifact_size_bytes: zstd_size,
            compression_ratio: compression_ratio(compared_size, zstd_size),
            compression: timing_report(compared_size, &zstd_compression),
            decompression: timing_report(compared_size, &zstd_decompression),
            validation: CompetitorValidationV1 {
                status: validation_status,
                restored_size_bytes: compared_size,
                sha256_match: true,
            },
        },
        winners: comparison_winners(
            compared_size,
            datapack_size,
            zstd_size,
            datapack_compression_median,
            zstd_compression_median,
            datapack_decompression_median,
            zstd_decompression_median,
        ),
        limitations: prepared.limitations,
    };

    workspace.finish()?;
    Ok(report)
}

fn validate_options(options: CompareOptions) -> Result<()> {
    if !(1..=MAX_RUNS).contains(&options.runs) {
        return Err(DatapackError::InvalidFormat(format!(
            "--runs must be between 1 and {MAX_RUNS}"
        )));
    }
    match (options.mode, options.max_input_mb) {
        (ComparisonMode::Full, Some(_)) => Err(DatapackError::InvalidFormat(
            "--max-input-mb is only valid with --mode quick".to_string(),
        )),
        (ComparisonMode::Quick, Some(0)) => Err(DatapackError::InvalidFormat(
            "--max-input-mb must be greater than zero".to_string(),
        )),
        _ => Ok(()),
    }
}

fn compared_size(source_size: u64, options: CompareOptions) -> Result<u64> {
    match options.mode {
        ComparisonMode::Full => Ok(source_size),
        ComparisonMode::Quick => {
            let limit_mb = options.max_input_mb.unwrap_or(DEFAULT_QUICK_MAX_INPUT_MB);
            let limit = limit_mb.checked_mul(MIB).ok_or_else(|| {
                DatapackError::InvalidFormat("--max-input-mb is too large".to_string())
            })?;
            Ok(source_size.min(limit))
        }
    }
}

fn prepare_datapack(input: &Path, input_size: u64) -> Result<PreparedDataPack> {
    let analysis = match analysis::analyze_cli_path(input, DEFAULT_PLANNING_SAMPLE_MB) {
        Ok(analysis) => analysis,
        Err(DatapackError::InvalidCsv(_)) => {
            return Ok(PreparedDataPack {
                encoding: DataPackEncoding::RawZstd,
                limitations: vec![ComparisonLimitationV1::new(
                    "STRUCTURED_ANALYSIS_UNAVAILABLE",
                    "Structured analysis was unavailable; DataPack used RawZstd.",
                )],
            });
        }
        Err(error) => return Err(error),
    };

    if analysis.requires_raw_fallback() {
        return Ok(PreparedDataPack {
            encoding: DataPackEncoding::RawZstd,
            limitations: vec![ComparisonLimitationV1::new(
                "STRUCTURED_ANALYSIS_LIMITED",
                "Structured analysis reached a safety limit; DataPack used RawZstd.",
            )],
        });
    }

    let delimiter = analysis.structured_delimiter();
    let mut plan = analysis.plan;
    planning::apply_dictionary_limits(
        &mut plan,
        &analysis.columns,
        DEFAULT_MAX_DICTIONARY_VALUES,
        DEFAULT_MAX_DICTIONARY_MB,
    );
    if plan.archive_mode == ArchiveMode::RawZstd {
        return Ok(PreparedDataPack {
            encoding: DataPackEncoding::RawZstd,
            limitations: Vec::new(),
        });
    }
    if input_size > MAX_BUFFERED_STRUCTURED_BYTES {
        return Ok(PreparedDataPack {
            encoding: DataPackEncoding::RawZstd,
            limitations: vec![ComparisonLimitationV1::new(
                "STRUCTURED_COMPARE_MEMORY_LIMIT",
                "The structured comparison buffering ceiling was reached; DataPack used RawZstd.",
            )],
        });
    }

    Ok(PreparedDataPack {
        encoding: DataPackEncoding::Structured {
            delimiter,
            execution_plan: ColumnExecutionPlan::from_compression_plan(
                &plan,
                DEFAULT_MAX_DICTIONARY_VALUES,
                DEFAULT_MAX_DICTIONARY_MB,
            ),
        },
        limitations: Vec::new(),
    })
}

fn measure_datapack_compression(
    input: &Path,
    output: &Path,
    input_size: u64,
    encoding: &DataPackEncoding,
) -> Result<CompressionRun> {
    let started = Instant::now();
    let (selected_mode, structured_fallback) = match encoding {
        DataPackEncoding::RawZstd => {
            write_raw_datapack(input, output, input_size)?;
            (ArchiveMode::RawZstd, false)
        }
        DataPackEncoding::Structured {
            delimiter,
            execution_plan,
        } => {
            let bytes = read_path_exact(input, input_size)?;
            let (archive, selected_mode, fallback) = planned::encode_for_plan_detailed(
                input,
                &bytes,
                ArchiveMode::CsvColumnarDictionary,
                *delimiter,
                execution_plan,
            )?;
            let mut writer = BufWriter::with_capacity(IO_BUFFER_BYTES, File::create(output)?);
            writer.write_all(&archive)?;
            writer.flush()?;
            (selected_mode, fallback.is_some())
        }
    };
    let elapsed = started.elapsed();
    Ok(CompressionRun {
        elapsed,
        artifact_size: std::fs::metadata(output)?.len(),
        selected_mode,
        structured_fallback,
    })
}

fn record_datapack_compression(
    run: CompressionRun,
    samples: &mut Vec<Duration>,
    stable_size: &mut Option<u64>,
    stable_mode: &mut Option<ArchiveMode>,
    structured_fallback: &mut bool,
) -> Result<()> {
    ensure_stable_value("DataPack archive size", stable_size, run.artifact_size)?;
    ensure_stable_value("DataPack selected mode", stable_mode, run.selected_mode)?;
    *structured_fallback |= run.structured_fallback;
    samples.push(run.elapsed);
    Ok(())
}

fn record_stable_size(
    label: &str,
    run: (Duration, u64),
    samples: &mut Vec<Duration>,
    stable_size: &mut Option<u64>,
) -> Result<()> {
    ensure_stable_value(&format!("{label} artifact size"), stable_size, run.1)?;
    samples.push(run.0);
    Ok(())
}

fn ensure_stable_value<T: Copy + PartialEq + std::fmt::Debug>(
    label: &str,
    expected: &mut Option<T>,
    actual: T,
) -> Result<()> {
    match expected {
        Some(value) if *value != actual => Err(DatapackError::InvalidFormat(format!(
            "{label} changed across runs: expected {value:?}, got {actual:?}"
        ))),
        Some(_) => Ok(()),
        None => {
            *expected = Some(actual);
            Ok(())
        }
    }
}

fn record_stable_artifact_hash(
    label: &str,
    path: &Path,
    artifact_size: u64,
    expected: &mut Option<[u8; 32]>,
) -> Result<()> {
    let actual = sha256_path_exact(path, artifact_size)?;
    ensure_stable_value(&format!("{label} artifact identity"), expected, actual)
}

fn validate_restored_identity(
    label: &str,
    path: &Path,
    restored_size: u64,
    expected: &[u8; 32],
) -> Result<()> {
    let actual = sha256_path_exact(path, restored_size)?;
    if actual == *expected {
        Ok(())
    } else {
        Err(DatapackError::InvalidFormat(format!(
            "comparison {label} SHA256 validation failed"
        )))
    }
}

fn write_raw_datapack(input: &Path, output: &Path, input_size: u64) -> Result<()> {
    let mut reader = BufReader::with_capacity(IO_BUFFER_BYTES, File::open(input)?);
    let mut writer = BufWriter::with_capacity(IO_BUFFER_BYTES, File::create(output)?);
    storage::write_raw_zstd_archive_stream(input, input_size, &mut reader, &mut writer)?;
    writer.flush()?;
    Ok(())
}

fn measure_zstd_compression(
    input: &Path,
    output: &Path,
    input_size: u64,
) -> Result<(Duration, u64)> {
    let started = Instant::now();
    let mut reader = BufReader::with_capacity(IO_BUFFER_BYTES, File::open(input)?);
    let mut writer = BufWriter::with_capacity(IO_BUFFER_BYTES, File::create(output)?);
    let read =
        zstd_backend::compress_stream(&mut reader, &mut writer, zstd_backend::DEFAULT_LEVEL)?;
    if read != input_size {
        return Err(DatapackError::InvalidFormat(format!(
            "standalone zstd compression read {read} bytes, expected {input_size}"
        )));
    }
    writer.flush()?;
    let elapsed = started.elapsed();
    Ok((elapsed, std::fs::metadata(output)?.len()))
}

fn measure_datapack_decompression(
    input: &Path,
    output: &Path,
    expected_size: u64,
) -> Result<Duration> {
    let started = Instant::now();
    let mut reader = BufReader::with_capacity(IO_BUFFER_BYTES, File::open(input)?);
    let metadata = storage::read_v1_archive_header(&mut reader)?;
    let restored_size = if matches!(
        metadata.payload_kind,
        PayloadKind::RawZstd | PayloadKind::Plain | PayloadKind::Dictionary
    ) {
        let mut writer = BufWriter::with_capacity(IO_BUFFER_BYTES, File::create(output)?);
        let restored = storage::restore_raw_zstd_stream(&metadata, &mut reader, &mut writer)?;
        writer.flush()?;
        restored
    } else {
        drop(reader);
        let archive_size = std::fs::metadata(input)?.len();
        let archive_bytes = read_path_exact(input, archive_size)?;
        let archive = storage::decode_archive(&archive_bytes)?;
        let restored = storage::restore_archive(&archive)?;
        let restored_size = u64::try_from(restored.len()).map_err(|_| {
            DatapackError::InvalidFormat("DataPack restored size exceeds u64 capacity".to_string())
        })?;
        let mut writer = BufWriter::with_capacity(IO_BUFFER_BYTES, File::create(output)?);
        writer.write_all(&restored)?;
        writer.flush()?;
        restored_size
    };
    if restored_size != expected_size {
        return Err(DatapackError::InvalidFormat(format!(
            "DataPack restored {restored_size} bytes, expected {expected_size}"
        )));
    }
    Ok(started.elapsed())
}

fn measure_zstd_decompression(input: &Path, output: &Path, expected_size: u64) -> Result<Duration> {
    let started = Instant::now();
    let mut reader = BufReader::with_capacity(IO_BUFFER_BYTES, File::open(input)?);
    let mut writer = BufWriter::with_capacity(IO_BUFFER_BYTES, File::create(output)?);
    let restored = zstd_backend::decompress_stream_exact(&mut reader, &mut writer, expected_size)?;
    writer.flush()?;
    if restored != expected_size {
        return Err(DatapackError::InvalidFormat(format!(
            "standalone zstd restored {restored} bytes, expected {expected_size}"
        )));
    }
    Ok(started.elapsed())
}

fn materialize_snapshot(
    input: &Path,
    output: &Path,
    expected_size: u64,
    require_eof: bool,
) -> Result<()> {
    let mut reader = BufReader::with_capacity(IO_BUFFER_BYTES, File::open(input)?);
    let mut writer = BufWriter::with_capacity(IO_BUFFER_BYTES, File::create(output)?);
    let copied = std::io::copy(&mut reader.by_ref().take(expected_size), &mut writer)?;
    writer.flush()?;
    if copied != expected_size {
        return Err(DatapackError::InvalidFormat(format!(
            "comparison snapshot copied {copied} bytes, expected {expected_size}; input changed while preparing the comparison"
        )));
    }
    if require_eof {
        let mut extra = [0u8; 1];
        if reader.read(&mut extra)? != 0 {
            return Err(DatapackError::InvalidFormat(
                "comparison input grew while preparing the immutable snapshot".to_string(),
            ));
        }
    }
    Ok(())
}

fn read_path_exact(path: &Path, expected_size: u64) -> Result<Vec<u8>> {
    let capacity = usize::try_from(expected_size).map_err(|_| {
        DatapackError::InvalidFormat(
            "comparison artifact size exceeds platform capacity".to_string(),
        )
    })?;
    let read_limit = expected_size.checked_add(1).ok_or_else(|| {
        DatapackError::InvalidFormat("comparison artifact read limit overflowed".to_string())
    })?;
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(capacity).map_err(|error| {
        DatapackError::InvalidFormat(format!(
            "cannot reserve {capacity} bytes for comparison artifact: {error}"
        ))
    })?;
    File::open(path)?.take(read_limit).read_to_end(&mut bytes)?;
    let actual = u64::try_from(bytes.len()).map_err(|_| {
        DatapackError::InvalidFormat("comparison artifact length exceeds u64 capacity".to_string())
    })?;
    if actual != expected_size {
        return Err(DatapackError::InvalidFormat(format!(
            "comparison artifact size changed: expected {expected_size} bytes, got {actual}"
        )));
    }
    Ok(bytes)
}

fn sha256_path_exact(path: &Path, expected_size: u64) -> Result<[u8; 32]> {
    let mut reader = BufReader::with_capacity(IO_BUFFER_BYTES, File::open(path)?);
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    let mut total = 0u64;
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        total = total
            .checked_add(u64::try_from(read).map_err(|_| {
                DatapackError::InvalidFormat("comparison hash byte count overflowed".to_string())
            })?)
            .ok_or_else(|| {
                DatapackError::InvalidFormat("comparison hash byte count overflowed".to_string())
            })?;
        if total > expected_size {
            return Err(DatapackError::InvalidFormat(format!(
                "comparison hash input exceeded expected size of {expected_size} bytes"
            )));
        }
        hasher.update(&buffer[..read]);
    }
    if total != expected_size {
        return Err(DatapackError::InvalidFormat(format!(
            "comparison hash read {total} bytes, expected {expected_size}"
        )));
    }
    Ok(hasher.finalize().into())
}

const fn archive_mode_label(mode: ArchiveMode) -> &'static str {
    match mode {
        ArchiveMode::RawZstd => "raw_zstd",
        ArchiveMode::CsvColumnarDictionary => "csv_columnar_dictionary",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compare_options_reject_zero_and_excessive_runs() {
        for runs in [0, MAX_RUNS + 1] {
            let error = validate_options(CompareOptions {
                mode: ComparisonMode::Quick,
                runs,
                max_input_mb: None,
            })
            .unwrap_err();
            assert!(error.to_string().contains("--runs must be between"));
        }
    }

    #[test]
    fn full_mode_rejects_prefix_limit() {
        let error = validate_options(CompareOptions {
            mode: ComparisonMode::Full,
            runs: 3,
            max_input_mb: Some(1),
        })
        .unwrap_err();
        assert!(error.to_string().contains("only valid with --mode quick"));
    }

    #[test]
    fn quick_mode_uses_bounded_prefix() {
        let options = CompareOptions {
            mode: ComparisonMode::Quick,
            runs: 1,
            max_input_mb: Some(2),
        };
        assert_eq!(compared_size(3 * MIB, options).unwrap(), 2 * MIB);
    }

    #[test]
    fn artifact_identity_changes_are_rejected() {
        let mut expected = None;
        ensure_stable_value("test artifact identity", &mut expected, [1u8; 32]).unwrap();
        let error =
            ensure_stable_value("test artifact identity", &mut expected, [2u8; 32]).unwrap_err();
        assert!(error.to_string().contains("changed across runs"));
    }

    #[test]
    fn same_length_restoration_mismatch_is_rejected() {
        let directory = tempfile::tempdir().expect("restoration validation directory");
        let restored = directory.path().join("restored.bin");
        std::fs::write(&restored, b"wrong").expect("write mismatched restoration");
        let expected: [u8; 32] = Sha256::digest(b"right").into();

        let error = validate_restored_identity("test", &restored, 5, &expected).unwrap_err();
        assert!(error.to_string().contains("SHA256 validation failed"));
    }
}
