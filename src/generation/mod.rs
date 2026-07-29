//! Controlled benchmark data generation.
//!
//! `repetitive` stresses dictionary/RLE opportunities with heavily skewed low-cardinality columns.
//! `realistic` models fictitious business exports with mixed repeated dimensions and varied facts.
//! `high-cardinality` exercises fallback behavior by making most columns effectively unique.
//! `random` approximates worst-case flat CSV input with deterministic pseudo-random values.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;
use std::time::{Duration, Instant};

use crate::error::{DatapackError, Result};

pub const DEFAULT_ROWS: u64 = 10_000;
pub const MIN_ROWS: u64 = 1;
pub const MAX_ROWS: u64 = 5_000_000;
const PROGRESS_ROW_INTERVAL: u64 = 100_000;
const PROGRESS_TIME_INTERVAL: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Profile {
    Repetitive,
    Realistic,
    HighCardinality,
    Random,
}

impl Profile {
    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "repetitive" => Ok(Self::Repetitive),
            "realistic" => Ok(Self::Realistic),
            "high-cardinality" => Ok(Self::HighCardinality),
            "random" => Ok(Self::Random),
            _ => Err(DatapackError::InvalidProfile(value.to_string())),
        }
    }
}

pub fn generate_to_path(
    profile: Profile,
    output: &Path,
    rows: u64,
    seed: Option<u64>,
) -> Result<()> {
    validate_rows(rows)?;
    let file = File::create(output)
        .map_err(|err| DatapackError::OutputNotWritable(format!("{} ({err})", output.display())))?;
    let mut writer = CsvWriter::new(BufWriter::new(file));
    let mut rng = SplitMix64::new(seed.unwrap_or(default_seed(profile)));
    let started = Instant::now();
    let mut last_report = Instant::now();

    write_header(&mut writer, profile)?;
    for row_index in 0..rows {
        let row = generate_row(profile, row_index, rows, &mut rng);
        writer.write_record(&row)?;
        let completed_rows = row_index + 1;
        if rows > 500_000
            && (completed_rows % PROGRESS_ROW_INTERVAL == 0
                || last_report.elapsed() >= PROGRESS_TIME_INTERVAL)
        {
            report_generation_progress(completed_rows, rows, writer.bytes_written(), started);
            last_report = Instant::now();
        }
    }
    if rows > 500_000 {
        report_generation_progress(rows, rows, writer.bytes_written(), started);
    }
    writer.flush()
}

fn report_generation_progress(rows_done: u64, total_rows: u64, bytes: u64, started: Instant) {
    let elapsed = started.elapsed().as_secs_f64().max(0.001);
    let mb = bytes as f64 / 1_048_576.0;
    eprintln!(
        "phase=generate-test-data rows={rows_done}/{total_rows} mb={mb:.2} elapsed={elapsed:.1}s throughput={:.2} MB/s",
        mb / elapsed
    );
}

pub fn validate_rows(rows: u64) -> Result<()> {
    if !(MIN_ROWS..=MAX_ROWS).contains(&rows) {
        Err(DatapackError::RowsOutOfRange(rows))
    } else {
        Ok(())
    }
}

fn write_header<W: Write>(writer: &mut CsvWriter<W>, profile: Profile) -> Result<()> {
    let header: &[&str] = match profile {
        Profile::Repetitive => &[
            "region",
            "department",
            "status",
            "tier",
            "product_line",
            "channel",
            "priority",
            "category",
            "sub_category",
            "flag",
        ],
        Profile::Realistic => &[
            "record_id",
            "client_name",
            "region",
            "department",
            "account_type",
            "transaction_date",
            "amount",
            "currency",
            "status",
            "payment_method",
            "notes",
            "checksum",
        ],
        Profile::HighCardinality => &[
            "uuid",
            "session_token",
            "user_id",
            "email",
            "ip_address",
            "timestamp",
            "score",
            "referrer_url",
            "search_query",
            "raw_payload",
        ],
        Profile::Random => &[
            "col_a", "col_b", "col_c", "col_d", "col_e", "col_f", "col_g", "col_h",
        ],
    };
    writer.write_record(header)
}

