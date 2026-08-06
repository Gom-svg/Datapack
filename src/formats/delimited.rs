//! Crate-private byte-oriented delimited scanning primitives.
//!
//! This module owns lexical mechanics only. Header interpretation, stable-width
//! policy, planner behavior, and user-facing diagnostics remain with their
//! compatibility adapters.

use std::fmt;
use std::ops::Range;

pub(crate) const CANONICAL_DELIMITERS: [u8; 4] = [b',', b';', b'\t', b'|'];
const QUOTE: u8 = b'"';

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum QuoteMode {
    Disabled,
    /// Matches the established DCSV01 grammar: a quote opens only at field
    /// start, doubled quotes escape, and post-quote bytes remain accepted.
    Dcsv01Compatible,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NewlinePolicy {
    Observe,
    RequireConsistent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DelimitedDialect {
    pub(crate) delimiter: u8,
    quote_mode: QuoteMode,
    newline_policy: NewlinePolicy,
}

impl DelimitedDialect {
    pub(crate) fn new(delimiter: u8, quote_mode: QuoteMode, newline_policy: NewlinePolicy) -> Self {
        Self {
            delimiter,
            quote_mode,
            newline_policy,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ObservedNewline {
    None,
    Lf,
    Crlf,
    Mixed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ScanLimits {
    pub(crate) max_input_bytes: usize,
    pub(crate) max_logical_record_bytes: usize,
    pub(crate) max_records: usize,
    pub(crate) max_fields_per_record: usize,
    pub(crate) max_total_fields: usize,
}

impl ScanLimits {
    /// Compatibility limits for a full input already resident in memory.
    ///
    /// These preserve the old codec's accepted domain; callers that need a
    /// source-size-independent bound must supply fixed limits instead.
    pub(crate) fn resident_input(input_len: usize) -> Self {
        let maximum_items = input_len.saturating_add(1);
        Self {
            max_input_bytes: input_len,
            max_logical_record_bytes: input_len,
            max_records: maximum_items,
            max_fields_per_record: maximum_items,
            max_total_fields: maximum_items,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LimitKind {
    InputBytes,
    LogicalRecordBytes,
    Records,
    FieldsPerRecord,
    TotalFields,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ScanError {
    Limit { kind: LimitKind, limit: usize },
    QuoteNotAllowed,
    UnterminatedQuotedField,
    BareCarriageReturn,
    MixedNewlines,
    CounterOverflow(&'static str),
    Allocation(&'static str),
}

impl fmt::Display for ScanError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Limit { kind, limit } => {
                write!(formatter, "delimited {kind} limit of {limit} reached")
            }
            Self::QuoteNotAllowed => formatter.write_str("quotes are not allowed in this mode"),
            Self::UnterminatedQuotedField => formatter.write_str("unterminated quoted field"),
            Self::BareCarriageReturn => formatter.write_str("bare CR record ending is unsupported"),
            Self::MixedNewlines => formatter.write_str("mixed LF and CRLF record endings"),
            Self::CounterOverflow(counter) => {
                write!(formatter, "delimited {counter} counter overflow")
            }
            Self::Allocation(purpose) => {
                write!(formatter, "cannot reserve memory for delimited {purpose}")
            }
        }
    }
}

impl fmt::Display for LimitKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InputBytes => formatter.write_str("input byte"),
            Self::LogicalRecordBytes => formatter.write_str("logical record byte"),
            Self::Records => formatter.write_str("record"),
            Self::FieldsPerRecord => formatter.write_str("per-record field"),
            Self::TotalFields => formatter.write_str("total field"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DelimitedRecord {
    fields: Vec<Range<usize>>,
}

impl DelimitedRecord {
    pub(crate) fn from_fields(fields: Vec<Range<usize>>) -> Self {
        Self { fields }
    }

    pub(crate) fn fields(&self) -> &[Range<usize>] {
        &self.fields
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DelimitedDocument {
    records: Vec<DelimitedRecord>,
    newline: ObservedNewline,
    has_final_newline: bool,
}

impl DelimitedDocument {
    pub(crate) fn into_records(self) -> Vec<DelimitedRecord> {
        self.records
    }

    pub(crate) fn newline(&self) -> ObservedNewline {
        self.newline
    }

    pub(crate) fn has_final_newline(&self) -> bool {
        self.has_final_newline
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code, reason = "Phase 6 is the designated production consumer")]
pub(crate) enum DetectionOutcome {
    Detected(DelimitedDialect),
    Ambiguous(Vec<u8>),
    Undetected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScannerState {
    Outside,
    InQuotes,
    AfterQuote,
    PendingCr,
}

/// Incremental logical-record boundary tracker for bounded streaming callers.
///
/// Field extraction remains with [`parse_document`]; this adapter retains only
/// lexical state and never owns input bytes.
pub(crate) struct LogicalRecordFramer {
    delimiter: u8,
    quote_mode: QuoteMode,
    state: ScannerState,
    at_field_start: bool,
}

impl LogicalRecordFramer {
    pub(crate) fn new(dialect: DelimitedDialect) -> Self {
        Self {
            delimiter: dialect.delimiter,
            quote_mode: dialect.quote_mode,
            state: ScannerState::Outside,
            at_field_start: true,
        }
    }

    /// Consumes one byte and reports whether it completed the current logical
    /// record. The caller must stop feeding this instance after `true`.
    pub(crate) fn push(&mut self, byte: u8) -> Result<bool, ScanError> {
        loop {
            match self.state {
                ScannerState::InQuotes => {
                    if byte == QUOTE {
                        self.state = ScannerState::AfterQuote;
                    }
                    return Ok(false);
                }
                ScannerState::AfterQuote => {
                    if byte == QUOTE {
                        self.state = ScannerState::InQuotes;
                        return Ok(false);
                    }
                    self.state = ScannerState::Outside;
                    self.at_field_start = false;
                }
                ScannerState::PendingCr => {
                    if byte != b'\n' {
                        return Err(ScanError::BareCarriageReturn);
                    }
                    self.state = ScannerState::Outside;
                    return Ok(true);
                }
                ScannerState::Outside => {
                    if self.quote_mode == QuoteMode::Disabled && byte == QUOTE {
                        return Err(ScanError::QuoteNotAllowed);
                    }
                    if self.quote_mode == QuoteMode::Dcsv01Compatible
                        && byte == QUOTE
                        && self.at_field_start
                    {
                        self.state = ScannerState::InQuotes;
                        return Ok(false);
                    }
                    if byte == self.delimiter {
                        self.at_field_start = true;
                        return Ok(false);
                    }
                    if byte == b'\r' {
                        self.state = ScannerState::PendingCr;
                        return Ok(false);
                    }
                    if byte == b'\n' {
                        return Ok(true);
                    }
                    self.at_field_start = false;
                    return Ok(false);
                }
            }
        }
    }

    /// Validates an unterminated final logical record at actual EOF.
    pub(crate) fn finish(&self) -> Result<(), ScanError> {
        match self.state {
            ScannerState::InQuotes => Err(ScanError::UnterminatedQuotedField),
            ScannerState::PendingCr => Err(ScanError::BareCarriageReturn),
            ScannerState::Outside | ScannerState::AfterQuote => Ok(()),
        }
    }
}

struct Scanner {
    dialect: DelimitedDialect,
    limits: ScanLimits,
    state: ScannerState,
    offset: usize,
    record_start: usize,
    field_start: usize,
    current_fields: Vec<Range<usize>>,
    records: Vec<DelimitedRecord>,
    total_fields: usize,
    newline: ObservedNewline,
    has_final_newline: bool,
}

impl Scanner {
    fn new(dialect: DelimitedDialect, limits: ScanLimits) -> Self {
        Self {
            dialect,
            limits,
            state: ScannerState::Outside,
            offset: 0,
            record_start: 0,
            field_start: 0,
            current_fields: Vec::new(),
            records: Vec::new(),
            total_fields: 0,
            newline: ObservedNewline::None,
            has_final_newline: false,
        }
    }

    fn feed(&mut self, chunk: &[u8]) -> Result<(), ScanError> {
        let next_input_size = self
            .offset
            .checked_add(chunk.len())
            .ok_or(ScanError::CounterOverflow("input byte"))?;
        if next_input_size > self.limits.max_input_bytes {
            return Err(ScanError::Limit {
                kind: LimitKind::InputBytes,
                limit: self.limits.max_input_bytes,
            });
        }

        let mut local_index = 0usize;
        while local_index < chunk.len() {
            if self.state == ScannerState::Outside
                && self.offset == self.record_start
                && self.current_fields.is_empty()
                && self.records.len() >= self.limits.max_records
            {
                return Err(ScanError::Limit {
                    kind: LimitKind::Records,
                    limit: self.limits.max_records,
                });
            }
            let byte = chunk[local_index];
            let absolute_index = self.offset;
            let mut consume = true;

            match self.state {
                ScannerState::InQuotes => {
                    if byte == QUOTE {
                        self.state = ScannerState::AfterQuote;
                    }
                }
                ScannerState::AfterQuote => {
                    if byte == QUOTE {
                        self.state = ScannerState::InQuotes;
                    } else {
                        self.state = ScannerState::Outside;
                        consume = false;
                    }
                }
                ScannerState::PendingCr => {
                    if byte != b'\n' {
                        return Err(ScanError::BareCarriageReturn);
                    }
                    let field_end = absolute_index
                        .checked_sub(1)
                        .ok_or(ScanError::CounterOverflow("field offset"))?;
                    self.consume_byte()?;
                    self.finish_record(field_end, ObservedNewline::Crlf)?;
                }
                ScannerState::Outside => {
                    if self.dialect.quote_mode == QuoteMode::Disabled && byte == QUOTE {
                        return Err(ScanError::QuoteNotAllowed);
                    }
                    if self.dialect.quote_mode == QuoteMode::Dcsv01Compatible
                        && byte == QUOTE
                        && absolute_index == self.field_start
                    {
                        self.state = ScannerState::InQuotes;
                        self.has_final_newline = false;
                    } else if byte == self.dialect.delimiter {
                        self.push_field(absolute_index)?;
                        self.field_start = absolute_index
                            .checked_add(1)
                            .ok_or(ScanError::CounterOverflow("field offset"))?;
                        self.has_final_newline = false;
                    } else if byte == b'\r' {
                        self.state = ScannerState::PendingCr;
                    } else if byte == b'\n' {
                        self.consume_byte()?;
                        self.finish_record(absolute_index, ObservedNewline::Lf)?;
                    } else {
                        self.has_final_newline = false;
                    }
                }
            }

            if consume && self.offset == absolute_index {
                self.consume_byte()?;
            }
            if consume {
                local_index = local_index
                    .checked_add(1)
                    .ok_or(ScanError::CounterOverflow("chunk offset"))?;
            }
        }
        Ok(())
    }

    fn consume_byte(&mut self) -> Result<(), ScanError> {
        self.offset = self
            .offset
            .checked_add(1)
            .ok_or(ScanError::CounterOverflow("input byte"))?;
        let record_bytes = self
            .offset
            .checked_sub(self.record_start)
            .ok_or(ScanError::CounterOverflow("logical record byte"))?;
        if record_bytes > self.limits.max_logical_record_bytes {
            return Err(ScanError::Limit {
                kind: LimitKind::LogicalRecordBytes,
                limit: self.limits.max_logical_record_bytes,
            });
        }
        Ok(())
    }

    fn finish_record(
        &mut self,
        field_end: usize,
        newline: ObservedNewline,
    ) -> Result<(), ScanError> {
        self.ensure_record_capacity()?;
        self.push_field(field_end)?;
        self.observe_newline(newline)?;
        let fields = std::mem::take(&mut self.current_fields);
        self.records.push(DelimitedRecord { fields });
        self.record_start = self.offset;
        self.field_start = self.offset;
        self.state = ScannerState::Outside;
        self.has_final_newline = true;
        Ok(())
    }

    fn finish(mut self) -> Result<DelimitedDocument, ScanError> {
        match self.state {
            ScannerState::InQuotes => return Err(ScanError::UnterminatedQuotedField),
            ScannerState::PendingCr => return Err(ScanError::BareCarriageReturn),
            ScannerState::AfterQuote => self.state = ScannerState::Outside,
            ScannerState::Outside => {}
        }

        if !self.has_final_newline
            && (self.offset > self.record_start || !self.current_fields.is_empty())
        {
            self.ensure_record_capacity()?;
            self.push_field(self.offset)?;
            let fields = std::mem::take(&mut self.current_fields);
            self.records.push(DelimitedRecord { fields });
        }

        Ok(DelimitedDocument {
            records: self.records,
            newline: self.newline,
            has_final_newline: self.has_final_newline,
        })
    }

    fn ensure_record_capacity(&mut self) -> Result<(), ScanError> {
        if self.records.len() >= self.limits.max_records {
            return Err(ScanError::Limit {
                kind: LimitKind::Records,
                limit: self.limits.max_records,
            });
        }
        self.records
            .try_reserve(1)
            .map_err(|_| ScanError::Allocation("records"))
    }

    fn push_field(&mut self, end: usize) -> Result<(), ScanError> {
        if self.current_fields.len() >= self.limits.max_fields_per_record {
            return Err(ScanError::Limit {
                kind: LimitKind::FieldsPerRecord,
                limit: self.limits.max_fields_per_record,
            });
        }
        if self.total_fields >= self.limits.max_total_fields {
            return Err(ScanError::Limit {
                kind: LimitKind::TotalFields,
                limit: self.limits.max_total_fields,
            });
        }
        self.current_fields
            .try_reserve(1)
            .map_err(|_| ScanError::Allocation("fields"))?;
        self.current_fields.push(self.field_start..end);
        self.total_fields = self
            .total_fields
            .checked_add(1)
            .ok_or(ScanError::CounterOverflow("field"))?;
        Ok(())
    }

    fn observe_newline(&mut self, next: ObservedNewline) -> Result<(), ScanError> {
        self.newline = match (self.newline, next) {
            (ObservedNewline::None, value) => value,
            (current, value) if current == value => current,
            (ObservedNewline::Mixed, _) => ObservedNewline::Mixed,
            _ if self.dialect.newline_policy == NewlinePolicy::RequireConsistent => {
                return Err(ScanError::MixedNewlines)
            }
            _ => ObservedNewline::Mixed,
        };
        Ok(())
    }
}

pub(crate) fn parse_document(
    bytes: &[u8],
    dialect: DelimitedDialect,
    limits: ScanLimits,
) -> Result<DelimitedDocument, ScanError> {
    parse_document_in_chunks(bytes, dialect, limits, bytes.len().max(1))
}

fn parse_document_in_chunks(
    bytes: &[u8],
    dialect: DelimitedDialect,
    limits: ScanLimits,
    chunk_size: usize,
) -> Result<DelimitedDocument, ScanError> {
    let mut scanner = Scanner::new(dialect, limits);
    for chunk in bytes.chunks(chunk_size.max(1)) {
        scanner.feed(chunk)?;
    }
    scanner.finish()
}

/// Canonical, quote-aware detection. Legacy callers intentionally retain
/// their historical raw-frequency detector until Phase 6.
#[allow(dead_code, reason = "Phase 6 is the designated production consumer")]
pub(crate) fn detect_dialect(
    bytes: &[u8],
    limits: ScanLimits,
) -> Result<DetectionOutcome, ScanError> {
    let mut scored = Vec::new();
    scored
        .try_reserve_exact(CANONICAL_DELIMITERS.len())
        .map_err(|_| ScanError::Allocation("delimiter candidates"))?;

    let mut first_candidate_failure = None;
    let mut first_shape_competitive_failure = None;
    for delimiter in CANONICAL_DELIMITERS {
        let mut scanner = DetectionScanner::new(delimiter, limits);
        match scanner.scan(bytes) {
            Ok(()) => {
                if let Some(score) = scanner.score(false)? {
                    scored.push((delimiter, score));
                }
            }
            Err(error) if is_candidate_specific_failure(&error) => {
                let shape_competitive = scanner.failure_could_match(&error)?;
                if first_candidate_failure.is_none() {
                    first_candidate_failure = Some(error.clone());
                }
                if shape_competitive && first_shape_competitive_failure.is_none() {
                    first_shape_competitive_failure = Some(error);
                }
            }
            Err(error) => return Err(error),
        }
    }

    if let Some(error) = first_shape_competitive_failure {
        return Err(error);
    }
    if !scored.iter().any(|(_, score)| score.consistent) {
        if let Some(error) = first_candidate_failure {
            return Err(error);
        }
    }

    let Some(best_score) = scored.iter().map(|(_, score)| *score).max() else {
        return Ok(DetectionOutcome::Undetected);
    };
    let tied_count = scored
        .iter()
        .filter(|(_, score)| *score == best_score)
        .count();
    let mut candidates = Vec::new();
    candidates
        .try_reserve_exact(tied_count)
        .map_err(|_| ScanError::Allocation("delimiter ties"))?;
    candidates.extend(
        scored
            .into_iter()
            .filter_map(|(delimiter, score)| (score == best_score).then_some(delimiter)),
    );
    match candidates.as_slice() {
        [delimiter] => Ok(DetectionOutcome::Detected(DelimitedDialect::new(
            *delimiter,
            QuoteMode::Dcsv01Compatible,
            NewlinePolicy::Observe,
        ))),
        [] => Ok(DetectionOutcome::Undetected),
        _ => Ok(DetectionOutcome::Ambiguous(candidates)),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct DetectionScore {
    consistent: bool,
    quoted_fields: usize,
    modal_records: usize,
    separator_count: usize,
    modal_width: usize,
}

struct DetectionScanner {
    delimiter: u8,
    limits: ScanLimits,
    state: ScannerState,
    offset: usize,
    record_start: usize,
    records: usize,
    record_has_bytes: bool,
    at_field_start: bool,
    width: usize,
    total_fields: usize,
    width_counts: Vec<usize>,
    significant_records: usize,
    separator_count: usize,
    quoted_fields: usize,
}

impl DetectionScanner {
    fn new(delimiter: u8, limits: ScanLimits) -> Self {
        Self {
            delimiter,
            limits,
            state: ScannerState::Outside,
            offset: 0,
            record_start: 0,
            records: 0,
            record_has_bytes: false,
            at_field_start: true,
            width: 1,
            total_fields: 0,
            width_counts: Vec::new(),
            significant_records: 0,
            separator_count: 0,
            quoted_fields: 0,
        }
    }

    fn scan(&mut self, bytes: &[u8]) -> Result<(), ScanError> {
        if bytes.len() > self.limits.max_input_bytes {
            return Err(ScanError::Limit {
                kind: LimitKind::InputBytes,
                limit: self.limits.max_input_bytes,
            });
        }

        while self.offset < bytes.len() {
            if self.offset == self.record_start {
                if self.records >= self.limits.max_records {
                    return Err(ScanError::Limit {
                        kind: LimitKind::Records,
                        limit: self.limits.max_records,
                    });
                }
                if self.width > self.limits.max_fields_per_record {
                    return Err(ScanError::Limit {
                        kind: LimitKind::FieldsPerRecord,
                        limit: self.limits.max_fields_per_record,
                    });
                }
            }
            let absolute_index = self.offset;
            let byte = bytes[self.offset];
            let mut consume = true;
            match self.state {
                ScannerState::InQuotes => {
                    if byte == QUOTE {
                        self.state = ScannerState::AfterQuote;
                    }
                }
                ScannerState::AfterQuote => {
                    if byte == QUOTE {
                        self.state = ScannerState::InQuotes;
                    } else {
                        self.state = ScannerState::Outside;
                        self.at_field_start = false;
                        consume = false;
                    }
                }
                ScannerState::PendingCr => {
                    if byte != b'\n' {
                        return Err(ScanError::BareCarriageReturn);
                    }
                    self.consume_detection_byte()?;
                    self.finish_detection_record()?;
                }
                ScannerState::Outside => {
                    if byte == QUOTE && self.at_field_start {
                        self.state = ScannerState::InQuotes;
                        self.record_has_bytes = true;
                        self.quoted_fields = self
                            .quoted_fields
                            .checked_add(1)
                            .ok_or(ScanError::CounterOverflow("candidate quoted field"))?;
                    } else if byte == self.delimiter {
                        self.width = self
                            .width
                            .checked_add(1)
                            .ok_or(ScanError::CounterOverflow("candidate field"))?;
                        self.separator_count = self
                            .separator_count
                            .checked_add(1)
                            .ok_or(ScanError::CounterOverflow("candidate separator"))?;
                        if self.width > self.limits.max_fields_per_record {
                            return Err(ScanError::Limit {
                                kind: LimitKind::FieldsPerRecord,
                                limit: self.limits.max_fields_per_record,
                            });
                        }
                        self.at_field_start = true;
                        self.record_has_bytes = true;
                    } else if byte == b'\r' {
                        self.state = ScannerState::PendingCr;
                    } else if byte == b'\n' {
                        self.consume_detection_byte()?;
                        self.finish_detection_record()?;
                    } else {
                        self.at_field_start = false;
                        self.record_has_bytes = true;
                    }
                }
            }
            if consume && self.offset == absolute_index {
                self.consume_detection_byte()?;
            }
        }

        match self.state {
            ScannerState::InQuotes => return Err(ScanError::UnterminatedQuotedField),
            ScannerState::PendingCr => return Err(ScanError::BareCarriageReturn),
            ScannerState::AfterQuote | ScannerState::Outside => {}
        }
        if self.offset > self.record_start {
            self.finish_detection_record()?;
        }
        Ok(())
    }

    fn score(&self, include_current_record: bool) -> Result<Option<DetectionScore>, ScanError> {
        let include_current_record = include_current_record && self.record_has_bytes;
        let significant_records = self
            .significant_records
            .checked_add(usize::from(include_current_record))
            .ok_or(ScanError::CounterOverflow("significant record"))?;
        if self.separator_count == 0 || significant_records == 0 {
            return Ok(None);
        }

        let mut modal_width = 0usize;
        let mut modal_records = 0usize;
        for (width, &completed_records) in self.width_counts.iter().enumerate() {
            let current_record = usize::from(include_current_record && self.width == width);
            let records = completed_records
                .checked_add(current_record)
                .ok_or(ScanError::CounterOverflow("candidate width"))?;
            if (records, width) > (modal_records, modal_width) {
                modal_width = width;
                modal_records = records;
            }
        }
        if include_current_record
            && self.width >= self.width_counts.len()
            && (1, self.width) > (modal_records, modal_width)
        {
            modal_width = self.width;
            modal_records = 1;
        }

        Ok(Some(DetectionScore {
            consistent: modal_records == significant_records,
            quoted_fields: self.quoted_fields,
            modal_records,
            separator_count: self.separator_count,
            modal_width,
        }))
    }

    fn failure_could_match(&self, error: &ScanError) -> Result<bool, ScanError> {
        if self.separator_count == 0 {
            return Ok(false);
        }
        if matches!(
            error,
            ScanError::UnterminatedQuotedField
                | ScanError::BareCarriageReturn
                | ScanError::MixedNewlines
                | ScanError::QuoteNotAllowed
                | ScanError::Limit {
                    kind: LimitKind::TotalFields,
                    ..
                }
        ) {
            return Ok(self.score(true)?.is_some_and(|score| score.consistent));
        }

        let mut established_width = None;
        for (width, &records) in self.width_counts.iter().enumerate() {
            if records == 0 {
                continue;
            }
            if established_width.is_some_and(|established| established != width) {
                return Ok(false);
            }
            established_width = Some(width);
        }
        Ok(established_width
            .is_none_or(|established| !self.record_has_bytes || self.width <= established))
    }

    fn consume_detection_byte(&mut self) -> Result<(), ScanError> {
        self.offset = self
            .offset
            .checked_add(1)
            .ok_or(ScanError::CounterOverflow("detection input byte"))?;
        let record_bytes = self
            .offset
            .checked_sub(self.record_start)
            .ok_or(ScanError::CounterOverflow("detection record byte"))?;
        if record_bytes > self.limits.max_logical_record_bytes {
            return Err(ScanError::Limit {
                kind: LimitKind::LogicalRecordBytes,
                limit: self.limits.max_logical_record_bytes,
            });
        }
        Ok(())
    }

    fn finish_detection_record(&mut self) -> Result<(), ScanError> {
        if self.records >= self.limits.max_records {
            return Err(ScanError::Limit {
                kind: LimitKind::Records,
                limit: self.limits.max_records,
            });
        }
        self.total_fields = self
            .total_fields
            .checked_add(self.width)
            .ok_or(ScanError::CounterOverflow("candidate total field"))?;
        if self.total_fields > self.limits.max_total_fields {
            return Err(ScanError::Limit {
                kind: LimitKind::TotalFields,
                limit: self.limits.max_total_fields,
            });
        }
        if self.record_has_bytes {
            increment_width_count(&mut self.width_counts, self.width)?;
        }
        if self.record_has_bytes {
            self.significant_records = self
                .significant_records
                .checked_add(1)
                .ok_or(ScanError::CounterOverflow("significant record"))?;
        }
        self.records = self
            .records
            .checked_add(1)
            .ok_or(ScanError::CounterOverflow("detection record"))?;
        self.record_start = self.offset;
        self.record_has_bytes = false;
        self.at_field_start = true;
        self.width = 1;
        self.state = ScannerState::Outside;
        Ok(())
    }
}

fn is_candidate_specific_failure(error: &ScanError) -> bool {
    !matches!(
        error,
        ScanError::Limit {
            kind: LimitKind::InputBytes,
            ..
        } | ScanError::CounterOverflow(_)
            | ScanError::Allocation(_)
    )
}

fn increment_width_count(counts: &mut Vec<usize>, width: usize) -> Result<(), ScanError> {
    let needed_len = width
        .checked_add(1)
        .ok_or(ScanError::CounterOverflow("candidate width histogram"))?;
    if counts.len() < needed_len {
        let additional = needed_len
            .checked_sub(counts.len())
            .ok_or(ScanError::CounterOverflow("candidate width histogram"))?;
        counts
            .try_reserve_exact(additional)
            .map_err(|_| ScanError::Allocation("candidate widths"))?;
        counts.resize(needed_len, 0);
    }
    let count = counts
        .get_mut(width)
        .ok_or(ScanError::CounterOverflow("candidate width histogram"))?;
    *count = count
        .checked_add(1)
        .ok_or(ScanError::CounterOverflow("candidate width"))?;
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LegacySplitError {
    UnterminatedQuotedField,
    FieldCounterOverflow,
    Allocation,
}

/// Splits one UTF-8 physical record with the exact PlannerPolicyV1 quote
/// behavior. Framing, header policy, and validation remain with the caller.
pub(crate) fn split_legacy_physical_record<'a>(
    record: &'a str,
    fields: &mut Vec<&'a str>,
    max_retained_fields: usize,
) -> Result<usize, LegacySplitError> {
    fields.clear();
    let bytes = record.as_bytes();
    let mut field_start = 0usize;
    let mut field_count = 0usize;
    let mut quoted = false;
    let mut index = 0usize;
    while index < bytes.len() {
        match bytes[index] {
            QUOTE if quoted && bytes.get(index + 1) == Some(&QUOTE) => {
                index = index
                    .checked_add(2)
                    .ok_or(LegacySplitError::FieldCounterOverflow)?;
            }
            QUOTE => {
                quoted = !quoted;
                index = index
                    .checked_add(1)
                    .ok_or(LegacySplitError::FieldCounterOverflow)?;
            }
            b',' if !quoted => {
                retain_legacy_field(fields, &record[field_start..index], max_retained_fields)?;
                field_count = field_count
                    .checked_add(1)
                    .ok_or(LegacySplitError::FieldCounterOverflow)?;
                index = index
                    .checked_add(1)
                    .ok_or(LegacySplitError::FieldCounterOverflow)?;
                field_start = index;
            }
            _ => {
                index = index
                    .checked_add(1)
                    .ok_or(LegacySplitError::FieldCounterOverflow)?;
            }
        }
    }
    if quoted {
        return Err(LegacySplitError::UnterminatedQuotedField);
    }
    retain_legacy_field(fields, &record[field_start..], max_retained_fields)?;
    field_count
        .checked_add(1)
        .ok_or(LegacySplitError::FieldCounterOverflow)
}

fn retain_legacy_field<'a>(
    fields: &mut Vec<&'a str>,
    field: &'a str,
    max_retained_fields: usize,
) -> Result<(), LegacySplitError> {
    if fields.len() >= max_retained_fields {
        return Ok(());
    }
    fields
        .try_reserve(1)
        .map_err(|_| LegacySplitError::Allocation)?;
    fields.push(field);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn limits(input: &[u8]) -> ScanLimits {
        ScanLimits::resident_input(input.len())
    }

    fn dialect(delimiter: u8) -> DelimitedDialect {
        DelimitedDialect::new(
            delimiter,
            QuoteMode::Dcsv01Compatible,
            NewlinePolicy::RequireConsistent,
        )
    }

    fn values<'a>(input: &'a [u8], document: &DelimitedDocument) -> Vec<Vec<&'a [u8]>> {
        document
            .records
            .iter()
            .map(|record| {
                record
                    .fields()
                    .iter()
                    .map(|range| &input[range.clone()])
                    .collect()
            })
            .collect()
    }

    #[test]
    fn parses_all_canonical_delimiters_without_consuming_a_header() {
        for delimiter in CANONICAL_DELIMITERS {
            let input = [b'1', delimiter, b'A', b'\n', b'2', delimiter, b'B'];
            let document = parse_document(&input, dialect(delimiter), limits(&input)).unwrap();
            assert_eq!(
                values(&input, &document),
                vec![
                    vec![b"1".as_slice(), b"A".as_slice()],
                    vec![b"2".as_slice(), b"B".as_slice()]
                ]
            );
        }
    }

    #[test]
    fn preserves_quoted_delimiters_escaped_quotes_and_multiline_bytes() {
        let input = b"id,note\r\n1,\"left, \"\"quoted\"\"\r\nright\"\r\n2,plain";
        let document = parse_document(input, dialect(b','), limits(input)).unwrap();
        assert_eq!(document.newline(), ObservedNewline::Crlf);
        assert!(!document.has_final_newline());
        assert_eq!(
            values(input, &document),
            vec![
                vec![b"id".as_slice(), b"note".as_slice()],
                vec![
                    b"1".as_slice(),
                    b"\"left, \"\"quoted\"\"\r\nright\"".as_slice()
                ],
                vec![b"2".as_slice(), b"plain".as_slice()]
            ]
        );
    }

    #[test]
    fn preserves_empty_quoted_and_unquoted_fields_and_blank_records() {
        let input = b",a,,\"\"\n\n,b,\n";
        let document = parse_document(input, dialect(b','), limits(input)).unwrap();
        assert!(document.has_final_newline());
        assert_eq!(
            values(input, &document),
            vec![
                vec![
                    b"".as_slice(),
                    b"a".as_slice(),
                    b"".as_slice(),
                    b"\"\"".as_slice()
                ],
                vec![b"".as_slice()],
                vec![b"".as_slice(), b"b".as_slice(), b"".as_slice()]
            ]
        );
    }

    #[test]
    fn shape_and_header_policy_remain_outside_the_scanner() {
        let input = b"1,Ada,active\n2,Grace\n";
        let document = parse_document(input, dialect(b','), limits(input)).unwrap();
        let rows = values(input, &document);
        assert_eq!(rows[0], [b"1".as_slice(), b"Ada", b"active"]);
        assert_eq!(rows[1], [b"2".as_slice(), b"Grace"]);
    }

    #[test]
    fn bytes_are_opaque_to_utf8() {
        let input = [b'a', b',', 0xff, b'\n'];
        let document = parse_document(&input, dialect(b','), limits(&input)).unwrap();
        assert_eq!(values(&input, &document)[0][1], &[0xff]);
    }

    #[test]
    fn permissive_dcsv01_quote_edges_are_characterized() {
        let input = b"a,b\n1,x\"y\n2,\"z\"tail\n";
        let document = parse_document(input, dialect(b','), limits(input)).unwrap();
        assert_eq!(values(input, &document)[1][1], b"x\"y");
        assert_eq!(values(input, &document)[2][1], b"\"z\"tail");
    }

    #[test]
    fn malformed_record_endings_and_quotes_are_typed() {
        let bare_cr = b"a,b\r1,2";
        assert_eq!(
            parse_document(bare_cr, dialect(b','), limits(bare_cr)),
            Err(ScanError::BareCarriageReturn)
        );
        let mixed = b"a,b\n1,2\r\n";
        assert_eq!(
            parse_document(mixed, dialect(b','), limits(mixed)),
            Err(ScanError::MixedNewlines)
        );
        let quote = b"a,b\n1,\"open";
        assert_eq!(
            parse_document(quote, dialect(b','), limits(quote)),
            Err(ScanError::UnterminatedQuotedField)
        );
    }

    #[test]
    fn observe_policy_reports_mixed_newlines_without_normalizing() {
        let input = b"a,b\n1,2\r\n";
        let observed =
            DelimitedDialect::new(b',', QuoteMode::Dcsv01Compatible, NewlinePolicy::Observe);
        let document = parse_document(input, observed, limits(input)).unwrap();
        assert_eq!(document.newline(), ObservedNewline::Mixed);
    }

    #[test]
    fn disabled_quotes_match_the_simple_parser_gate() {
        let input = b"a,b\n1,\"x\"\n";
        let simple =
            DelimitedDialect::new(b',', QuoteMode::Disabled, NewlinePolicy::RequireConsistent);
        assert_eq!(
            parse_document(input, simple, limits(input)),
            Err(ScanError::QuoteNotAllowed)
        );
    }

    #[test]
    fn every_limit_accepts_the_boundary_and_rejects_one_more() {
        let input = b"a,b\n1,2";
        let exact = ScanLimits {
            max_input_bytes: input.len(),
            max_logical_record_bytes: 4,
            max_records: 2,
            max_fields_per_record: 2,
            max_total_fields: 4,
        };
        assert!(parse_document(input, dialect(b','), exact).is_ok());

        let cases = [
            (
                ScanLimits {
                    max_input_bytes: input.len() - 1,
                    ..exact
                },
                LimitKind::InputBytes,
            ),
            (
                ScanLimits {
                    max_logical_record_bytes: 3,
                    ..exact
                },
                LimitKind::LogicalRecordBytes,
            ),
            (
                ScanLimits {
                    max_records: 1,
                    ..exact
                },
                LimitKind::Records,
            ),
            (
                ScanLimits {
                    max_fields_per_record: 1,
                    ..exact
                },
                LimitKind::FieldsPerRecord,
            ),
            (
                ScanLimits {
                    max_total_fields: 3,
                    ..exact
                },
                LimitKind::TotalFields,
            ),
        ];
        for (case, kind) in cases {
            assert!(matches!(
                parse_document(input, dialect(b','), case),
                Err(ScanError::Limit { kind: actual, .. }) if actual == kind
            ));
        }
    }

    #[test]
    fn record_and_input_limits_never_complete_an_out_of_scope_record() {
        let input = b"a,b\nthis,record,is,beyond,the,next,scope";
        let record_limited = ScanLimits {
            max_input_bytes: input.len(),
            max_logical_record_bytes: 4,
            max_records: 1,
            max_fields_per_record: 16,
            max_total_fields: 16,
        };
        assert!(matches!(
            parse_document(input, dialect(b','), record_limited),
            Err(ScanError::Limit {
                kind: LimitKind::Records,
                ..
            })
        ));

        let quoted = b"a,b\n1,\"continued\nrecord\"\n";
        let input_limited = ScanLimits {
            max_input_bytes: quoted.len() - 1,
            ..limits(quoted)
        };
        assert!(matches!(
            parse_document(quoted, dialect(b','), input_limited),
            Err(ScanError::Limit {
                kind: LimitKind::InputBytes,
                ..
            })
        ));
    }

    #[test]
    fn chunk_boundaries_do_not_change_events() {
        let input = b"a,b\r\n1,\"x,\"\"y\"\"\r\nz\"\r\n2,q";
        let expected = parse_document(input, dialect(b','), limits(input)).unwrap();
        for chunk_size in [1, 2, 3, 7] {
            let actual =
                parse_document_in_chunks(input, dialect(b','), limits(input), chunk_size).unwrap();
            assert_eq!(actual, expected, "chunk size {chunk_size}");
        }
    }

    #[test]
    fn detector_ignores_quoted_decoys_and_reports_ambiguity() {
        let comma = b"name,\"note|detail|more\"\na,\"x|y|z\"\n";
        assert!(matches!(
            detect_dialect(comma, limits(comma)).unwrap(),
            DetectionOutcome::Detected(dialect) if dialect.delimiter == b','
        ));

        let multiline_comma = b"id,note|tag\n1,\"left|right\nup|down\"\n2,plain|tail\n";
        assert!(matches!(
            detect_dialect(multiline_comma, limits(multiline_comma)).unwrap(),
            DetectionOutcome::Detected(dialect) if dialect.delimiter == b','
        ));

        let ambiguous = b"a,b|c\n1,2|3\n";
        assert_eq!(
            detect_dialect(ambiguous, limits(ambiguous)).unwrap(),
            DetectionOutcome::Ambiguous(vec![b',', b'|'])
        );
        let absent = b"alpha\nbeta\n";
        assert_eq!(
            detect_dialect(absent, limits(absent)).unwrap(),
            DetectionOutcome::Undetected
        );
    }

    #[test]
    fn detector_keeps_quote_state_local_to_each_candidate() {
        let pipe = b"h1|h2\na,\"b|c\"\n";
        assert!(matches!(
            detect_dialect(pipe, limits(pipe)).unwrap(),
            DetectionOutcome::Detected(dialect) if dialect.delimiter == b'|'
        ));

        let pipe_with_literal_unterminated_quote = b"h1|h2\na,\"unterminated|b\n";
        assert!(matches!(
            detect_dialect(
                pipe_with_literal_unterminated_quote,
                limits(pipe_with_literal_unterminated_quote)
            )
            .unwrap(),
            DetectionOutcome::Detected(dialect) if dialect.delimiter == b'|'
        ));

        let comma = b"a,\"x;y\"\nc,\"u;v\"\n";
        assert!(matches!(
            detect_dialect(comma, limits(comma)).unwrap(),
            DetectionOutcome::Detected(dialect) if dialect.delimiter == b','
        ));
    }

    #[test]
    fn detector_does_not_reinterpret_an_unterminated_quote_as_another_dialect() {
        let malformed = b"a,b\n1,\"unterminated;value\n";
        assert_eq!(
            detect_dialect(malformed, limits(malformed)),
            Err(ScanError::UnterminatedQuotedField)
        );

        let shape_competitive = b"a,b|c\n1,\"unterminated|x\n";
        assert_eq!(
            detect_dialect(shape_competitive, limits(shape_competitive)),
            Err(ScanError::UnterminatedQuotedField)
        );
    }

    #[test]
    fn detector_covers_semicolon_tab_and_pipe() {
        for delimiter in [b';', b'\t', b'|'] {
            let input = [b'a', delimiter, b'b', b'\n', b'1', delimiter, b'2'];
            assert!(matches!(
                detect_dialect(&input, limits(&input)).unwrap(),
                DetectionOutcome::Detected(dialect) if dialect.delimiter == delimiter
            ));
        }
    }

    #[test]
    fn detector_enforces_every_configured_limit() {
        let input = b"a,b\n1,2";
        let exact = ScanLimits {
            max_input_bytes: input.len(),
            max_logical_record_bytes: 4,
            max_records: 2,
            max_fields_per_record: 2,
            max_total_fields: 4,
        };
        assert!(matches!(
            detect_dialect(input, exact).unwrap(),
            DetectionOutcome::Detected(dialect) if dialect.delimiter == b','
        ));

        let cases = [
            (
                ScanLimits {
                    max_input_bytes: input.len() - 1,
                    ..exact
                },
                LimitKind::InputBytes,
            ),
            (
                ScanLimits {
                    max_logical_record_bytes: 3,
                    ..exact
                },
                LimitKind::LogicalRecordBytes,
            ),
            (
                ScanLimits {
                    max_records: 1,
                    ..exact
                },
                LimitKind::Records,
            ),
            (
                ScanLimits {
                    max_fields_per_record: 1,
                    ..exact
                },
                LimitKind::FieldsPerRecord,
            ),
            (
                ScanLimits {
                    max_total_fields: 3,
                    ..exact
                },
                LimitKind::TotalFields,
            ),
        ];
        for (case, kind) in cases {
            assert!(matches!(
                detect_dialect(input, case),
                Err(ScanError::Limit { kind: actual, .. }) if actual == kind
            ));
        }
    }

    #[test]
    fn irrelevant_candidate_limits_do_not_mask_a_valid_dialect() {
        let pipe = b"h1|h2\n1|\"x\ny\"\n";
        let bounded = ScanLimits {
            max_input_bytes: pipe.len(),
            max_logical_record_bytes: pipe.len(),
            max_records: 2,
            max_fields_per_record: 2,
            max_total_fields: 4,
        };
        assert!(matches!(
            detect_dialect(pipe, bounded).unwrap(),
            DetectionOutcome::Detected(dialect) if dialect.delimiter == b'|'
        ));
    }

    #[test]
    fn detector_enforces_zero_field_limit_for_one_field_record() {
        let input = b"value\n";
        let zero_fields = ScanLimits {
            max_input_bytes: input.len(),
            max_logical_record_bytes: input.len(),
            max_records: 1,
            max_fields_per_record: 0,
            max_total_fields: 1,
        };
        assert_eq!(
            detect_dialect(input, zero_fields),
            Err(ScanError::Limit {
                kind: LimitKind::FieldsPerRecord,
                limit: 0,
            })
        );
    }

    #[test]
    fn logical_record_framer_preserves_shared_scanner_boundaries() {
        type FramingCase<'a> = (u8, &'a [u8], &'a [&'a [u8]]);
        let cases: &[FramingCase<'_>] = &[
            (
                b',',
                b"a,b\n1,\"x\ny\"\n2,\"a\"\"b\"",
                &[b"a,b\n", b"1,\"x\ny\"\n", b"2,\"a\"\"b\""],
            ),
            (
                b'\t',
                b"a\tb\r\n1\t\"x\ty\"\r\n",
                &[b"a\tb\r\n", b"1\t\"x\ty\"\r\n"],
            ),
            (b'|', b"a|b\n1||\n", &[b"a|b\n", b"1||\n"]),
            (b';', b"a;b\n1;2", &[b"a;b\n", b"1;2"]),
        ];

        for &(delimiter, input, expected) in cases {
            let dialect = dialect(delimiter);
            let mut framer = LogicalRecordFramer::new(dialect);
            let mut start = 0usize;
            let mut records = Vec::new();
            for (index, &byte) in input.iter().enumerate() {
                if framer.push(byte).unwrap() {
                    let end = index + 1;
                    records.push(&input[start..end]);
                    start = end;
                    framer = LogicalRecordFramer::new(dialect);
                }
            }
            framer.finish().unwrap();
            if start < input.len() {
                records.push(&input[start..]);
            }
            assert_eq!(records, expected, "delimiter {delimiter:?}");
        }
    }

    #[test]
    fn logical_record_framer_validates_only_at_actual_eof() {
        let dialect = dialect(b'|');
        let mut open_quote = LogicalRecordFramer::new(dialect);
        for byte in b"1|\"continued\nrecord" {
            assert!(!open_quote.push(*byte).unwrap());
        }
        assert_eq!(open_quote.finish(), Err(ScanError::UnterminatedQuotedField));

        let mut bare_cr = LogicalRecordFramer::new(dialect);
        for byte in b"1|2\r" {
            assert!(!bare_cr.push(*byte).unwrap());
        }
        assert_eq!(bare_cr.finish(), Err(ScanError::BareCarriageReturn));
    }

    #[test]
    fn legacy_physical_splitter_preserves_toggle_anywhere_behavior() {
        let mut fields = Vec::new();
        let count =
            split_legacy_physical_record("left,x\"quoted,comma\"y,right", &mut fields, usize::MAX)
                .unwrap();
        assert_eq!(count, 3);
        assert_eq!(fields, ["left", "x\"quoted,comma\"y", "right"]);

        assert_eq!(
            split_legacy_physical_record("a,\"open", &mut fields, usize::MAX),
            Err(LegacySplitError::UnterminatedQuotedField)
        );
    }
}
