//! Typed progress facts for application-service callers.

/// Operation producing a progress event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum OperationKind {
    Analyze,
    Compress,
    Decompress,
    Validate,
    Compare,
    Benchmark,
}

/// Stable phase names shared by path-based application operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ProgressPhase {
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
}

/// Lifecycle state for one progress phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ProgressState {
    Started,
    Advanced,
    Completed,
}

/// A privacy-safe progress snapshot.
///
/// Events never contain paths, field values, rows, hashes, or presentation
/// strings. Observers are called synchronously on the operation's calling
/// thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
}
