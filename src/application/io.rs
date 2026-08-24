use std::fs::File;
use std::io::{BufReader, Read, Write};
use std::path::Path;

use super::control::{OperationContext, OperationResult};
use super::progress::{ProgressPhase, ProgressState};

pub(crate) const IO_BUFFER_BYTES: usize = 256 * 1024;
const PROGRESS_INTERVAL_BYTES: u64 = 8 * 1024 * 1024;

pub(crate) fn read_all(
    path: &Path,
    phase: ProgressPhase,
    context: &mut OperationContext<'_>,
) -> OperationResult<Vec<u8>> {
    read_prefix(path, phase, None, context)
}

pub(crate) fn read_prefix(
    path: &Path,
    phase: ProgressPhase,
    max_bytes: Option<u64>,
    context: &mut OperationContext<'_>,
) -> OperationResult<Vec<u8>> {
    context.checkpoint()?;
    let file_size = std::fs::metadata(path).ok().map(|metadata| metadata.len());
    let total = match (file_size, max_bytes) {
        (Some(file_size), Some(max_bytes)) => Some(file_size.min(max_bytes)),
        (Some(file_size), None) => Some(file_size),
        (None, max_bytes) => max_bytes,
    };
    let mut reader = ObservedReader::new(
        BufReader::with_capacity(IO_BUFFER_BYTES, File::open(path)?),
        phase,
        total,
        context,
    );
    let mut bytes = Vec::new();
    let mut buffer = vec![0u8; IO_BUFFER_BYTES];
    loop {
        reader.checkpoint()?;
        let remaining = max_bytes
            .map(|limit| limit.saturating_sub(bytes.len() as u64))
            .unwrap_or(u64::MAX);
        if remaining == 0 {
            break;
        }
        let requested = buffer
            .len()
            .min(usize::try_from(remaining).unwrap_or(usize::MAX));
        let read = reader.read(&mut buffer[..requested])?;
        if read == 0 {
            break;
        }
        bytes.extend_from_slice(&buffer[..read]);
    }
    reader.finish();
    reader.checkpoint()?;
    Ok(bytes)
}

struct ProgressCounter {
    phase: ProgressPhase,
    total_bytes: Option<u64>,
    completed_bytes: u64,
    last_reported_bytes: u64,
}

impl ProgressCounter {
    fn new(
        phase: ProgressPhase,
        total_bytes: Option<u64>,
        context: &mut OperationContext<'_>,
    ) -> Self {
        context.emit_unchecked(phase, ProgressState::Started, 0, total_bytes, 0, None);
        Self {
            phase,
            total_bytes,
            completed_bytes: 0,
            last_reported_bytes: 0,
        }
    }

    fn advance(&mut self, bytes: u64, context: &mut OperationContext<'_>) {
        self.completed_bytes = self.completed_bytes.saturating_add(bytes);
        if self
            .completed_bytes
            .saturating_sub(self.last_reported_bytes)
            >= PROGRESS_INTERVAL_BYTES
        {
            context.emit_unchecked(
                self.phase,
                ProgressState::Advanced,
                self.completed_bytes,
                self.total_bytes,
                0,
                None,
            );
            self.last_reported_bytes = self.completed_bytes;
        }
    }

    fn finish(&self, context: &mut OperationContext<'_>) {
        context.emit_unchecked(
            self.phase,
            ProgressState::Completed,
            self.completed_bytes,
            self.total_bytes,
            0,
            None,
        );
    }
}

pub(crate) struct ObservedReader<'emitter, 'observer, R> {
    inner: R,
    counter: ProgressCounter,
    context: &'emitter mut OperationContext<'observer>,
}

impl<'emitter, 'observer, R> ObservedReader<'emitter, 'observer, R> {
    pub(crate) fn new(
        inner: R,
        phase: ProgressPhase,
        total_bytes: Option<u64>,
        context: &'emitter mut OperationContext<'observer>,
    ) -> Self {
        Self {
            inner,
            counter: ProgressCounter::new(phase, total_bytes, context),
            context,
        }
    }

    pub(crate) fn finish(&mut self) {
        self.counter.finish(self.context);
    }

    pub(crate) fn checkpoint(&self) -> OperationResult<()> {
        self.context.checkpoint()
    }
}

impl<R: Read> Read for ObservedReader<'_, '_, R> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        let read = self.inner.read(buffer)?;
        self.counter.advance(read as u64, self.context);
        Ok(read)
    }
}

pub(crate) struct ObservedWriter<'emitter, 'observer, W> {
    inner: W,
    counter: ProgressCounter,
    context: &'emitter mut OperationContext<'observer>,
}

impl<'emitter, 'observer, W> ObservedWriter<'emitter, 'observer, W> {
    pub(crate) fn new(
        inner: W,
        phase: ProgressPhase,
        total_bytes: Option<u64>,
        context: &'emitter mut OperationContext<'observer>,
    ) -> Self {
        Self {
            inner,
            counter: ProgressCounter::new(phase, total_bytes, context),
            context,
        }
    }

    pub(crate) fn finish(&mut self) {
        self.counter.finish(self.context);
    }

    pub(crate) fn checkpoint(&self) -> OperationResult<()> {
        self.context.checkpoint()
    }
}

impl<W: Write> Write for ObservedWriter<'_, '_, W> {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        let written = self.inner.write(buffer)?;
        self.counter.advance(written as u64, self.context);
        Ok(written)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}
