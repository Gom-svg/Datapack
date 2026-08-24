//! Explicit cooperative operation control.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use thiserror::Error;

use crate::error::{DatapackError, ErrorCategory};

use super::progress::{
    OperationKind, ProgressEmitter, ProgressObserver, ProgressPhase, ProgressState,
};

/// A cheap, cloneable, thread-safe cooperative cancellation request.
///
/// Cancellation is monotonic: once any clone requests cancellation, every
/// clone remains cancelled. A token may be deliberately shared across
/// operations; using an already-cancelled token pre-cancels the next operation.
#[derive(Clone, Debug, Default)]
pub struct CancellationToken {
    cancelled: Arc<AtomicBool>,
}

impl CancellationToken {
    /// Creates a token whose cancellation has not been requested.
    pub fn new() -> Self {
        Self::default()
    }

    /// Requests cooperative cancellation. Repeated calls are harmless.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }

    /// Returns whether cancellation has been requested through any clone.
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }

    pub(crate) fn checkpoint(&self) -> OperationResult<()> {
        if self.is_cancelled() {
            Err(OperationError::Cancelled)
        } else {
            Ok(())
        }
    }
}

/// Failure from a control-aware Application operation.
///
/// This additive error keeps cancellation distinct without adding a variant to
/// the existing exhaustively matchable [`DatapackError`].
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum OperationError {
    #[error("operation cancelled")]
    Cancelled,

    #[error(transparent)]
    Failed(#[from] DatapackError),
}

impl OperationError {
    /// Returns true only for the typed cooperative cancellation outcome.
    pub const fn is_cancelled(&self) -> bool {
        matches!(self, Self::Cancelled)
    }

    /// Returns a stable broad category without parsing the display message.
    pub const fn category(&self) -> ErrorCategory {
        match self {
            Self::Cancelled => ErrorCategory::Cancellation,
            Self::Failed(error) => error.category(),
        }
    }

    /// Returns the stable error code, delegating ordinary failures to
    /// [`DatapackError::code`].
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Cancelled => "cancelled",
            Self::Failed(error) => error.code(),
        }
    }
}

impl From<std::io::Error> for OperationError {
    fn from(error: std::io::Error) -> Self {
        Self::Failed(error.into())
    }
}

/// Result returned by control-aware Application operations.
pub type OperationResult<T> = std::result::Result<T, OperationError>;

/// Optional controls carried by one Application operation.
///
/// Progress remains observational. Cancellation remains explicit; callback
/// return values and callback failures never request cancellation.
pub struct OperationControl<'a> {
    observer: Option<&'a mut dyn ProgressObserver>,
    cancellation: Option<CancellationToken>,
}

impl OperationControl<'_> {
    /// Creates control with neither an observer nor a cancellation token.
    pub const fn new() -> Self {
        Self {
            observer: None,
            cancellation: None,
        }
    }
}

impl Default for OperationControl<'_> {
    fn default() -> Self {
        Self::new()
    }
}

impl<'a> OperationControl<'a> {
    /// Adds a synchronous progress observer.
    #[must_use]
    pub fn with_progress(mut self, observer: &'a mut dyn ProgressObserver) -> Self {
        self.observer = Some(observer);
        self
    }

    /// Adds an explicit cancellation token.
    #[must_use]
    pub fn with_cancellation(mut self, cancellation: CancellationToken) -> Self {
        self.cancellation = Some(cancellation);
        self
    }
}

pub(crate) struct OperationContext<'a> {
    emitter: ProgressEmitter<'a>,
    cancellation: Option<CancellationToken>,
}

impl<'a> OperationContext<'a> {
    pub(crate) fn silent(operation: OperationKind) -> Self {
        Self {
            emitter: ProgressEmitter::silent(operation),
            cancellation: None,
        }
    }

    pub(crate) fn observed(
        operation: OperationKind,
        observer: &'a mut dyn ProgressObserver,
    ) -> Self {
        Self {
            emitter: ProgressEmitter::observed(operation, observer),
            cancellation: None,
        }
    }

    pub(crate) fn controlled(operation: OperationKind, control: OperationControl<'a>) -> Self {
        let emitter = match control.observer {
            Some(observer) => ProgressEmitter::observed(operation, observer),
            None => ProgressEmitter::silent(operation),
        };
        Self {
            emitter,
            cancellation: control.cancellation,
        }
    }

    pub(crate) fn cancellation(&self) -> Option<&CancellationToken> {
        self.cancellation.as_ref()
    }

    pub(crate) fn checkpoint(&self) -> OperationResult<()> {
        match &self.cancellation {
            Some(cancellation) => cancellation.checkpoint(),
            None => Ok(()),
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
    ) -> OperationResult<()> {
        self.checkpoint()?;
        self.emitter.emit(
            phase,
            state,
            completed_bytes,
            total_bytes,
            completed_items,
            total_items,
        );
        self.checkpoint()
    }

    pub(crate) fn emit_unchecked(
        &mut self,
        phase: ProgressPhase,
        state: ProgressState,
        completed_bytes: u64,
        total_bytes: Option<u64>,
        completed_items: u64,
        total_items: Option<u64>,
    ) {
        self.emitter.emit(
            phase,
            state,
            completed_bytes,
            total_bytes,
            completed_items,
            total_items,
        );
    }

    pub(crate) fn started(
        &mut self,
        phase: ProgressPhase,
        total_bytes: Option<u64>,
    ) -> OperationResult<()> {
        self.emit(phase, ProgressState::Started, 0, total_bytes, 0, None)
    }

    pub(crate) fn started_with_items(
        &mut self,
        phase: ProgressPhase,
        total_bytes: Option<u64>,
        total_items: Option<u64>,
    ) -> OperationResult<()> {
        self.emit(
            phase,
            ProgressState::Started,
            0,
            total_bytes,
            0,
            total_items,
        )
    }

    pub(crate) fn completed(
        &mut self,
        phase: ProgressPhase,
        completed_bytes: u64,
        total_bytes: Option<u64>,
    ) -> OperationResult<()> {
        self.emit(
            phase,
            ProgressState::Completed,
            completed_bytes,
            total_bytes,
            0,
            None,
        )
    }

    /// Emits terminal success after the operation's logical/transactional
    /// commit point. A later cancellation request cannot revoke that success.
    pub(crate) fn succeeded(&mut self) {
        self.emitter.succeeded();
    }
}

pub(crate) fn uncontrolled<T>(result: OperationResult<T>) -> crate::error::Result<T> {
    match result {
        Ok(value) => Ok(value),
        Err(OperationError::Failed(error)) => Err(error),
        Err(OperationError::Cancelled) => {
            unreachable!("an operation without a cancellation token reported cancellation")
        }
    }
}
