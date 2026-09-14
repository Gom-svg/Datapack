//! The only Desktop module that executes engine services. No codecs or CLI parsing.
use std::path::PathBuf;
use std::sync::{mpsc, Arc, Mutex};
use std::thread::{self, JoinHandle};

use datapack::application::{
    self as api, CancellationToken, OperationControl, OperationError, ProgressEvent,
};

use crate::model::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CompressionChoice {
    #[default]
    Recommended,
    Chunked,
}

#[derive(Debug, Clone)]
pub enum Job {
    Select {
        path: PathBuf,
        kind: FileKind,
    },
    Analyze {
        input: PathBuf,
    },
    Compress {
        input: PathBuf,
        output: PathBuf,
        choice: CompressionChoice,
    },
    Validate {
        archive: PathBuf,
        against: Option<PathBuf>,
    },
    Decompress {
        archive: PathBuf,
        output: PathBuf,
    },
}

impl Job {
    pub fn operation(&self) -> Operation {
        match self {
            Self::Select { .. } => Operation::Select,
            Self::Analyze { .. } => Operation::Analyze,
            Self::Compress { .. } => Operation::Compress,
            Self::Validate { .. } => Operation::Validate,
            Self::Decompress { .. } => Operation::Decompress,
        }
    }
}

#[derive(Debug)]
pub enum Outcome {
    Selected(SelectedFile),
    Analyzed(Analysis),
    Compressed(FileResult),
    Validated(Validation),
    Decompressed(FileResult),
}

pub fn execute(
    job: Job,
    token: CancellationToken,
    observer: &mut dyn api::ProgressObserver,
) -> api::OperationResult<Outcome> {
    let control = OperationControl::new()
        .with_cancellation(token.clone())
        .with_progress(observer);
    match job {
        Job::Select { path, kind } => {
            if token.is_cancelled() {
                return Err(OperationError::Cancelled);
            }
            let file = SelectedFile::inspect(path, kind)?;
            if token.is_cancelled() {
                return Err(OperationError::Cancelled);
            }
            Ok(Outcome::Selected(file))
        }
        Job::Analyze { input } => {
            api::analyze_with_control(api::AnalyzeRequest::new(input), control)
                .map(|report| Outcome::Analyzed(report.into()))
        }
        Job::Compress {
            input,
            output,
            choice,
        } => {
            // No Desktop overwrite or keep-partial switches. Engine transaction semantics apply.
            let mut request = api::CompressRequest::new(input, &output);
            if choice == CompressionChoice::Chunked {
                let mut options = api::V2CompressionOptions::default();
                options.chunk_size_bytes = 8 * 1024 * 1024;
                options.threads = 2;
                options.max_in_flight_chunks = 2;
                options.max_memory_bytes = Some(16 * 1024 * 1024);
                request.format = api::CompressionFormat::V2(options);
            }
            api::compress_with_control(request, control).map(|report| {
                Outcome::Compressed(FileResult {
                    output,
                    input_bytes: report.input_size_bytes,
                    output_bytes: report.archive_size_bytes,
                    version: report.archive_version,
                    method: method(report.selected_mode),
                    integrity:
                        "Archive created. Validate against the original to verify exact bytes.",
                    details: report
                        .diagnostics
                        .into_iter()
                        .map(|d| format!("{}: {}", d.code, d.message))
                        .collect(),
                })
            })
        }
        Job::Validate { archive, against } => {
            let mut request = api::ValidateRequest::new(archive);
            request.against = against;
            api::validate_with_control(request, control)
                .map(|report| Outcome::Validated(report.into()))
        }
        Job::Decompress { archive, output } => {
            let mut request = api::DecompressRequest::new(archive, &output);
            request.max_memory_bytes = Some(api::DEFAULT_VALIDATION_MEMORY_BYTES);
            api::decompress_with_control(request, control).map(|report| Outcome::Decompressed(FileResult {
                output, input_bytes: report.archive_size_bytes, output_bytes: report.restored_size_bytes,
                version: report.archive_version, method: method(report.selected_mode),
                integrity: match report.verified {
                    Some(true) => "Exact restoration verified using archive SHA-256.",
                    Some(false) => "Restored without hash verification.",
                    None => "Restored. This archive has no embedded SHA-256; validate against the original to verify exact bytes.",
                },
                details: report.diagnostics.into_iter().map(|d| format!("{}: {}", d.code, d.message)).collect(),
            }))
        }
    }
}

#[derive(Debug)]
enum Completion {
    Finished(api::OperationResult<Outcome>),
    Panicked,
}

struct Worker {
    id: u64,
    operation: Operation,
    token: CancellationToken,
    progress: Arc<Mutex<Option<Progress>>>,
    completion: mpsc::Receiver<(u64, Completion)>,
    thread: JoinHandle<()>,
}