fn generate_row(profile: Profile, index: u64, rows: u64, rng: &mut SplitMix64) -> Vec<String> {
    match profile {
        Profile::Repetitive => repetitive_row(rng),
        Profile::Realistic => realistic_row(index, rng),
        Profile::HighCardinality => high_cardinality_row(index, rows, rng),
        Profile::Random => random_row(rng),
    }
}

fn repetitive_row(rng: &mut SplitMix64) -> Vec<String> {
    vec![
        skewed(rng, &["North", "South", "East", "West"]).to_string(),
        skewed(rng, &["Sales", "HR", "IT", "Finance", "Ops", "Legal"]).to_string(),
        skewed(rng, &["Active", "Inactive", "Pending"]).to_string(),
        skewed(rng, &["Gold", "Silver", "Bronze"]).to_string(),
        skewed(rng, &["Alpha", "Beta", "Gamma", "Delta", "Epsilon"]).to_string(),
        skewed(rng, &["Online", "Phone", "Store", "Partner"]).to_string(),
        skewed(rng, &["Low", "Medium", "High"]).to_string(),
        skewed(rng, &["Nalo", "Vexa", "Mira", "Tavo", "Luno"]).to_string(),
        skewed(rng, &["Kip", "Rul", "Zin", "Pax"]).to_string(),
        skewed(rng, &["Y", "N"]).to_string(),
    ]
}

fn realistic_row(index: u64, rng: &mut SplitMix64) -> Vec<String> {
    let record_id = format!("{:08}", index + 1);
    let checksum = checksum8(&record_id);
    let amount = if rng.chance(30) {
        skewed(
            rng,
            &["19.99", "49.99", "99.00", "125.50", "250.00", "999.99"],
        )
        .to_string()
    } else {
        format!("{:.2}", 0.01 + (rng.next_bounded(9_999_999) as f64 / 100.0))
    };
    vec![
        record_id.clone(),
        format!(
            "{} {}",
            skewed(rng, COMPANY_PREFIXES),
            pick(rng, COMPANY_SUFFIXES)
        ),
        skewed(rng, &["North", "South", "East", "West"]).to_string(),
        skewed(rng, &["Sales", "HR", "IT", "Finance", "Ops", "Legal"]).to_string(),
        skewed(rng, &["Standard", "Premium", "Enterprise", "Trial"]).to_string(),
        clustered_date(rng),
        amount,
        weighted(rng, &[("USD", 70), ("EUR", 20), ("CRC", 10)]).to_string(),
        weighted(
            rng,
            &[
                ("Paid", 58),
                ("Pending", 20),
                ("Overdue", 12),
                ("Cancelled", 6),
                ("Disputed", 4),
            ],
        )
        .to_string(),
        weighted(
            rng,
            &[("Wire", 18), ("ACH", 38), ("Card", 34), ("Check", 10)],
        )
        .to_string(),
        note(index, rng),
        checksum,
    ]
}

fn high_cardinality_row(index: u64, rows: u64, rng: &mut SplitMix64) -> Vec<String> {
    let user_id = index + 1;
    let token_a = rng.next();
    let token_b = rng.next();
    let query_words = 4 + rng.next_bounded(5) as usize;
    let payload_len = 40 + rng.next_bounded(41) as usize;
    vec![
        uuid_like(index, rng),
        format!("{token_a:016x}{token_b:016x}"),
        user_id.to_string(),
        format!("user{:08}@{}.test", user_id, pick(rng, INVENTED_DOMAINS)),
        format!(
            "{}.{}.{}.{}",
            10 + (index % 200),
            (index / 200) % 255,
            (index / 51_000) % 255,
            1 + (index % 254)
        ),
        iso_timestamp(index),
        format!(
            "{:.6}",
            ((index * 7919) % rows.max(1)) as f64 + rng.next_f64()
        ),
        format!(
            "https://{}/r/{}/{}/{}",
            pick(rng, INVENTED_DOMAINS),
            random_alnum(rng, 8),
            random_alnum(rng, 10),
            index
        ),
        random_words(rng, query_words),
        random_alnum(rng, payload_len),
    ]
}

