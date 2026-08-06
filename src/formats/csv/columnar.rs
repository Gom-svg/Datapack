use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::error::{DatapackError, Result};
use crate::formats::csv::{detect_delimiter, NewlineStyle};
use crate::formats::delimited::{
    parse_document, DelimitedDialect, DelimitedRecord, NewlinePolicy, ObservedNewline, QuoteMode,
    ScanError, ScanLimits,
};
use crate::planning::{ColumnExecutionMode, ColumnExecutionPlan, DictionaryExecutionLimits};

const MAGIC: &[u8; 6] = b"DCSV01";
const MIN_SAVINGS_RATIO: f64 = 0.05;
const SAFETY_SCAN_BYTES: usize = 1024 * 1024;
/// Standalone callers do not supply the archive's declared original size, so
/// keep their reconstruction bounded. Archive restoration uses
/// [`decode_with_output_limit`] with the exact declared output size instead.
pub const DEFAULT_DECODE_OUTPUT_LIMIT_BYTES: u64 = 1024 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CompactIdWidth {
    U8,
    U16,
    U32,
}

impl CompactIdWidth {
    pub fn bytes(self) -> usize {
        match self {
            Self::U8 => 1,
            Self::U16 => 2,
            Self::U32 => 4,
        }
    }

    fn to_byte(self) -> u8 {
        match self {
            Self::U8 => 1,
            Self::U16 => 2,
            Self::U32 => 4,
        }
    }

