use std::fs::File;
use std::io::{BufReader, Read, Write};
use std::path::Path;
use std::time::{Duration, Instant};

use crate::error::Result;

pub(super) const IO_BUFFER_BYTES: usize = 256 * 1024;
const PROGRESS_INTERVAL: Duration = Duration::from_secs(5);

pub(super) fn read_all_buffered_progress(path: &Path, phase: &str) -> Result<Vec<u8>> {
    read_prefix_buffered_progress(path, phase, None)
}

pub(super) fn read_prefix_buffered_progress(
    path: &Path,
    phase: &str,
    max_bytes: Option<u64>,
) -> Result<Vec<u8>> {
    let file_size = std::fs::metadata(path).ok().map(|metadata| metadata.len());
    let total = match (file_size, max_bytes) {
        (Some(file_size), Some(max_bytes)) => Some(file_size.min(max_bytes)),
        (Some(file_size), None) => Some(file_size),
        (None, max_bytes) => max_bytes,
    };
    let mut reader = BufReader::with_capacity(IO_BUFFER_BYTES, File::open(path)?);
    let mut bytes = Vec::new();
    let mut buffer = vec![0u8; IO_BUFFER_BYTES];
    let mut reporter = ProgressReporter::new(phase, total);
    loop {
        let remaining = max_bytes
            .map(|limit| limit.saturating_sub(bytes.len() as u64))
            .unwrap_or(u64::MAX);
        if remaining == 0 {
            break;
        }
        let requested = buffer.len().min(remaining as usize);
        let read = reader.read(&mut buffer[..requested])?;
        if read == 0 {
            break;
        }
        bytes.extend_from_slice(&buffer[..read]);
        reporter.add_bytes(read as u64);
    }
    reporter.finish();
    Ok(bytes)
}

pub(super) fn progress_phase(
    phase: &str,
    processed_bytes: u64,
    total_bytes: Option<u64>,
    started: Instant,
) {
    let elapsed = started.elapsed().as_secs_f64().max(0.001);
    let mb = processed_bytes as f64 / 1_048_576.0;
    let throughput = mb / elapsed;
    match total_bytes {
        Some(total) => {
            let percent = if total == 0 {
                100.0
            } else {
                (processed_bytes as f64 / total as f64 * 100.0).clamp(0.0, 100.0)
            };
            let bytes_per_second = processed_bytes as f64 / elapsed;
            let eta_seconds = if processed_bytes < total && bytes_per_second > 0.0 {
                (total - processed_bytes) as f64 / bytes_per_second
            } else {
                0.0
            };
            eprintln!(
                "phase={phase} rows=unknown mb={:.2}/{:.2} percent={:.1} elapsed={:.1}s eta={:.1}s throughput={:.2} MB/s",
                mb,
                total as f64 / 1_048_576.0,
                percent,
                elapsed,
                eta_seconds,
                throughput
            );
        }
        None => eprintln!(
            "phase={phase} rows=unknown mb={:.2} elapsed={:.1}s throughput={:.2} MB/s",
            mb, elapsed, throughput
        ),
    }
}

struct ProgressReporter {
    phase: String,
    total_bytes: Option<u64>,
    processed_bytes: u64,
    started: Instant,
    last_report: Instant,
}

impl ProgressReporter {
    fn new(phase: &str, total_bytes: Option<u64>) -> Self {
        Self {
            phase: phase.to_string(),
            total_bytes,
            processed_bytes: 0,
            started: Instant::now(),
            last_report: Instant::now(),
        }
    }

    fn add_bytes(&mut self, bytes: u64) {
        self.processed_bytes = self.processed_bytes.saturating_add(bytes);
        if self.last_report.elapsed() >= PROGRESS_INTERVAL {
            self.report();
            self.last_report = Instant::now();
        }
    }

    fn finish(&self) {
        self.report();
    }

    fn report(&self) {
        progress_phase(
            &self.phase,
            self.processed_bytes,
            self.total_bytes,
            self.started,
        );
    }
}

pub(super) struct ProgressReader<R> {
    inner: R,
    reporter: ProgressReporter,
}

impl<R> ProgressReader<R> {
    pub(super) fn new(inner: R, phase: &str, total_bytes: Option<u64>) -> Self {
        Self {
            inner,
            reporter: ProgressReporter::new(phase, total_bytes),
        }
    }

    pub(super) fn finish(&self) {
        self.reporter.finish();
    }
}

impl<R: Read> Read for ProgressReader<R> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        let read = self.inner.read(buffer)?;
        self.reporter.add_bytes(read as u64);
        Ok(read)
    }
}

pub(super) struct ProgressWriter<W> {
    inner: W,
    reporter: ProgressReporter,
}

impl<W> ProgressWriter<W> {
    pub(super) fn new(inner: W, phase: &str, total_bytes: Option<u64>) -> Self {
        Self {
            inner,
            reporter: ProgressReporter::new(phase, total_bytes),
        }
    }

    pub(super) fn finish(&self) {
        self.reporter.finish();
    }
}

impl<W: Write> Write for ProgressWriter<W> {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        let written = self.inner.write(buffer)?;
        self.reporter.add_bytes(written as u64);
        Ok(written)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}