fn random_row(rng: &mut SplitMix64) -> Vec<String> {
    let col_a_len = 8 + rng.next_bounded(9) as usize;
    let col_b_len = 4 + rng.next_bounded(29) as usize;
    let col_g_len = 5 + rng.next_bounded(16) as usize;
    let col_h_len = 10 + rng.next_bounded(21) as usize;
    vec![
        random_alnum(rng, col_a_len),
        random_alnum(rng, col_b_len),
        format!("{:.8}", rng.next_f64() * 10_000_000.0),
        rng.next_bounded(i32::MAX as u64).to_string(),
        random_hex(rng, 16),
        random_base64_like(rng, 24),
        random_mixed_word(rng, col_g_len),
        random_printable_no_comma_newline(rng, col_h_len),
    ]
}

fn skewed<'a>(rng: &mut SplitMix64, values: &'a [&str]) -> &'a str {
    if values.len() == 1 || rng.chance(60) {
        values[0]
    } else {
        values[1 + rng.next_bounded((values.len() - 1) as u64) as usize]
    }
}

fn weighted<'a>(rng: &mut SplitMix64, values: &'a [(&str, u64)]) -> &'a str {
    let total: u64 = values.iter().map(|(_, weight)| *weight).sum();
    let mut cursor = rng.next_bounded(total);
    for (value, weight) in values {
        if cursor < *weight {
            return value;
        }
        cursor -= *weight;
    }
    values[0].0
}

fn pick<'a>(rng: &mut SplitMix64, values: &'a [&str]) -> &'a str {
    values[rng.next_bounded(values.len() as u64) as usize]
}

fn clustered_date(rng: &mut SplitMix64) -> String {
    let base_days = if rng.chance(70) {
        730 + rng.next_bounded(366)
    } else {
        rng.next_bounded(1096)
    };
    date_from_2022_day(base_days)
}