    fn from_byte(value: u8) -> Result<Self> {
        match value {
            1 => Ok(Self::U8),
            2 => Ok(Self::U16),
            4 => Ok(Self::U32),
            _ => Err(DatapackError::InvalidFormat(format!(
                "invalid compact id width {value}"
            ))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ColumnMode {
    Plain,
    Dictionary,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CsvSafety {
    Simple,
    RequiresRfc4180,
    Unsupported(String),
}

pub struct CsvSafetyScanner;

impl CsvSafetyScanner {
    pub fn scan(bytes: &[u8], delimiter: u8) -> CsvSafety {
        let mut sample_len = bytes.len().min(SAFETY_SCAN_BYTES);
        if sample_len < bytes.len() {
            sample_len = bytes[..sample_len]
                .iter()
                .rposition(|byte| *byte == b'\n')
                .map(|index| index + 1)
                .unwrap_or(sample_len);
        }
        let sample = &bytes[..sample_len];
        if sample.contains(&b'"') {
            return match parse_rfc4180_rows(sample, delimiter) {
                Ok(parsed) if has_stable_columns(&parsed.rows) => CsvSafety::RequiresRfc4180,
                Ok(_) => CsvSafety::Unsupported("csv column count is not stable".to_string()),
                Err(error) => CsvSafety::Unsupported(error.to_string()),
            };
        }

        match parse_simple_rows(sample, delimiter) {
            Ok(parsed) if has_stable_columns(&parsed.rows) => CsvSafety::Simple,
            Ok(_) => CsvSafety::Unsupported("csv column count is not stable".to_string()),
            Err(error) => CsvSafety::Unsupported(error.to_string()),
        }
    }
}

pub fn compact_id_width(unique_values: usize) -> CompactIdWidth {
    if unique_values <= u8::MAX as usize {
        CompactIdWidth::U8
    } else if unique_values <= u16::MAX as usize {
        CompactIdWidth::U16
    } else {
        CompactIdWidth::U32
    }
}

pub fn encode(bytes: &[u8]) -> Result<Option<Vec<u8>>> {
    let text = String::from_utf8_lossy(bytes);
    let delimiter = detect_delimiter(&text) as u8;
    encode_with_delimiter(bytes, delimiter)
}

pub(crate) fn encode_with_delimiter(bytes: &[u8], delimiter: u8) -> Result<Option<Vec<u8>>> {
    encode_internal(bytes, delimiter, None)
}

pub(crate) fn encode_with_execution_plan(
    bytes: &[u8],
    delimiter: u8,
    execution_plan: &ColumnExecutionPlan,
) -> Result<Option<Vec<u8>>> {
    encode_internal(bytes, delimiter, Some(execution_plan))
}

fn encode_internal(
    bytes: &[u8],
    delimiter: u8,
    execution_plan: Option<&ColumnExecutionPlan>,
) -> Result<Option<Vec<u8>>> {
    if !matches!(delimiter, b',' | b';' | b'\t' | b'|') {
        return Err(DatapackError::InvalidFormat(format!(
            "unsupported csv payload delimiter byte {delimiter}"
        )));
    }
    let shape = CsvShape::parse(bytes, delimiter)?;
    if let Some(plan) = execution_plan {
        plan.validate_column_count(shape.column_count)?;
    }
    let mut output = Vec::new();

    output.extend_from_slice(MAGIC);
    output.push(delimiter);
    output.push(match shape.newline_style {
        NewlineStyle::Lf => 1,
        NewlineStyle::Crlf => 2,
    });
    output.push(u8::from(shape.has_final_newline));
    let row_count = u64::try_from(shape.row_count).map_err(|_| {
        DatapackError::InvalidFormat("csv row count exceeds u64 capacity".to_string())
    })?;
    let column_count = u32::try_from(shape.column_count).map_err(|_| {
        DatapackError::InvalidFormat("csv column count exceeds u32 capacity".to_string())
    })?;
    write_u64(&mut output, row_count);
    write_u32(&mut output, column_count);

    for column_index in 0..shape.column_count {
        let values = shape.column_values(column_index);
        if let Some(plan) = execution_plan {
            match plan.mode(column_index)? {
                ColumnExecutionMode::Plain => {
                    write_planned_plain_column(&mut output, &values)?;
                }
                ColumnExecutionMode::Dictionary => write_planned_dictionary_column(
                    &mut output,
                    &values,
                    plan.dictionary_limits(),
                    column_index,
                )?,
            }
        } else {
            match choose_column_mode(&values) {
                ColumnMode::Plain => write_plain_column(&mut output, &values),
                ColumnMode::Dictionary => write_dictionary_column(&mut output, &values),
            }
        }
    }

    let validation_limit = u64::try_from(bytes.len()).map_err(|_| {
        DatapackError::InvalidFormat(
            "csv input size exceeds the supported u64 capacity".to_string(),
        )
    })?;
    let restored = decode_with_output_limit(&output, validation_limit)?;
    if restored == bytes {
        Ok(Some(output))
    } else {
        Ok(None)
    }
}

pub fn decode(bytes: &[u8]) -> Result<Vec<u8>> {
    decode_with_output_limit(bytes, DEFAULT_DECODE_OUTPUT_LIMIT_BYTES)
}

/// Decodes a v1 columnar CSV reconstruction payload without permitting output
/// amplification beyond `max_output_size`.
pub fn decode_with_output_limit(bytes: &[u8], max_output_size: u64) -> Result<Vec<u8>> {
    let mut cursor = Cursor::new(bytes);
    cursor.consume_expected(MAGIC)?;
    let delimiter = cursor.read_u8()?;
    if !matches!(delimiter, b',' | b';' | b'\t' | b'|') {
        return Err(DatapackError::InvalidFormat(format!(
            "invalid csv payload delimiter byte {delimiter}"
        )));
    }
    let newline_style = match cursor.read_u8()? {
        1 => NewlineStyle::Lf,
        2 => NewlineStyle::Crlf,
        value => {
            return Err(DatapackError::InvalidFormat(format!(
                "invalid newline style {value}"
            )))
        }
    };
    let has_final_newline = match cursor.read_u8()? {
        0 => false,
        1 => true,
        value => {
            return Err(DatapackError::InvalidFormat(format!(
                "invalid final-newline marker {value}; expected 0 or 1"
            )))
        }
    };
    let row_count_u64 = cursor.read_u64()?;
    let row_count = usize::try_from(row_count_u64).map_err(|_| {
        DatapackError::InvalidFormat(format!(
            "csv row count {row_count_u64} exceeds platform capacity"
        ))
    })?;
    let column_count_u32 = cursor.read_u32()?;
    let column_count = usize::try_from(column_count_u32).map_err(|_| {
        DatapackError::InvalidFormat(format!(
            "csv column count {column_count_u32} exceeds platform capacity"
        ))
    })?;
    if row_count == 0 || column_count == 0 {
        return Err(DatapackError::InvalidFormat(
            "csv payload must contain at least one row and one column".to_string(),
        ));
    }
    let cell_count = row_count.checked_mul(column_count).ok_or_else(|| {
        DatapackError::InvalidFormat("csv row_count * column_count overflows usize".to_string())
    })?;
    let remaining_payload_bytes = cursor.remaining()?;
    if cell_count > remaining_payload_bytes {
        return Err(DatapackError::InvalidFormat(format!(
            "csv payload declares {cell_count} cells but only {} encoded bytes remain",
            remaining_payload_bytes
        )));
    }
    let minimum_output_cells = cell_count
        .checked_sub(1)
        .ok_or_else(|| DatapackError::InvalidFormat("csv cell count underflow".to_string()))?;
    let minimum_output_size = u64::try_from(minimum_output_cells).map_err(|_| {
        DatapackError::InvalidFormat("minimum csv output size exceeds u64 capacity".to_string())
    })?;
    if minimum_output_size > max_output_size {
        return Err(DatapackError::InvalidFormat(format!(
            "csv reconstruction exceeds output limit of {max_output_size} bytes"
        )));
    }

    let mut columns = try_vec_with_capacity(column_count, "csv columns")?;

    for _ in 0..column_count {
        columns.push(read_column(&mut cursor, row_count)?);
    }
    cursor.finish()?;

    let newline = match newline_style {
        NewlineStyle::Lf => b"\n".as_slice(),
        NewlineStyle::Crlf => b"\r\n".as_slice(),
    };
    let output_size = reconstructed_size(
        &columns,
        row_count,
        column_count,
        newline.len(),
        has_final_newline,
    )?;
    if output_size > max_output_size {
        return Err(DatapackError::InvalidFormat(format!(
            "csv reconstruction requires {output_size} bytes, exceeding output limit of {max_output_size} bytes"
        )));
    }
    let output_capacity = usize::try_from(output_size).map_err(|_| {
        DatapackError::InvalidFormat(format!(
            "csv reconstruction size {output_size} exceeds platform capacity"
        ))
    })?;
    let mut output = Vec::new();
    output.try_reserve_exact(output_capacity).map_err(|error| {
        DatapackError::InvalidFormat(format!(
            "cannot reserve {output_capacity} bytes for csv reconstruction: {error}"
        ))
    })?;
    for row_index in 0..row_count {
        if row_index > 0 {
            output.extend_from_slice(newline);
        }
        for (column_index, column) in columns.iter().enumerate() {
            if column_index > 0 {
                output.push(delimiter);
            }
            output.extend_from_slice(column.value(row_index)?);
        }
    }
    if has_final_newline {
        output.extend_from_slice(newline);
    }
    if output.len() != output_capacity {
        return Err(DatapackError::InvalidFormat(format!(
            "csv reconstruction size changed during decode: expected {output_capacity}, got {}",
            output.len()
        )));
    }
    Ok(output)
}

fn choose_column_mode(values: &[&[u8]]) -> ColumnMode {
    let raw_size = plain_column_size(values);
    let (dictionary_size, _) = dictionary_column_size(values);
    let threshold_size = (raw_size as f64 * (1.0 - MIN_SAVINGS_RATIO)) as usize;

    if dictionary_size < threshold_size {
        ColumnMode::Dictionary
    } else {
        ColumnMode::Plain
    }
}

fn plain_column_size(values: &[&[u8]]) -> usize {
    1 + values.iter().map(|value| 4 + value.len()).sum::<usize>()
}

fn dictionary_column_size(values: &[&[u8]]) -> (usize, CompactIdWidth) {
    let mut unique = HashMap::<Vec<u8>, ()>::new();
    let mut dictionary_bytes = 0usize;
    for value in values {
        if !unique.contains_key(*value) {
            dictionary_bytes += 4 + value.len();
            unique.insert((*value).to_vec(), ());
        }
    }

    let width = compact_id_width(unique.len());
    let size = 1 + 1 + 4 + dictionary_bytes + values.len() * width.bytes();
    (size, width)
}

fn write_plain_column(output: &mut Vec<u8>, values: &[&[u8]]) {
    output.push(0);
    for value in values {
        write_u32(output, value.len() as u32);
        output.extend_from_slice(value);
    }
}

fn write_dictionary_column(output: &mut Vec<u8>, values: &[&[u8]]) {
    let mut codes = Vec::with_capacity(values.len());
    let mut code_by_value = HashMap::<Vec<u8>, u32>::new();
    let mut dictionary = Vec::<Vec<u8>>::new();

    for value in values {
        let code = match code_by_value.get(*value) {
            Some(code) => *code,
            None => {
                let code = dictionary.len() as u32;
                dictionary.push((*value).to_vec());
                code_by_value.insert((*value).to_vec(), code);
                code
            }
        };
        codes.push(code);
    }

    let width = compact_id_width(dictionary.len());
    output.push(1);
    output.push(width.to_byte());
    write_u32(output, dictionary.len() as u32);
    for value in &dictionary {
        write_u32(output, value.len() as u32);
        output.extend_from_slice(value);
    }
    for code in codes {
        match width {
            CompactIdWidth::U8 => output.push(code as u8),
            CompactIdWidth::U16 => output.extend_from_slice(&(code as u16).to_le_bytes()),
            CompactIdWidth::U32 => output.extend_from_slice(&code.to_le_bytes()),
        }
    }
}

fn write_planned_plain_column(output: &mut Vec<u8>, values: &[&[u8]]) -> Result<()> {
    output.push(0);
    for value in values {
        let value_len = u32::try_from(value.len()).map_err(|_| {
            DatapackError::InvalidFormat(
                "planned plain-column value length exceeds u32 capacity".to_string(),
            )
        })?;
        write_u32(output, value_len);
        output.extend_from_slice(value);
    }
    Ok(())
}

fn write_planned_dictionary_column(
    output: &mut Vec<u8>,
    values: &[&[u8]],
    limits: DictionaryExecutionLimits,
    column_index: usize,
) -> Result<()> {
    let capacity = values
        .len()
        .min(usize::try_from(limits.max_values).unwrap_or(usize::MAX));
    let mut codes = Vec::new();
    codes.try_reserve_exact(values.len()).map_err(|error| {
        DatapackError::InvalidFormat(format!(
            "cannot reserve dictionary codes for column {column_index}: {error}"
        ))
    })?;
    let mut code_by_value = HashMap::<&[u8], u32>::new();
    code_by_value.try_reserve(capacity).map_err(|error| {
        DatapackError::InvalidFormat(format!(
            "cannot reserve dictionary map for column {column_index}: {error}"
        ))
    })?;
    let mut dictionary = Vec::<&[u8]>::new();
    dictionary.try_reserve_exact(capacity).map_err(|error| {
        DatapackError::InvalidFormat(format!(
            "cannot reserve dictionary values for column {column_index}: {error}"
        ))
    })?;
    let mut dictionary_bytes = 0u64;

    for value in values {
        let code = match code_by_value.get(*value) {
            Some(code) => *code,
            None => {
                let next_count = u64::try_from(dictionary.len())
                    .ok()
                    .and_then(|count| count.checked_add(1))
                    .ok_or_else(|| {
                        DatapackError::InvalidFormat(format!(
                            "dictionary value count overflowed for column {column_index}"
                        ))
                    })?;
                if next_count > limits.max_values {
                    return Err(DatapackError::InvalidFormat(format!(
                        "column {column_index} dictionary requires more than {} values",
                        limits.max_values
                    )));
                }
                let value_bytes = u64::try_from(value.len()).map_err(|_| {
                    DatapackError::InvalidFormat(format!(
                        "dictionary value length exceeds u64 capacity for column {column_index}"
                    ))
                })?;
                let entry_bytes = value_bytes.checked_add(4).ok_or_else(|| {
                    DatapackError::InvalidFormat(format!(
                        "dictionary byte count overflowed for column {column_index}"
                    ))
                })?;
                let next_bytes = dictionary_bytes.checked_add(entry_bytes).ok_or_else(|| {
                    DatapackError::InvalidFormat(format!(
                        "dictionary byte count overflowed for column {column_index}"
                    ))
                })?;
                if next_bytes > limits.max_bytes {
                    return Err(DatapackError::InvalidFormat(format!(
                        "column {column_index} dictionary requires more than {} bytes",
                        limits.max_bytes
                    )));
                }
                let code = u32::try_from(dictionary.len()).map_err(|_| {
                    DatapackError::InvalidFormat(format!(
                        "dictionary value count exceeds u32 capacity for column {column_index}"
                    ))
                })?;
                dictionary.push(*value);
                code_by_value.insert(*value, code);
                dictionary_bytes = next_bytes;
                code
            }
        };
        codes.push(code);
    }

    let width = compact_id_width(dictionary.len());
    output.push(1);
    output.push(width.to_byte());
    let dictionary_len = u32::try_from(dictionary.len()).map_err(|_| {
        DatapackError::InvalidFormat(format!(
            "dictionary value count exceeds u32 capacity for column {column_index}"
        ))
    })?;
    write_u32(output, dictionary_len);
    for value in &dictionary {
        let value_len = u32::try_from(value.len()).map_err(|_| {
            DatapackError::InvalidFormat(format!(
                "dictionary value length exceeds u32 capacity for column {column_index}"
            ))
        })?;
        write_u32(output, value_len);
        output.extend_from_slice(value);
    }
    for code in codes {
        match width {
            CompactIdWidth::U8 => output.push(u8::try_from(code).map_err(|_| {
                DatapackError::InvalidFormat(format!(
                    "dictionary code {code} exceeds u8 capacity for column {column_index}"
                ))
            })?),
            CompactIdWidth::U16 => {
                let code = u16::try_from(code).map_err(|_| {
                    DatapackError::InvalidFormat(format!(
                        "dictionary code {code} exceeds u16 capacity for column {column_index}"
                    ))
                })?;
                output.extend_from_slice(&code.to_le_bytes());
            }
            CompactIdWidth::U32 => output.extend_from_slice(&code.to_le_bytes()),
        }
    }
    Ok(())
}

enum DecodedColumn<'a> {
    Plain(Vec<&'a [u8]>),
    Dictionary {
        values: Vec<&'a [u8]>,
        codes: Vec<u32>,
    },
}

impl DecodedColumn<'_> {
    fn value(&self, row_index: usize) -> Result<&[u8]> {
        match self {
            Self::Plain(values) => values.get(row_index).copied().ok_or_else(|| {
                DatapackError::InvalidFormat(format!(
                    "csv row index {row_index} is missing from a plain column"
                ))
            }),
            Self::Dictionary { values, codes } => {
                let code = *codes.get(row_index).ok_or_else(|| {
                    DatapackError::InvalidFormat(format!(
                        "csv row index {row_index} is missing from a dictionary column"
                    ))
                })?;
                let code_index = usize::try_from(code).map_err(|_| {
                    DatapackError::InvalidFormat(format!(
                        "dictionary code {code} exceeds platform capacity"
                    ))
                })?;
                values.get(code_index).copied().ok_or_else(|| {
                    DatapackError::InvalidFormat(format!("dictionary code {code} is out of range"))
                })
            }
        }
    }
}

fn read_column<'a>(cursor: &mut Cursor<'a>, row_count: usize) -> Result<DecodedColumn<'a>> {
    match cursor.read_u8()? {
        0 => {
            let minimum_bytes = row_count.checked_mul(4).ok_or_else(|| {
                DatapackError::InvalidFormat(
                    "plain column row count overflows its minimum encoded size".to_string(),
                )
            })?;
            let remaining = cursor.remaining()?;
            if minimum_bytes > remaining {
                return Err(DatapackError::InvalidFormat(format!(
                    "plain column requires at least {minimum_bytes} bytes for {row_count} rows, but only {} remain",
                    remaining
                )));
            }
            let mut values = try_vec_with_capacity(row_count, "plain column values")?;
            for _ in 0..row_count {
                values.push(cursor.read_len_prefixed()?);
            }
            Ok(DecodedColumn::Plain(values))
        }
        1 => {
            let width = CompactIdWidth::from_byte(cursor.read_u8()?)?;
            let dictionary_len_u32 = cursor.read_u32()?;
            let dictionary_len = usize::try_from(dictionary_len_u32).map_err(|_| {
                DatapackError::InvalidFormat(format!(
                    "dictionary length {dictionary_len_u32} exceeds platform capacity"
                ))
            })?;
            let minimum_dictionary_bytes = dictionary_len.checked_mul(4).ok_or_else(|| {
                DatapackError::InvalidFormat(
                    "dictionary length overflows its minimum encoded size".to_string(),
                )
            })?;
            let code_bytes = row_count.checked_mul(width.bytes()).ok_or_else(|| {
                DatapackError::InvalidFormat(
                    "dictionary row count overflows encoded code size".to_string(),
                )
            })?;
            let minimum_remaining = minimum_dictionary_bytes
                .checked_add(code_bytes)
                .ok_or_else(|| {
                    DatapackError::InvalidFormat(
                        "dictionary metadata and code sizes overflow usize".to_string(),
                    )
                })?;
            let remaining = cursor.remaining()?;
            if minimum_remaining > remaining {
                return Err(DatapackError::InvalidFormat(format!(
                    "dictionary column requires at least {minimum_remaining} bytes, but only {} remain",
                    remaining
                )));
            }
            let mut dictionary = try_vec_with_capacity(dictionary_len, "dictionary values")?;
            for _ in 0..dictionary_len {
                dictionary.push(cursor.read_len_prefixed()?);
            }

            let remaining = cursor.remaining()?;
            if code_bytes > remaining {
                return Err(DatapackError::InvalidFormat(format!(
                    "dictionary codes require {code_bytes} bytes, but only {} remain",
                    remaining
                )));
            }
            let mut codes = try_vec_with_capacity(row_count, "dictionary codes")?;
            for _ in 0..row_count {
                let code = match width {
                    CompactIdWidth::U8 => u32::from(cursor.read_u8()?),
                    CompactIdWidth::U16 => u32::from(cursor.read_u16()?),
                    CompactIdWidth::U32 => cursor.read_u32()?,
                };
                let code_index = usize::try_from(code).map_err(|_| {
                    DatapackError::InvalidFormat(format!(
                        "dictionary code {code} exceeds platform capacity"
                    ))
                })?;
                dictionary.get(code_index).ok_or_else(|| {
                    DatapackError::InvalidFormat(format!("dictionary code {code} out of range"))
                })?;
                codes.push(code);
            }
            Ok(DecodedColumn::Dictionary {
                values: dictionary,
                codes,
            })
        }
        value => Err(DatapackError::InvalidFormat(format!(
            "invalid column mode {value}"
        ))),
    }
}

fn reconstructed_size(
    columns: &[DecodedColumn<'_>],
    row_count: usize,
    column_count: usize,
    newline_len: usize,
    has_final_newline: bool,
) -> Result<u64> {
    let mut size = 0u64;
    for row_index in 0..row_count {
        for column in columns {
            let value_len = u64::try_from(column.value(row_index)?.len()).map_err(|_| {
                DatapackError::InvalidFormat("csv field length exceeds u64 capacity".to_string())
            })?;
            size = size.checked_add(value_len).ok_or_else(|| {
                DatapackError::InvalidFormat(
                    "csv reconstructed field sizes exceed u64 capacity".to_string(),
                )
            })?;
        }
    }

    let delimiter_count = row_count
        .checked_mul(column_count.checked_sub(1).ok_or_else(|| {
            DatapackError::InvalidFormat("csv column count underflow".to_string())
        })?)
        .ok_or_else(|| {
            DatapackError::InvalidFormat("csv delimiter count exceeds usize capacity".to_string())
        })?;
    let inter_row_newlines = row_count
        .checked_sub(1)
        .ok_or_else(|| DatapackError::InvalidFormat("csv row count underflow".to_string()))?;
    let newline_count = inter_row_newlines
        .checked_add(usize::from(has_final_newline))
        .ok_or_else(|| {
            DatapackError::InvalidFormat("csv newline count exceeds usize capacity".to_string())
        })?;
    let newline_bytes = newline_count.checked_mul(newline_len).ok_or_else(|| {
        DatapackError::InvalidFormat("csv newline bytes exceed usize capacity".to_string())
    })?;
    let separator_bytes = delimiter_count.checked_add(newline_bytes).ok_or_else(|| {
        DatapackError::InvalidFormat("csv separator bytes exceed usize capacity".to_string())
    })?;
    let separator_bytes_u64 = u64::try_from(separator_bytes).map_err(|_| {
        DatapackError::InvalidFormat("csv separator bytes exceed u64 capacity".to_string())
    })?;
    size.checked_add(separator_bytes_u64).ok_or_else(|| {
        DatapackError::InvalidFormat("csv reconstructed size exceeds u64 capacity".to_string())
    })
}

fn try_vec_with_capacity<T>(capacity: usize, purpose: &str) -> Result<Vec<T>> {
    let mut values = Vec::new();
    values.try_reserve_exact(capacity).map_err(|error| {
        DatapackError::InvalidFormat(format!(
            "cannot reserve capacity for {purpose} ({capacity} entries): {error}"
        ))
    })?;
    Ok(values)
}

struct CsvShape<'a> {
    bytes: &'a [u8],
    rows: Vec<DelimitedRecord>,
    row_count: usize,
    column_count: usize,
    newline_style: NewlineStyle,
    has_final_newline: bool,
}

impl<'a> CsvShape<'a> {
    fn parse(bytes: &'a [u8], delimiter: u8) -> Result<Self> {
        let ParsedCsv {
            rows,
            newline_style,
            has_final_newline,
        } = match CsvSafetyScanner::scan(bytes, delimiter) {
            CsvSafety::Simple => match parse_simple_rows(bytes, delimiter) {
                Ok(parsed) => parsed,
                Err(_) => parse_rfc4180_rows(bytes, delimiter)?,
            },
            CsvSafety::RequiresRfc4180 => parse_rfc4180_rows(bytes, delimiter)?,
            CsvSafety::Unsupported(reason) => return Err(DatapackError::InvalidFormat(reason)),
        };

        if rows.is_empty() {
            return Err(DatapackError::InvalidFormat("empty csv input".to_string()));
        }
        let column_count = rows[0].fields().len();
        if column_count == 0 || rows.iter().any(|row| row.fields().len() != column_count) {
            return Err(DatapackError::InvalidFormat(
                "csv column count is not stable".to_string(),
            ));
        }

        Ok(Self {
            bytes,
            row_count: rows.len(),
            column_count,
            rows,
            newline_style,
            has_final_newline,
        })
    }

    fn column_values(&self, column_index: usize) -> Vec<&'a [u8]> {
        self.rows
            .iter()
            .map(|row| &self.bytes[row.fields()[column_index].clone()])
            .collect()
    }
}

fn has_stable_columns(rows: &[DelimitedRecord]) -> bool {
    let Some(first) = rows.first() else {
        return false;
    };
    !first.fields().is_empty()
        && rows
            .iter()
            .all(|row| row.fields().len() == first.fields().len())
}

struct ParsedCsv {
    rows: Vec<DelimitedRecord>,
    newline_style: NewlineStyle,
    has_final_newline: bool,
}

fn parse_simple_rows(bytes: &[u8], delimiter: u8) -> Result<ParsedCsv> {
    if bytes.contains(&b'"') {
        return Err(DatapackError::InvalidFormat(
            "simple csv path does not support quotes".to_string(),
        ));
    }
    parse_delimited_rows(bytes, delimiter, QuoteMode::Disabled)
}

fn parse_rfc4180_rows(bytes: &[u8], delimiter: u8) -> Result<ParsedCsv> {
    parse_delimited_rows(bytes, delimiter, QuoteMode::Dcsv01Compatible)
}

fn parse_delimited_rows(bytes: &[u8], delimiter: u8, quote_mode: QuoteMode) -> Result<ParsedCsv> {
    let document = parse_document(
        bytes,
        DelimitedDialect::new(delimiter, quote_mode, NewlinePolicy::RequireConsistent),
        ScanLimits::resident_input(bytes.len()),
    )
    .map_err(map_scan_error)?;
    let newline_style = match document.newline() {
        ObservedNewline::None | ObservedNewline::Lf => NewlineStyle::Lf,
        ObservedNewline::Crlf => NewlineStyle::Crlf,
        ObservedNewline::Mixed => {
            return Err(DatapackError::InvalidFormat(
                "mixed csv newline styles are unsupported".to_string(),
            ))
        }
    };
    let has_final_newline = document.has_final_newline();
    let mut rows = document.into_records();
    if rows.is_empty() {
        // The established DCSV01 parser represents empty input as one row
        // containing one empty field. Keep that codec-only convention out of
        // the canonical scanner while preserving the public adapter exactly.
        rows.try_reserve(1).map_err(|error| {
            DatapackError::InvalidFormat(format!(
                "cannot reserve capacity for empty csv row (1 entry): {error}"
            ))
        })?;
        let mut empty_fields = try_vec_with_capacity(1, "empty csv row fields")?;
        empty_fields.push(0..0);
        rows.push(DelimitedRecord::from_fields(empty_fields));
    }
    Ok(ParsedCsv {
        rows,
        newline_style,
        has_final_newline,
    })
}

fn map_scan_error(error: ScanError) -> DatapackError {
    let reason = match error {
        ScanError::QuoteNotAllowed => "simple csv path does not support quotes".to_string(),
        ScanError::UnterminatedQuotedField => "unterminated quoted csv field".to_string(),
        ScanError::BareCarriageReturn => "unsupported bare CR newline in csv".to_string(),
        ScanError::MixedNewlines => "mixed csv newline styles are unsupported".to_string(),
        other => other.to_string(),
    };
    DatapackError::InvalidFormat(reason)
}

fn write_u32(output: &mut Vec<u8>, value: u32) {
    output.extend_from_slice(&value.to_le_bytes());
}

fn write_u64(output: &mut Vec<u8>, value: u64) {
    output.extend_from_slice(&value.to_le_bytes());
}

struct Cursor<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Cursor<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn consume_expected(&mut self, expected: &[u8]) -> Result<()> {
        let actual = self.read_exact(expected.len())?;
        if actual == expected {
            Ok(())
        } else {
            Err(DatapackError::InvalidFormat(
                "bad csv payload magic".to_string(),
            ))
        }
    }

