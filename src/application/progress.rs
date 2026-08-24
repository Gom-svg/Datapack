//! Typed progress facts for application-service callers.

use serde::Serialize;

/// Operation producing a progress event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum OperationKind {
    Analyze,
    Compress,
    Decompress,
    Validate,
    Compare,
    Benchmark,
}

impl OperationKind {
    /// Stable presentation-neutral identifier for adapters and event streams.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Analyze => "analyze",
            Self::Compress => "compress",
            Self::Decompress => "decompress",
            Self::Validate => "validate",
            Self::Compare => "compare",
            Self::Benchmark => "benchmark",
        }
    }
}

/// Stable phase names shared by path-based application operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ProgressPhase {
    Analyzing,
    Planning,
    ReadingInput,
    Hashing,
    Compressing,
    WritingArchive,
    ReadingArchive,
    Decompressing,
    WritingOutput,
    Validating,
    Comparing,
    Benchmarking,
    CleaningUp,
    /// The operation has produced and, where applicable, committed its result.
    Finalizing,
}

impl ProgressPhase {
    /// Stable presentation-neutral identifier for adapters and event streams.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Analyzing => "analyzing",
            Self::Planning => "planning",
            Self::ReadingInput => "reading_input",
            Self::Hashing => "hashing",
            Self::Compressing => "compressing",
            Self::WritingArchive => "writing_archive",
            Self::ReadingArchive => "reading_archive",
            Self::Decompressing => "decompressing",
            Self::WritingOutput => "writing_output",
            Self::Validating => "validating",
            Self::Comparing => "comparing",
            Self::Benchmarking => "benchmarking",
            Self::CleaningUp => "cleaning_up",
            Self::Finalizing => "finalizing",
        }
    }
}

/// Lifecycle state for one progress phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ProgressState {
    Started,
    Advanced,
    Completed,
}

impl ProgressState {
    /// Stable presentation-neutral identifier for adapters and event streams.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Started => "started",
            Self::Advanced => "advanced",
            Self::Completed => "completed",
        }
    }
}

/// A privacy-safe progress snapshot.
///
/// Events never contain paths, field values, rows, hashes, or presentation
/// strings. Observers are called synchronously on the operation's calling
/// thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct ProgressEvent {
    pub operation: OperationKind,
    pub phase: ProgressPhase,
    pub state: ProgressState,
    pub completed_bytes: u64,
    pub total_bytes: Option<u64>,
    pub completed_items: u64,
    pub total_items: Option<u64>,
}

impl ProgressEvent {
    /// Derives percentage directly from integer byte counters.
    ///
    /// Unknown totals and an in-progress zero-byte scope have no percentage.
    /// A successfully completed zero-byte phase is exactly 100 percent.
    pub fn percentage(self) -> Option<f64> {
        match self.total_bytes {
            None => None,
            Some(0) if self.state == ProgressState::Completed => Some(100.0),
            Some(0) => None,
            Some(total) => Some((self.completed_bytes as f64 / total as f64 * 100.0).min(100.0)),
        }
    }

    /// True only for the terminal event of a successfully returned operation.
    pub const fn is_terminal_success(self) -> bool {
        matches!(self.phase, ProgressPhase::Finalizing)
            && matches!(self.state, ProgressState::Completed)
    }
}

/// Receives synchronous progress facts from an application operation.
pub trait ProgressObserver {
    fn on_event(&mut self, event: &ProgressEvent);
}

impl<F> ProgressObserver for F
where
    F: FnMut(&ProgressEvent),
{
    fn on_event(&mut self, event: &ProgressEvent) {
        self(event);
    }
}

pub(crate) struct ProgressEmitter<'a> {
    operation: OperationKind,
    observer: Option<&'a mut dyn ProgressObserver>,
}

impl<'a> ProgressEmitter<'a> {
    pub(crate) fn silent(operation: OperationKind) -> Self {
        Self {
            operation,
            observer: None,
        }
    }

    pub(crate) fn observed(
        operation: OperationKind,
        observer: &'a mut dyn ProgressObserver,
    ) -> Self {
        Self {
            operation,
            observer: Some(observer),
        }
    }

    pub(crate) fn emit(
        &mut self,
        phase: ProgressPhase,
        state: ProgressState,
        completed_bytes: u64,
        total_bytes: Option<u64>,
        completed_items: u64,
        total_items: Option<u64>,
    ) {
        // A file can change after metadata is read. A stale total becomes
        // unknown rather than allowing the public contract to report work
        // beyond that total.
        let total_bytes = total_bytes.filter(|total| completed_bytes <= *total);
        let total_items = total_items.filter(|total| completed_items <= *total);
        if let Some(observer) = self.observer.as_deref_mut() {
            observer.on_event(&ProgressEvent {
                operation: self.operation,
                phase,
                state,
                completed_bytes,
                total_bytes,
                completed_items,
                total_items,
            });
        }
    }

    pub(crate) fn started(&mut self, phase: ProgressPhase, total_bytes: Option<u64>) {
        self.emit(phase, ProgressState::Started, 0, total_bytes, 0, None);
    }

    pub(crate) fn completed(
        &mut self,
        phase: ProgressPhase,
        completed_bytes: u64,
        total_bytes: Option<u64>,
    ) {
        self.emit(
            phase,
            ProgressState::Completed,
            completed_bytes,
            total_bytes,
            0,
            None,
        );
    }

    /// Emits the only event that means the complete operation succeeded.
    pub(crate) fn succeeded(&mut self) {
        self.started(ProgressPhase::Finalizing, None);
        self.completed(ProgressPhase::Finalizing, 0, None);
    }
}