fn date_from_2022_day(day_offset: u64) -> String {
    let mut year = 2022;
    let mut day = day_offset as i32;
    loop {
        let days = if is_leap(year) { 366 } else { 365 };
        if day < days {
            break;
        }
        day -= days;
        year += 1;
    }
    let month_days = [
        31,
        if is_leap(year) { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut month = 1;
    for days in month_days {
        if day < days {
            break;
        }
        day -= days;
        month += 1;
    }
    format!("{year:04}-{month:02}-{:02}", day + 1)
}

fn is_leap(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

fn note(index: u64, rng: &mut SplitMix64) -> String {
    let action = pick(
        rng,
        &["Reviewed", "Queued", "Approved", "Reconciled", "Flagged"],
    );
    let object = pick(rng, &["invoice", "account", "case", "shipment", "request"]);
    let code = 1 + (index % 200);
    format!("{action} fictitious {object} batch {code:03}, no external contact required")
}

fn checksum8(value: &str) -> String {
    let mut hash = 0x9e37_79b9_7f4a_7c15u64;
    for byte in value.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0xbf58_476d_1ce4_e5b9);
        hash ^= hash >> 27;
    }
    format!("{:08x}", hash as u32)
}

fn uuid_like(index: u64, rng: &mut SplitMix64) -> String {
    let a = (index as u32) ^ (rng.next() as u32);
    let b = rng.next() as u16;
    let c = ((rng.next() as u16) & 0x0fff) | 0x4000;
    let d = ((rng.next() as u16) & 0x3fff) | 0x8000;
    let e = rng.next() & 0x0000_ffff_ffff_ffff;
    format!("{a:08x}-{b:04x}-{c:04x}-{d:04x}-{e:012x}")
}

fn iso_timestamp(index: u64) -> String {
    let millis = index * 137;
    let seconds = millis / 1000;
    let ms = millis % 1000;
    let day = seconds / 86_400;
    let within_day = seconds % 86_400;
    let hour = within_day / 3600;
    let minute = (within_day % 3600) / 60;
    let second = within_day % 60;
    format!(
        "{}T{hour:02}:{minute:02}:{second:02}.{ms:03}Z",
        date_from_2022_day(day % 1096)
    )
}

fn random_words(rng: &mut SplitMix64, count: usize) -> String {
    (0..count)
        .map(|_| pick(rng, INVENTED_WORDS))
        .collect::<Vec<_>>()
        .join(" ")
}

fn random_alnum(rng: &mut SplitMix64, len: usize) -> String {
    random_from(
        rng,
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789",
        len,
    )
}

fn random_hex(rng: &mut SplitMix64, len: usize) -> String {
    random_from(rng, b"0123456789abcdef", len)
}

fn random_base64_like(rng: &mut SplitMix64, len: usize) -> String {
    random_from(
        rng,
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/",
        len,
    )
}

fn random_mixed_word(rng: &mut SplitMix64, len: usize) -> String {
    random_from(
        rng,
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz",
        len,
    )
}

fn random_printable_no_comma_newline(rng: &mut SplitMix64, len: usize) -> String {
    random_from(
        rng,
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789 !#$%&'()*+-./:;<=>?@[\\]^_`{|}~",
        len,
    )
}

fn random_from(rng: &mut SplitMix64, alphabet: &[u8], len: usize) -> String {
    (0..len)
        .map(|_| alphabet[rng.next_bounded(alphabet.len() as u64) as usize] as char)
        .collect()
}

fn default_seed(profile: Profile) -> u64 {
    match profile {
        Profile::Repetitive => 0xDADA_0001,
        Profile::Realistic => 0xDADA_0002,
        Profile::HighCardinality => 0xDADA_0003,
        Profile::Random => 0xDADA_0004,
    }
}

struct CsvWriter<W: Write> {
    inner: W,
    bytes_written: u64,
}

impl<W: Write> CsvWriter<W> {
    fn new(inner: W) -> Self {
        Self {
            inner,
            bytes_written: 0,
        }
    }

    fn bytes_written(&self) -> u64 {
        self.bytes_written
    }

    fn write_record<S: AsRef<str>>(&mut self, fields: &[S]) -> Result<()> {
        for (index, field) in fields.iter().enumerate() {
            if index > 0 {
                self.write_all(b",")?;
            }
            let encoded_len = csv_field_encoded_len(field.as_ref()) as u64;
            write_field(&mut self.inner, field.as_ref())
                .map_err(|err| DatapackError::OutputNotWritable(err.to_string()))?;
            self.bytes_written = self.bytes_written.saturating_add(encoded_len);
        }
        self.write_all(b"\r\n")
    }

    fn write_all(&mut self, bytes: &[u8]) -> Result<()> {
        self.inner
            .write_all(bytes)
            .map_err(|err| DatapackError::OutputNotWritable(err.to_string()))?;
        self.bytes_written = self.bytes_written.saturating_add(bytes.len() as u64);
        Ok(())
    }

    fn flush(&mut self) -> Result<()> {
        self.inner
            .flush()
            .map_err(|err| DatapackError::OutputNotWritable(err.to_string()))
    }
}

fn write_field<W: Write>(writer: &mut W, field: &str) -> std::io::Result<()> {
    let needs_quotes = field
        .bytes()
        .any(|byte| matches!(byte, b',' | b'"' | b'\r' | b'\n'));
    if !needs_quotes {
        return writer.write_all(field.as_bytes());
    }

    writer.write_all(b"\"")?;
    for byte in field.bytes() {
        if byte == b'"' {
            writer.write_all(b"\"\"")?;
        } else {
            writer.write_all(&[byte])?;
        }
    }
    writer.write_all(b"\"")
}

fn csv_field_encoded_len(field: &str) -> usize {
    let needs_quotes = field
        .bytes()
        .any(|byte| matches!(byte, b',' | b'"' | b'\r' | b'\n'));
    if !needs_quotes {
        return field.len();
    }
    2 + field
        .bytes()
        .map(|byte| if byte == b'"' { 2 } else { 1 })
        .sum::<usize>()
}

#[derive(Debug, Clone)]
struct SplitMix64 {
    state: u64,
}

impl SplitMix64 {
    fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    fn next(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    fn next_bounded(&mut self, upper: u64) -> u64 {
        if upper == 0 {
            0
        } else {
            self.next() % upper
        }
    }

    fn chance(&mut self, percent: u64) -> bool {
        self.next_bounded(100) < percent
    }

    fn next_f64(&mut self) -> f64 {
        (self.next() >> 11) as f64 / ((1u64 << 53) as f64)
    }
}

const COMPANY_PREFIXES: &[&str] = &[
    "Aster", "Brava", "Civon", "Dextra", "Eldin", "Faron", "Galen", "Helio", "Ivara", "Juno",
    "Kairo", "Luma", "Maven", "Nexel", "Orbis", "Prax", "Quanta", "Rivo", "Sora", "Talon",
];
const COMPANY_SUFFIXES: &[&str] = &["Works", "Systems", "Group", "Labs"];
const INVENTED_DOMAINS: &[&str] = &[
    "example-a.test",
    "example-b.test",
    "sample-node.test",
    "demo-grid.test",
];
const INVENTED_WORDS: &[&str] = &[
    "nalo", "vexa", "mira", "tavo", "luno", "prax", "kiva", "zento", "orli", "bex", "cavo", "dori",
    "elun", "fexa", "gavo", "hilo",
];

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn repetitive_profile_shape_and_cardinality() {
        let csv = generate_string(Profile::Repetitive, 1000, 42);
        let rows = parse_csv(&csv);
        assert_eq!(rows.len(), 1001);
        assert!(rows.iter().all(|row| row.len() == 10));
        for column in 0..10 {
            assert!(unique_count(&rows[1..], column) <= 10);
        }
    }

    #[test]
    fn realistic_profile_shape_and_cardinality() {
        let csv = generate_string(Profile::Realistic, 1000, 2026);
        let rows = parse_csv(&csv);
        assert_eq!(rows.len(), 1001);
        assert!(rows.iter().all(|row| row.len() == 12));
        assert!(unique_count(&rows[1..], 1) <= 80);
    }

    #[test]
    fn high_cardinality_profile_shape_and_uniqueness() {
        let csv = generate_string(Profile::HighCardinality, 1000, 7);
        let rows = parse_csv(&csv);
        assert_eq!(rows.len(), 1001);
        assert!(rows.iter().all(|row| row.len() == 10));
        assert!(unique_count(&rows[1..], 0) >= 950);
        assert!(unique_count(&rows[1..], 1) >= 950);
        assert!(unique_count(&rows[1..], 2) >= 950);
    }

    #[test]
    fn random_profile_shape_and_uniqueness() {
        let csv = generate_string(Profile::Random, 1000, 99);
        let rows = parse_csv(&csv);
        assert_eq!(rows.len(), 1001);
        assert!(rows.iter().all(|row| row.len() == 8));
        assert!(unique_count(&rows[1..], 0) >= 950);
    }

    #[test]
    fn deterministic_seed_output_is_byte_identical() {
        let left = generate_string(Profile::Realistic, 50, 2026);
        let right = generate_string(Profile::Realistic, 50, 2026);
        assert_eq!(left.as_bytes(), right.as_bytes());
        assert!(!left.as_bytes().starts_with(&[0xef, 0xbb, 0xbf]));
        assert!(left.contains("\r\n"));
    }

    fn generate_string(profile: Profile, rows: u64, seed: u64) -> String {
        let mut output = Vec::new();
        {
            let mut writer = CsvWriter::new(&mut output);
            let mut rng = SplitMix64::new(seed);
            write_header(&mut writer, profile).unwrap();
            for row_index in 0..rows {
                let row = generate_row(profile, row_index, rows, &mut rng);
                writer.write_record(&row).unwrap();
            }
            writer.flush().unwrap();
        }
        String::from_utf8(output).unwrap()
    }

    fn unique_count(rows: &[Vec<String>], column: usize) -> usize {
        rows.iter()
            .map(|row| row[column].clone())
            .collect::<HashSet<_>>()
            .len()
    }

    fn parse_csv(csv: &str) -> Vec<Vec<String>> {
        assert!(!csv.as_bytes().starts_with(&[0xef, 0xbb, 0xbf]));
        csv.split("\r\n")
            .filter(|line| !line.is_empty())
            .map(parse_line)
            .collect()
    }

    fn parse_line(line: &str) -> Vec<String> {
        let mut fields = Vec::new();
        let mut field = String::new();
        let mut chars = line.chars().peekable();
        let mut quoted = false;
        while let Some(ch) = chars.next() {
            match ch {
                '"' if quoted && chars.peek() == Some(&'"') => {
                    field.push('"');
                    chars.next();
                }
                '"' => quoted = !quoted,
                ',' if !quoted => {
                    fields.push(std::mem::take(&mut field));
                }
                _ => field.push(ch),
            }
        }
        fields.push(field);
        assert!(!quoted);
        fields
    }
}