    fn read_u8(&mut self) -> Result<u8> {
        self.read_exact(1)?.first().copied().ok_or_else(|| {
            DatapackError::InvalidFormat("unexpected end of csv payload".to_string())
        })
    }

    fn read_u16(&mut self) -> Result<u16> {
        let mut value = [0u8; 2];
        value.copy_from_slice(self.read_exact(2)?);
        Ok(u16::from_le_bytes(value))
    }

    fn read_u32(&mut self) -> Result<u32> {
        let mut value = [0u8; 4];
        value.copy_from_slice(self.read_exact(4)?);
        Ok(u32::from_le_bytes(value))
    }

    fn read_u64(&mut self) -> Result<u64> {
        let mut value = [0u8; 8];
        value.copy_from_slice(self.read_exact(8)?);
        Ok(u64::from_le_bytes(value))
    }

    fn read_len_prefixed(&mut self) -> Result<&'a [u8]> {
        let len_u32 = self.read_u32()?;
        let len = usize::try_from(len_u32).map_err(|_| {
            DatapackError::InvalidFormat(format!(
                "csv field length {len_u32} exceeds platform capacity"
            ))
        })?;
        self.read_exact(len)
    }

    fn read_exact(&mut self, len: usize) -> Result<&'a [u8]> {
        let end = self.offset.checked_add(len).ok_or_else(|| {
            DatapackError::InvalidFormat("csv payload offset overflow".to_string())
        })?;
        let value = self.bytes.get(self.offset..end).ok_or_else(|| {
            DatapackError::InvalidFormat(format!(
                "unexpected end of csv payload at offset {} while reading {len} bytes",
                self.offset
            ))
        })?;
        self.offset = end;
        Ok(value)
    }

    fn remaining(&self) -> Result<usize> {
        self.bytes.len().checked_sub(self.offset).ok_or_else(|| {
            DatapackError::InvalidFormat(format!(
                "csv cursor offset {} exceeds payload length {}",
                self.offset,
                self.bytes.len()
            ))
        })
    }

    fn finish(&self) -> Result<()> {
        if self.offset != self.bytes.len() {
            return Err(DatapackError::InvalidFormat(format!(
                "csv payload has {} trailing bytes",
                self.remaining()?
            )));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::planning::{ArchiveMode, ColumnPlan, ColumnStrategy, CompressionPlan};

    const FIRST_COLUMN_MODE_OFFSET: usize = 6 + 1 + 1 + 1 + 8 + 4;

    fn execution_plan(
        strategies: &[ColumnStrategy],
        max_values: u64,
        max_bytes: u64,
    ) -> ColumnExecutionPlan {
        let plan = CompressionPlan {
            archive_mode: ArchiveMode::CsvColumnarDictionary,
            columns: strategies
                .iter()
                .copied()
                .enumerate()
                .map(|(column_index, strategy)| ColumnPlan {
                    column_index,
                    column_name: format!("column_{column_index}"),
                    strategy,
                    reason: "codec execution test".to_string(),
                })
                .collect(),
            estimated_savings_percent: 10.0,
            estimated_memory_mb: 1.0,
            planning_time_ms: 0,
            reason: "codec execution test".to_string(),
        };
        ColumnExecutionPlan::with_dictionary_limit_bytes(&plan, max_values, max_bytes)
    }

    #[test]
    fn compact_id_width_selection() {
        assert_eq!(compact_id_width(255), CompactIdWidth::U8);
        assert_eq!(compact_id_width(256), CompactIdWidth::U16);
        assert_eq!(compact_id_width(65_535), CompactIdWidth::U16);
        assert_eq!(compact_id_width(65_536), CompactIdWidth::U32);
    }

    #[test]
    fn columnar_csv_round_trip() {
        let input = b"id,status,region\r\n1,open,north\r\n2,open,north\r\n3,closed,south\r\n";
        let encoded = encode(input).unwrap().unwrap();
        let restored = decode(&encoded).unwrap();

        assert_eq!(restored, input);
    }

    #[test]
    fn quoted_comma_field_round_trip() {
        let input = b"id,note\r\n1,\"hello, world\"\r\n2,\"north, south\"\r\n";
        let encoded = encode(input).unwrap().unwrap();
        let restored = decode(&encoded).unwrap();

        assert_eq!(restored, input);
    }

    #[test]
    fn escaped_quotes_round_trip() {
        let input = b"id,note\r\n1,\"said \"\"hello\"\" today\"\r\n2,\"plain\"\r\n";
        let encoded = encode(input).unwrap().unwrap();
        let restored = decode(&encoded).unwrap();

        assert_eq!(restored, input);
    }

    #[test]
    fn empty_fields_round_trip() {
        let input = b"id,a,b,c\r\n1,,\"\",x\r\n2,y,,\r\n";
        let encoded = encode(input).unwrap().unwrap();
        let restored = decode(&encoded).unwrap();

        assert_eq!(restored, input);
    }

    #[test]
    fn lf_round_trip() {
        let input = b"id,note\n1,\"hello, world\"\n2,plain\n";
        let encoded = encode(input).unwrap().unwrap();
        let restored = decode(&encoded).unwrap();

        assert_eq!(restored, input);
    }

    #[test]
    fn utf8_round_trip() {
        let input = "id,note\r\n1,\"café, mañana\"\r\n2,\"東京\"\r\n".as_bytes();
        let encoded = encode(input).unwrap().unwrap();
        let restored = decode(&encoded).unwrap();

        assert_eq!(restored, input);
    }

    #[test]
    fn spaces_preserved_round_trip() {
        let input = b"id,note\r\n1,  padded value  \r\n2,\" spaced, quoted \"\r\n";
        let encoded = encode(input).unwrap().unwrap();
        let restored = decode(&encoded).unwrap();

        assert_eq!(restored, input);
    }

    #[test]
    fn mixed_quoted_unquoted_round_trip() {
        let input =
            b"id,a,b,c\r\n1,plain,\"quoted, comma\",tail\r\n2,\"x\",unquoted,\"y\"\"z\"\r\n";
        let encoded = encode(input).unwrap().unwrap();
        let restored = decode(&encoded).unwrap();

        assert_eq!(restored, input);
    }

    #[test]
    fn explicit_delimiter_adapter_round_trips_rich_dcsv01_matrix() {
        for delimiter in [b',', b';', b'\t', b'|'] {
            for (newline, final_newline) in [(b"\n".as_slice(), false), (b"\r\n", true)] {
                let mut input = Vec::new();
                push_delimited_fields(
                    &mut input,
                    delimiter,
                    [
                        b"id".as_slice(),
                        b"code",
                        b"note",
                        b"city",
                        b"empty",
                        b"space",
                    ],
                );
                input.extend_from_slice(newline);

                push_delimited_fields(&mut input, delimiter, [b"001".as_slice(), b"00042"]);
                input.push(delimiter);
                input.extend_from_slice(b"\"  left");
                input.push(delimiter);
                input.extend_from_slice(b"right");
                input.extend_from_slice(newline);
                input.extend_from_slice(b"continued;|,\t  \"");
                input.push(delimiter);
                input.extend_from_slice("東京".as_bytes());
                input.push(delimiter);
                input.push(delimiter);
                input.extend_from_slice(b"  padded  ");
                input.extend_from_slice(newline);

                push_delimited_fields(
                    &mut input,
                    delimiter,
                    [
                        b"002".as_slice(),
                        b"00007",
                        b"\"said \"\"hello\"\"\"",
                        "café".as_bytes(),
                        b"\"\"",
                        b"\" spaced \"",
                    ],
                );
                if final_newline {
                    input.extend_from_slice(newline);
                }

                assert_eq!(
                    CsvSafetyScanner::scan(&input, delimiter),
                    CsvSafety::RequiresRfc4180
                );

                let encoded = encode_with_delimiter(&input, delimiter).unwrap().unwrap();
                assert_eq!(&encoded[..MAGIC.len()], MAGIC);
                assert_eq!(encoded.get(MAGIC.len()), Some(&delimiter));
                assert_eq!(encoded.get(MAGIC.len() + 2), Some(&u8::from(final_newline)));
                assert_eq!(decode(&encoded).unwrap(), input);
            }
        }
    }

    fn push_delimited_fields<const N: usize>(
        output: &mut Vec<u8>,
        delimiter: u8,
        fields: [&[u8]; N],
    ) {
        for (index, field) in fields.into_iter().enumerate() {
            if index > 0 {
                output.push(delimiter);
            }
            output.extend_from_slice(field);
        }
    }

    #[test]
    fn compatibility_adapter_preserves_invalid_utf8_bytes() {
        let input = b"id,value\n1,\xff\n2,\xfe";
        assert_eq!(CsvSafetyScanner::scan(input, b','), CsvSafety::Simple);

        let encoded = encode(input).unwrap().unwrap();

        assert_eq!(decode(&encoded).unwrap(), input);
    }

    #[test]
    fn empty_input_preserves_the_legacy_single_empty_field_shape() {
        assert_eq!(CsvSafetyScanner::scan(b"", b','), CsvSafety::Simple);
        let parsed = parse_simple_rows(b"", b',').unwrap();
        assert_eq!(parsed.rows.len(), 1);
        assert_eq!(parsed.rows[0].fields().first(), Some(&(0..0)));
        assert_eq!(parsed.rows[0].fields().len(), 1);
        assert_eq!(parsed.newline_style, NewlineStyle::Lf);
        assert!(!parsed.has_final_newline);

        let encoded = encode(b"")
            .unwrap()
            .expect("legacy empty input remains columnar-encodable");
        assert_eq!(decode(&encoded).unwrap(), b"");
    }

    #[test]
    fn compatibility_adapter_preserves_permissive_quote_edges() {
        let cases = [
            b"id,value\n1,un\"quoted\n2,plain\n".as_slice(),
            b"id,value\n1,\"quoted\"suffix\n2,plain\n".as_slice(),
        ];

        for input in cases {
            assert_eq!(
                CsvSafetyScanner::scan(input, b','),
                CsvSafety::RequiresRfc4180
            );
            let encoded = encode(input).unwrap().unwrap();
            assert_eq!(decode(&encoded).unwrap(), input);
        }
    }

    #[test]
    fn compatibility_adapter_preserves_legacy_error_strings() {
        let cases = [
            (b"a,b\r1,2".as_slice(), "unsupported bare CR newline in csv"),
            (
                b"a,b\n1,2\r\n".as_slice(),
                "mixed csv newline styles are unsupported",
            ),
            (
                b"a,b\n1,\"unterminated".as_slice(),
                "unterminated quoted csv field",
            ),
        ];

        for (input, reason) in cases {
            assert_eq!(
                CsvSafetyScanner::scan(input, b','),
                CsvSafety::Unsupported(format!("invalid dpack file: {reason}"))
            );
            assert_eq!(
                encode(input).unwrap_err().to_string(),
                format!("invalid dpack file: invalid dpack file: {reason}")
            );
        }
    }

    #[test]
    fn compatibility_adapter_rejects_inconsistent_widths() {
        for input in [b"a,b\n1,2\n3\n".as_slice(), b"a,b\n1,\"2\"\n3\n".as_slice()] {
            assert_eq!(
                CsvSafetyScanner::scan(input, b','),
                CsvSafety::Unsupported("csv column count is not stable".to_string())
            );
            assert_eq!(
                encode(input).unwrap_err().to_string(),
                "invalid dpack file: csv column count is not stable"
            );
        }
    }

    #[test]
    fn safety_scanner_classifies_simple_and_rfc_and_unsupported() {
        assert_eq!(
            CsvSafetyScanner::scan(b"a,b\r\n1,2\r\n", b','),
            CsvSafety::Simple
        );
        assert_eq!(
            CsvSafetyScanner::scan(b"a,b\r\n1,\"hello, world\"\r\n", b','),
            CsvSafety::RequiresRfc4180
        );
        assert!(matches!(
            CsvSafetyScanner::scan(b"a,b\r\n1,\"unterminated\r\n", b','),
            CsvSafety::Unsupported(_)
        ));
    }

    #[test]
    fn dictionary_mode_requires_estimated_savings() {
        let repeated = vec![
            b"active".as_slice(),
            b"active".as_slice(),
            b"active".as_slice(),
            b"inactive".as_slice(),
        ];
        let unique = vec![b"alpha".as_slice(), b"beta".as_slice(), b"gamma".as_slice()];

        assert_eq!(choose_column_mode(&repeated), ColumnMode::Dictionary);
        assert_eq!(choose_column_mode(&unique), ColumnMode::Plain);
    }

    #[test]
    fn planned_encoder_obeys_plain_and_dictionary_instead_of_replanning() {
        let naturally_dictionary = b"value\nsame\nsame\nsame\n";
        let automatic = encode_with_delimiter(naturally_dictionary, b',')
            .unwrap()
            .unwrap();
        assert_eq!(automatic[FIRST_COLUMN_MODE_OFFSET], 1);
        let plain_plan = execution_plan(&[ColumnStrategy::Plain], 10, 1024);
        let forced_plain = encode_with_execution_plan(naturally_dictionary, b',', &plain_plan)
            .unwrap()
            .unwrap();
        assert_eq!(forced_plain[FIRST_COLUMN_MODE_OFFSET], 0);
        assert_eq!(decode(&forced_plain).unwrap(), naturally_dictionary);

        let naturally_plain = b"value\nalpha\nbeta\ngamma\n";
        let automatic = encode_with_delimiter(naturally_plain, b',')
            .unwrap()
            .unwrap();
        assert_eq!(automatic[FIRST_COLUMN_MODE_OFFSET], 0);
        let dictionary_plan = execution_plan(&[ColumnStrategy::Dictionary], 10, 1024);
        let forced_dictionary = encode_with_execution_plan(naturally_plain, b',', &dictionary_plan)
            .unwrap()
            .unwrap();
        assert_eq!(forced_dictionary[FIRST_COLUMN_MODE_OFFSET], 1);
        assert_eq!(decode(&forced_dictionary).unwrap(), naturally_plain);
    }

    #[test]
    fn planned_dictionary_limits_include_header_and_enforce_exact_boundaries() {
        let input = b"value\nA\nB\n";
        let exact_plan = execution_plan(&[ColumnStrategy::Dictionary], 3, 19);
        let encoded = encode_with_execution_plan(input, b',', &exact_plan)
            .unwrap()
            .unwrap();
        assert_eq!(encoded[FIRST_COLUMN_MODE_OFFSET], 1);
        assert_eq!(decode(&encoded).unwrap(), input);

        let value_limited = execution_plan(&[ColumnStrategy::Dictionary], 2, 19);
        let error = encode_with_execution_plan(input, b',', &value_limited).unwrap_err();
        assert!(error.to_string().contains("more than 2 values"));

        let byte_limited = execution_plan(&[ColumnStrategy::Dictionary], 3, 18);
        let error = encode_with_execution_plan(input, b',', &byte_limited).unwrap_err();
        assert!(error.to_string().contains("more than 18 bytes"));
    }

    #[test]
    fn planned_encoder_rejects_column_shape_mismatch() {
        let input = b"left,right\nA,B\n";
        let missing_column = execution_plan(&[ColumnStrategy::Plain], 10, 1024);
        let error = encode_with_execution_plan(input, b',', &missing_column).unwrap_err();

        assert!(error
            .to_string()
            .contains("plan contains 1 columns, but input contains 2"));
    }

    #[test]
    fn invalid_final_newline_marker_is_rejected() {
        let mut encoded = encode(b"a,b\n1,2\n").unwrap().unwrap();
        encoded[8] = 2;

        let error = decode(&encoded).unwrap_err();

        assert!(error.to_string().contains("final-newline marker"));
    }

    #[test]
    fn trailing_columnar_payload_bytes_are_rejected() {
        let mut encoded = encode(b"a,b\n1,2\n").unwrap().unwrap();
        encoded.push(0xff);

        let error = decode(&encoded).unwrap_err();

        assert!(error.to_string().contains("trailing bytes"));
    }

    #[test]
    fn absurd_columnar_counts_are_rejected_before_capacity_allocation() {
        let mut payload = Vec::new();
        payload.extend_from_slice(MAGIC);
        payload.push(b',');
        payload.push(1);
        payload.push(0);
        payload.extend_from_slice(&u64::MAX.to_le_bytes());
        payload.extend_from_slice(&1u32.to_le_bytes());

        let error = decode(&payload).unwrap_err();
        let message = error.to_string();

        assert!(message.contains("row count") || message.contains("declares"));
    }

    #[test]
    fn columnar_output_amplification_limit_is_enforced() {
        let input = b"a,b\n1,2\n";
        let encoded = encode(input).unwrap().unwrap();
        let limit = u64::try_from(input.len() - 1).unwrap();

        let error = decode_with_output_limit(&encoded, limit).unwrap_err();

        assert!(error.to_string().contains("output limit"));
    }

    #[test]
    fn cursor_offset_overflow_is_a_clean_error() {
        let mut cursor = Cursor {
            bytes: &[],
            offset: usize::MAX,
        };

        let error = cursor.read_exact(1).unwrap_err();

        assert!(error.to_string().contains("offset overflow"));
    }
}