/// One active worker; one coalesced progress snapshot; one completion message.
/// UI polling uses try_lock/try_recv and never waits for engine work.
pub struct Controller {
    pub state: State,
    pub selected: Option<SelectedFile>,
    pub analysis: Option<Analysis>,
    pub progress: Option<Progress>,
    worker: Option<Worker>,
    next_id: u64,
}

impl Default for Controller {
    fn default() -> Self {
        Self {
            state: State::Idle,
            selected: None,
            analysis: None,
            progress: None,
            worker: None,
            next_id: 0,
        }
    }
}

impl Controller {
    pub fn busy(&self) -> bool {
        self.worker.is_some()
    }

    pub fn operation(&self) -> Option<Operation> {
        self.worker.as_ref().map(|worker| worker.operation)
    }

    pub fn start(&mut self, job: Job, wake: Arc<dyn Fn() + Send + Sync>) -> Result<(), Problem> {
        if self.busy() {
            return Err(Problem::desktop(
                "desktop_busy",
                "Wait for the current operation to finish.",
            ));
        }
        let id = self.next_id;
        self.next_id = self
            .next_id
            .checked_add(1)
            .expect("desktop operation id exhausted");
        let operation = job.operation();
        let token = CancellationToken::new();
        let thread_token = token.clone();
        let progress = Arc::new(Mutex::new(None));
        let thread_progress = progress.clone();
        let (sender, completion) = mpsc::sync_channel(1);
        let thread = thread::Builder::new()
            .name("datapack-operation".into())
            .spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    execute(job, thread_token, &mut |event: &ProgressEvent| {
                        if let Ok(mut slot) = thread_progress.lock() {
                            *slot = Some(event.into());
                        }
                        wake();
                    })
                }));
                let completion = match result {
                    Ok(result) => Completion::Finished(result),
                    Err(_) => Completion::Panicked,
                };
                let _ = sender.send((id, completion));
                wake();
            })
            .map_err(|_| {
                Problem::desktop(
                    "desktop_worker_start",
                    "DataPack could not start a worker. Close other applications and retry.",
                )
            })?;
        if operation == Operation::Select {
            self.selected = None;
            self.analysis = None;
        }
        self.progress = None;
        self.state = match operation {
            Operation::Select => State::Selecting,
            Operation::Analyze => State::Analyzing,
            Operation::Compress => State::Compressing,
            Operation::Validate => State::Validating,
            Operation::Decompress => State::Decompressing,
        };
        self.worker = Some(Worker {
            id,
            operation,
            token,
            progress,
            completion,
            thread,
        });
        Ok(())
    }

    pub fn cancel(&mut self) {
        if let Some(worker) = &self.worker {
            worker.token.cancel();
            self.state = State::Cancelling(worker.operation);
        }
    }

    pub fn poll(&mut self) {
        let Some(worker) = &self.worker else {
            return;
        };
        if let Ok(mut slot) = worker.progress.try_lock() {
            if let Some(progress) = slot.take() {
                self.progress = Some(progress);
            }
        }
        // Receiving a result is insufficient to join: the final wake may still be running.
        if !worker.thread.is_finished() {
            return;
        }
        let completion = worker.completion.try_recv().ok();
        let worker = self.worker.take().expect("worker exists");
        let _ = worker.thread.join();
        match completion {
            Some((id, completion)) if id == worker.id => self.finish(worker.operation, completion),
            _ => {
                self.state = State::Failed(Problem::desktop(
                    "desktop_worker_lost",
                    "The worker stopped unexpectedly. Inspect the destination before retrying.",
                ))
            }
        }
    }

    fn finish(&mut self, operation: Operation, completion: Completion) {
        self.progress = None;
        self.state = match completion {
            // The returned outcome is authoritative, including success after a late cancel.
            Completion::Finished(Ok(Outcome::Selected(file))) => {
                self.selected = Some(file);
                State::FileSelected
            }
            Completion::Finished(Ok(Outcome::Analyzed(analysis))) => {
                self.analysis = Some(analysis);
                State::AnalysisReady
            }
            Completion::Finished(Ok(Outcome::Compressed(result))) => {
                State::CompressComplete(result)
            }
            Completion::Finished(Ok(Outcome::Validated(result))) => {
                State::ValidationComplete(result)
            }
            Completion::Finished(Ok(Outcome::Decompressed(result))) => {
                State::DecompressComplete(result)
            }
            Completion::Finished(Err(error)) if error.is_cancelled() => State::Cancelled(operation),
            Completion::Finished(Err(error)) => State::Failed(Problem::from_engine(&error)),
            Completion::Panicked => State::Failed(Problem::desktop(
                "desktop_worker_panic",
                "The operation stopped unexpectedly. Inspect the destination before retrying.",
            )),
        };
    }
}

impl Drop for Controller {
    fn drop(&mut self) {
        // Normal app close waits in the responsive UI until poll sees completion.
        // Never terminate an engine worker. An abnormal owner drop requests cancellation.
        if let Some(worker) = &self.worker {
            worker.token.cancel();
        }
    }
}
