#![forbid(unsafe_code)]

use std::path::PathBuf;

use datapack::application::{
    self, AnalyzeRequest, CancellationToken as RustCancellationToken, CompareMode, CompareRequest,
    CompressRequest, CompressionBackend, CompressionFormat, CompressionMode, DecompressRequest,
    OperationControl, OperationError, ProgressEvent, ProgressObserver, V1CompressionOptions,
    V2CompressionOptions, ValidateRequest,
};
use datapack::error::DatapackError as RustDataPackError;
use pyo3::create_exception;
use pyo3::exceptions::PyException;
use pyo3::prelude::*;
use pyo3::types::{PyModule, PyType};
use serde::Serialize;

create_exception!(
    datapack,
    DataPackError,
    PyException,
    "Base exception for DataPack SDK failures."
);
create_exception!(
    datapack,
    DataPackIOError,
    DataPackError,
    "A DataPack filesystem operation failed."
);
create_exception!(
    datapack,
    DataPackFormatError,
    DataPackError,
    "Input data or an archive had an invalid format."
);
create_exception!(
    datapack,
    DataPackConfigurationError,
    DataPackError,
    "A DataPack request option was invalid."
);
create_exception!(
    datapack,
    DataPackAnalysisError,
    DataPackError,
    "DataPack could not analyze the requested input."
);
create_exception!(
    datapack,
    DataPackOutputError,
    DataPackError,
    "DataPack could not safely create or replace the requested output."
);
create_exception!(
    datapack,
    DataPackOperationError,
    DataPackError,
    "A transactional DataPack operation failed."
);
create_exception!(
    datapack,
    DataPackTranslationError,
    DataPackError,
    "A Rust application report could not be translated to Python."
);
create_exception!(
    datapack,
    CancelledError,
    DataPackError,
    "A DataPack operation was cooperatively cancelled."
);

/// Thread-safe, monotonic cooperative cancellation control.
///
/// Call ``cancel()`` from the operation's progress callback or another Python
/// thread, then pass the token through an operation's ``cancellation=``
/// keyword. Cancellation is cooperative and cannot be reset.
#[pyclass(name = "CancellationToken", module = "datapack", frozen)]
struct PyCancellationToken {
    inner: RustCancellationToken,
}

#[pymethods]
impl PyCancellationToken {
    #[new]
    fn new() -> Self {
        Self {
            inner: RustCancellationToken::new(),
        }
    }

    fn cancel(&self) {
        self.inner.cancel();
    }

    #[getter]
    fn is_cancelled(&self) -> bool {
        self.inner.is_cancelled()
    }
}

/// Immutable progress facts emitted by the Rust Application API.
///
/// Byte and item totals are ``None`` when unknown. ``percentage`` is derived
/// by Rust from byte counters, and ``terminal`` is true only for successful
/// whole-operation completion.
#[pyclass(name = "ProgressEvent", module = "datapack", frozen)]
struct PyProgressEvent {
    #[pyo3(get)]
    operation: &'static str,
    #[pyo3(get)]
    stage: &'static str,
    #[pyo3(get)]
    state: &'static str,
    #[pyo3(get)]
    completed_bytes: u64,
    #[pyo3(get)]
    total_bytes: Option<u64>,
    #[pyo3(get)]
    completed_items: u64,
    #[pyo3(get)]
    total_items: Option<u64>,
    #[pyo3(get)]
    percentage: Option<f64>,
    #[pyo3(get)]
    terminal: bool,
}

impl From<&ProgressEvent> for PyProgressEvent {
    fn from(event: &ProgressEvent) -> Self {
        Self {
            operation: event.operation.as_str(),
            stage: event.phase.as_str(),
            state: event.state.as_str(),
            completed_bytes: event.completed_bytes,
            total_bytes: event.total_bytes,
            completed_items: event.completed_items,
            total_items: event.total_items,
            percentage: event.percentage(),
            terminal: event.is_terminal_success(),
        }
    }
}

struct PythonProgressObserver {
    callback: Py<PyAny>,
    callback_failed: bool,
}

impl PythonProgressObserver {
    fn new(callback: Py<PyAny>) -> Self {
        Self {
            callback,
            callback_failed: false,
        }
    }
}

impl ProgressObserver for PythonProgressObserver {
    fn on_event(&mut self, event: &ProgressEvent) {
        if self.callback_failed {
            return;
        }
        Python::attach(|py| {
            let result = Py::new(py, PyProgressEvent::from(event))
                .and_then(|event| self.callback.call1(py, (event,)));
            if let Err(error) = result {
                self.callback_failed = true;
                error.write_unraisable(py, Some(self.callback.bind(py)));
            }
        });
    }
}

#[pyclass(
    name = "V1CompressionOptions",
    module = "datapack",
    frozen,
    skip_from_py_object
)]
/// Immutable options for planner-selected DataPack v1 compression.
///
/// ``mode`` is ``"fast"`` or ``"best"``; ``sample_mb`` bounds analysis.
/// Dictionary value and memory limits are safety ceilings, and
/// ``verify_best`` explicitly materializes eligible candidates before keeping
/// the smaller valid archive.
struct PyV1CompressionOptions {
    #[pyo3(get)]
    mode: String,
    #[pyo3(get)]
    sample_mb: u64,
    #[pyo3(get)]
    verify_best: bool,
    #[pyo3(get)]
    max_dictionary_values: u64,
    #[pyo3(get)]
    max_dictionary_mb: u64,
    rust_mode: CompressionMode,
}

#[pymethods]
impl PyV1CompressionOptions {
    #[new]
    #[pyo3(signature = (
        *,
        mode = "fast",
        sample_mb = 64,
        verify_best = false,
        max_dictionary_values = 65_535,
        max_dictionary_mb = 64
    ))]
    fn new(
        mode: &str,
        sample_mb: u64,
        verify_best: bool,
        max_dictionary_values: u64,
        max_dictionary_mb: u64,
    ) -> PyResult<Self> {
        Ok(Self {
            mode: mode.to_string(),
            sample_mb,
            verify_best,
            max_dictionary_values,
            max_dictionary_mb,
            rust_mode: parse_compression_mode(mode)?,
        })
    }
}

impl PyV1CompressionOptions {
    fn as_rust_options(&self) -> V1CompressionOptions {
        let mut options = V1CompressionOptions::default();
        options.mode = self.rust_mode;
        options.sample_mb = self.sample_mb;
        options.verify_best = self.verify_best;
        options.max_dictionary_values = self.max_dictionary_values;
        options.max_dictionary_mb = self.max_dictionary_mb;
        options
    }
}

#[pyclass(
    name = "V2CompressionOptions",
    module = "datapack",
    frozen,
    skip_from_py_object
)]
/// Immutable options for bounded chunked RawZstd DataPack v2 compression.
///
/// Chunk size, worker count, and maximum in-flight chunks define the bounded
/// pipeline. ``max_memory_bytes`` is a preflight admission check for configured
/// in-flight chunk bytes, not a complete process-RSS ceiling.
struct PyV2CompressionOptions {
    #[pyo3(get)]
    chunk_size_bytes: usize,
    #[pyo3(get)]
    threads: usize,
    #[pyo3(get)]
    max_in_flight_chunks: usize,
    #[pyo3(get)]
    backend: String,
    #[pyo3(get)]
    adaptive_level: bool,
    #[pyo3(get)]
    max_memory_bytes: Option<u64>,
    rust_backend: CompressionBackend,
}

#[pymethods]
impl PyV2CompressionOptions {
    #[new]
    #[pyo3(signature = (
        *,
        chunk_size_bytes = None,
        threads = None,
        max_in_flight_chunks = None,
        backend = "chunked_raw_zstd",
        adaptive_level = false,
        max_memory_bytes = None
    ))]
    fn new(
        chunk_size_bytes: Option<usize>,
        threads: Option<usize>,
        max_in_flight_chunks: Option<usize>,
        backend: &str,
        adaptive_level: bool,
        max_memory_bytes: Option<u64>,
    ) -> PyResult<Self> {
        let defaults = V2CompressionOptions::default();
        Ok(Self {
            chunk_size_bytes: chunk_size_bytes.unwrap_or(defaults.chunk_size_bytes),
            threads: threads.unwrap_or(defaults.threads),
            max_in_flight_chunks: max_in_flight_chunks.unwrap_or(defaults.max_in_flight_chunks),
            backend: backend.to_string(),
            adaptive_level,
            max_memory_bytes,
            rust_backend: parse_compression_backend(backend)?,
        })
    }
}

impl PyV2CompressionOptions {
    fn as_rust_options(&self) -> V2CompressionOptions {
        let mut options = V2CompressionOptions::default();
        options.chunk_size_bytes = self.chunk_size_bytes;
        options.threads = self.threads;
        options.max_in_flight_chunks = self.max_in_flight_chunks;
        options.backend = self.rust_backend;
        options.adaptive_level = self.adaptive_level;
        options.max_memory_bytes = self.max_memory_bytes;
        options
    }
}

/// Analyze bounded structured facts and planner advice for a path.
///
/// Args:
///     input: Source path accepted as ``str`` or ``os.PathLike``.
///     sample_mb: Maximum analysis byte sample in MiB; other safety bounds can
///         still make the report partial.
///     progress: Optional synchronous callable receiving ``ProgressEvent``.
///     cancellation: Optional explicit cooperative cancellation token.
///
/// Returns:
///     An ``AnalysisReport`` dictionary. Inspect ``sampling`` for actual scope,
///     completeness, bytes/records analyzed, and any reached limit.
///
/// Raises:
///     DataPackAnalysisError: If the source cannot be analyzed.
///     DataPackConfigurationError: If an argument is invalid.
///     CancelledError: If explicit cancellation is accepted.
#[pyfunction]
#[pyo3(signature = (input, *, sample_mb = 64, progress = None, cancellation = None))]
fn analyze(
    py: Python<'_>,
    input: PathBuf,
    sample_mb: u64,
    progress: Option<Py<PyAny>>,
    cancellation: Option<Py<PyCancellationToken>>,
) -> PyResult<Py<PyAny>> {
    let mut request = AnalyzeRequest::new(input);
    request.sample_mb = sample_mb;
    let controlled_request = request.clone();
    let cancellation = rust_cancellation(py, cancellation);
    service_report_with_optional_control(
        py,
        "analyze",
        progress,
        cancellation,
        move || application::analyze(request),
        move |control| application::analyze_with_control(controlled_request, control),
    )
}

/// Compress a source path into a transactional DataPack archive.
///
/// Args:
///     input: Source path accepted as ``str`` or ``os.PathLike``.
///     output: Destination ``.dpack`` path.
///     options: ``V1CompressionOptions``, ``V2CompressionOptions``, or ``None``
///         for Rust's safe v1 defaults.
///     overwrite: Replace an existing regular destination only when true.
///     keep_partial: Retain the owned sibling ``.partial`` after failure.
///     progress: Optional synchronous callable receiving ``ProgressEvent``.
///     cancellation: Optional explicit cooperative cancellation token.
///
/// Returns:
///     A ``CompressionResult`` dictionary containing archive version, selected
///     mode/backend, byte counts, diagnostics, and profiling facts.
///
/// Raises:
///     DataPackError: If input, configuration, or transactional output fails.
///     CancelledError: If explicit cancellation wins before commit.
#[pyfunction]
#[allow(clippy::too_many_arguments)]
#[pyo3(signature = (
    input,
    output,
    *,
    options = None,
    overwrite = false,
    keep_partial = false,
    progress = None,
    cancellation = None
))]
fn compress(
    py: Python<'_>,
    input: PathBuf,
    output: PathBuf,
    options: Option<&Bound<'_, PyAny>>,
    overwrite: bool,
    keep_partial: bool,
    progress: Option<Py<PyAny>>,
    cancellation: Option<Py<PyCancellationToken>>,
) -> PyResult<Py<PyAny>> {
    let mut request = CompressRequest::new(input, output);
    request.format = compression_format(options)?;
    request.overwrite = overwrite;
    request.keep_partial = keep_partial;
    let controlled_request = request.clone();
    let cancellation = rust_cancellation(py, cancellation);
    service_report_with_optional_control(
        py,
        "compress",
        progress,
        cancellation,
        move || application::compress(request),
        move |control| application::compress_with_control(controlled_request, control),
    )
}

/// Decompress a DataPack archive into a transactional restored output.
///
/// Args:
///     archive: Source ``.dpack`` path.
///     output: Destination path for restored bytes.
///     verify: Verify v2 chunk/global hashes; enabled by default.
///     max_output_bytes: Optional declared restored-size safety limit.
///     max_chunks: Optional v2 chunk-table safety limit.
///     max_memory_bytes: Optional approximate working-memory admission limit.
///     overwrite: Replace an existing regular destination only when true.
///     keep_partial: Retain the owned sibling ``.partial`` after failure.
///     progress: Optional synchronous callable receiving ``ProgressEvent``.
///     cancellation: Optional explicit cooperative cancellation token.
///
/// Returns:
///     A ``DecompressionResult`` dictionary containing archive/mode/backend,
///     archive/restored byte counts, verification, diagnostics, and profile.
///
/// Raises:
///     DataPackError: If parsing, limits, verification, or output fails.
///     CancelledError: If explicit cancellation wins before commit.
#[pyfunction]
#[allow(clippy::too_many_arguments)]
#[pyo3(signature = (
    archive,
    output,
    *,
    verify = true,
    max_output_bytes = None,
    max_chunks = None,
    max_memory_bytes = None,
    overwrite = false,
    keep_partial = false,
    progress = None,
    cancellation = None
))]
fn decompress(
    py: Python<'_>,
    archive: PathBuf,
    output: PathBuf,
    verify: bool,
    max_output_bytes: Option<u64>,
    max_chunks: Option<u64>,
    max_memory_bytes: Option<u64>,
    overwrite: bool,
    keep_partial: bool,
    progress: Option<Py<PyAny>>,
    cancellation: Option<Py<PyCancellationToken>>,
) -> PyResult<Py<PyAny>> {
    let mut request = DecompressRequest::new(archive, output);
    request.verify = verify;
    request.max_output_bytes = max_output_bytes;
    request.max_chunks = max_chunks;
    request.max_memory_bytes = max_memory_bytes;
    request.overwrite = overwrite;
    request.keep_partial = keep_partial;
    let controlled_request = request.clone();
    let cancellation = rust_cancellation(py, cancellation);
    service_report_with_optional_control(
        py,
        "decompress",
        progress,
        cancellation,
        move || application::decompress(request),
        move |control| application::decompress_with_control(controlled_request, control),
    )
}

/// Validate an archive without publishing a restored output.
///
/// Args:
///     archive: Source ``.dpack`` path.
///     against: Optional original source for full restored-identity comparison.
///     max_output_bytes: Optional declared restored-size safety limit.
///     max_chunks: Optional v2 chunk-table safety limit.
///     max_memory_bytes: Optional validation working-memory bound; ``None`` uses
///         Rust's 512 MiB default.
///     progress: Optional synchronous callable receiving ``ProgressEvent``.
///     cancellation: Optional explicit cooperative cancellation token.
///
/// Returns:
///     A ``ValidationReport`` dictionary. A readable but invalid/mismatched
///     archive returns ``valid=False`` with check and diagnostic codes.
///
/// Raises:
///     DataPackError: If validation cannot be performed at all.
///     CancelledError: If explicit cancellation is accepted.
#[pyfunction]
#[allow(clippy::too_many_arguments)]
#[pyo3(signature = (
    archive,
    *,
    against = None,
    max_output_bytes = None,
    max_chunks = None,
    max_memory_bytes = None,
    progress = None,
    cancellation = None
))]
fn validate(
    py: Python<'_>,
    archive: PathBuf,
    against: Option<PathBuf>,
    max_output_bytes: Option<u64>,
    max_chunks: Option<u64>,
    max_memory_bytes: Option<u64>,
    progress: Option<Py<PyAny>>,
    cancellation: Option<Py<PyCancellationToken>>,
) -> PyResult<Py<PyAny>> {
    let mut request = ValidateRequest::new(archive);
    request.against = against;
    request.max_output_bytes = max_output_bytes;
    request.max_chunks = max_chunks;
    if let Some(value) = max_memory_bytes {
        request.max_memory_bytes = value;
    }
    let controlled_request = request.clone();
    let cancellation = rust_cancellation(py, cancellation);
    service_report_with_optional_control(
        py,
        "validate",
        progress,
        cancellation,
        move || application::validate(request),
        move |control| application::validate_with_control(controlled_request, control),
    )
}

/// Compare DataPack v1 with standalone zstd over a documented scope.
///
/// Args:
///     input: Source path accepted as ``str`` or ``os.PathLike``.
///     mode: ``"quick"`` for a bounded prefix or ``"full"`` for all bytes.
///     runs: Timing runs from 1 through 25.
///     max_input_mb: Optional Quick-mode prefix bound in MiB.
///     progress: Optional synchronous callable receiving ``ProgressEvent``.
///     cancellation: Optional explicit cooperative cancellation token.
///
/// Returns:
///     A ``ComparisonReport`` dictionary. Winners/differences are successful
///     report facts; an inability to compare raises ``DataPackError``.
///
/// Raises:
///     DataPackConfigurationError: If mode, runs, or scope is invalid.
///     DataPackError: If the comparison operation fails.
///     CancelledError: If explicit cancellation is accepted.
#[pyfunction]
#[pyo3(signature = (
    input,
    *,
    mode = "quick",
    runs = 3,
    max_input_mb = None,
    progress = None,
    cancellation = None
))]
fn compare(
    py: Python<'_>,
    input: PathBuf,
    mode: &str,
    runs: usize,
    max_input_mb: Option<u64>,
    progress: Option<Py<PyAny>>,
    cancellation: Option<Py<PyCancellationToken>>,
) -> PyResult<Py<PyAny>> {
    let mut request = CompareRequest::new(input);
    request.mode = parse_compare_mode(mode)?;
    request.runs = runs;
    request.max_input_mb = max_input_mb;
    let controlled_request = request.clone();
    let cancellation = rust_cancellation(py, cancellation);
    service_report_with_optional_control(
        py,
        "compare",
        progress,
        cancellation,
        move || application::compare(request),
        move |control| application::compare_with_control(controlled_request, control),
    )
}

fn compression_format(options: Option<&Bound<'_, PyAny>>) -> PyResult<CompressionFormat> {
    let Some(options) = options else {
        return Ok(CompressionFormat::V1(V1CompressionOptions::default()));
    };
    if let Ok(options) = options.extract::<PyRef<'_, PyV1CompressionOptions>>() {
        return Ok(CompressionFormat::V1(options.as_rust_options()));
    }
    if let Ok(options) = options.extract::<PyRef<'_, PyV2CompressionOptions>>() {
        return Ok(CompressionFormat::V2(options.as_rust_options()));
    }
    Err(DataPackConfigurationError::new_err(
        "compress request rejected: options must be V1CompressionOptions or V2CompressionOptions",
    ))
}

fn service_report<T, F>(py: Python<'_>, operation: &'static str, service: F) -> PyResult<Py<PyAny>>
where
    T: Serialize,
    F: FnOnce() -> datapack::error::Result<T> + Send + 'static,
{
    let serialized = py.detach(move || {
        let report = service().map_err(|error| service_failure(operation, error))?;
        serde_json::to_string(&report).map_err(|error| ServiceFailure {
            kind: PythonErrorKind::Translation,
            message: format!("{operation} result translation failed: {error}"),
        })
    });
    let serialized = serialized.map_err(ServiceFailure::into_pyerr)?;
    json_report(py, &serialized, operation)
}

fn service_report_with_optional_control<T, F, P>(
    py: Python<'_>,
    operation: &'static str,
    progress: Option<Py<PyAny>>,
    cancellation: Option<RustCancellationToken>,
    service: F,
    service_with_control: P,
) -> PyResult<Py<PyAny>>
where
    T: Serialize,
    F: FnOnce() -> datapack::error::Result<T> + Send + 'static,
    P: FnOnce(OperationControl<'_>) -> application::OperationResult<T> + Send + 'static,
{
    if progress.is_none() && cancellation.is_none() {
        return service_report(py, operation, service);
    }
    if progress
        .as_ref()
        .is_some_and(|callback| !callback.bind(py).is_callable())
    {
        return Err(DataPackConfigurationError::new_err(format!(
            "{operation} request rejected: progress must be callable"
        )));
    }

    let serialized = py.detach(move || {
        let mut observer = progress.map(PythonProgressObserver::new);
        let mut control = OperationControl::new();
        if let Some(cancellation) = cancellation {
            control = control.with_cancellation(cancellation);
        }
        if let Some(observer) = observer.as_mut() {
            control = control.with_progress(observer);
        }
        let report = service_with_control(control)
            .map_err(|error| controlled_service_failure(operation, error))?;
        serde_json::to_string(&report).map_err(|error| ServiceFailure {
            kind: PythonErrorKind::Translation,
            message: format!("{operation} result translation failed: {error}"),
        })
    });
    let serialized = serialized.map_err(ServiceFailure::into_pyerr)?;
    json_report(py, &serialized, operation)
}

fn json_report(py: Python<'_>, serialized: &str, operation: &'static str) -> PyResult<Py<PyAny>> {
    let json = PyModule::import(py, "json").map_err(|error| {
        DataPackTranslationError::new_err(format!(
            "{operation} result translation failed while importing json: {error}"
        ))
    })?;
    let report = json.call_method1("loads", (serialized,)).map_err(|error| {
        DataPackTranslationError::new_err(format!(
            "{operation} result translation failed while decoding JSON: {error}"
        ))
    })?;
    Ok(report.unbind())
}

#[derive(Clone, Copy)]
enum PythonErrorKind {
    Cancelled,
    Io,
    Format,
    Configuration,
    Analysis,
    Output,
    Operation,
    Translation,
}

struct ServiceFailure {
    kind: PythonErrorKind,
    message: String,
}

impl ServiceFailure {
    fn into_pyerr(self) -> PyErr {
        match self.kind {
            PythonErrorKind::Cancelled => CancelledError::new_err(self.message),
            PythonErrorKind::Io => DataPackIOError::new_err(self.message),
            PythonErrorKind::Format => DataPackFormatError::new_err(self.message),
            PythonErrorKind::Configuration => DataPackConfigurationError::new_err(self.message),
            PythonErrorKind::Analysis => DataPackAnalysisError::new_err(self.message),
            PythonErrorKind::Output => DataPackOutputError::new_err(self.message),
            PythonErrorKind::Operation => DataPackOperationError::new_err(self.message),
            PythonErrorKind::Translation => DataPackTranslationError::new_err(self.message),
        }
    }
}

fn controlled_service_failure(operation: &'static str, error: OperationError) -> ServiceFailure {
    match error {
        OperationError::Cancelled => ServiceFailure {
            kind: PythonErrorKind::Cancelled,
            message: format!("{operation} cancelled"),
        },
        OperationError::Failed(error) => service_failure(operation, error),
        other => ServiceFailure {
            kind: PythonErrorKind::Operation,
            message: format!("{operation} failed: {other}"),
        },
    }
}

fn rust_cancellation(
    py: Python<'_>,
    cancellation: Option<Py<PyCancellationToken>>,
) -> Option<RustCancellationToken> {
    cancellation.map(|token| token.borrow(py).inner.clone())
}

fn service_failure(operation: &'static str, error: RustDataPackError) -> ServiceFailure {
    let kind = match &error {
        RustDataPackError::Io(_) => PythonErrorKind::Io,
        RustDataPackError::Bincode(_)
        | RustDataPackError::InvalidFormat(_)
        | RustDataPackError::InvalidCsv(_)
        | RustDataPackError::ArchiveParse { .. } => PythonErrorKind::Format,
        RustDataPackError::InvalidProfile(_) | RustDataPackError::RowsOutOfRange(_) => {
            PythonErrorKind::Configuration
        }
        RustDataPackError::OutputNotWritable(_) => PythonErrorKind::Output,
        RustDataPackError::AnalyzeRead(_) => PythonErrorKind::Analysis,
        RustDataPackError::OperationFailed { .. } => PythonErrorKind::Operation,
    };
    ServiceFailure {
        kind,
        message: format!("{operation} failed: {error}"),
    }
}

fn parse_compression_mode(value: &str) -> PyResult<CompressionMode> {
    match value {
        "fast" => Ok(CompressionMode::Fast),
        "best" => Ok(CompressionMode::Best),
        _ => Err(DataPackConfigurationError::new_err(format!(
            "compress request rejected: mode must be 'fast' or 'best', got {value:?}"
        ))),
    }
}

fn parse_compression_backend(value: &str) -> PyResult<CompressionBackend> {
    match value {
        "chunked_raw_zstd" => Ok(CompressionBackend::ChunkedRawZstd),
        "zstd_mt_experimental" => Ok(CompressionBackend::ZstdMtExperimental),
        _ => Err(DataPackConfigurationError::new_err(format!(
            "compress request rejected: backend must be 'chunked_raw_zstd' or 'zstd_mt_experimental', got {value:?}"
        ))),
    }
}

fn parse_compare_mode(value: &str) -> PyResult<CompareMode> {
    match value {
        "quick" => Ok(CompareMode::Quick),
        "full" => Ok(CompareMode::Full),
        _ => Err(DataPackConfigurationError::new_err(format!(
            "compare request rejected: mode must be 'quick' or 'full', got {value:?}"
        ))),
    }
}

fn add_exception_type(
    module: &Bound<'_, PyModule>,
    name: &str,
    exception: Bound<'_, PyType>,
    category: &'static str,
    code: &'static str,
) -> PyResult<()> {
    exception.setattr("category", category)?;
    exception.setattr("code", code)?;
    module.add(name, exception)
}

#[pymodule(gil_used = false)]
fn _native(module: &Bound<'_, PyModule>) -> PyResult<()> {
    let py = module.py();
    add_exception_type(
        module,
        "DataPackError",
        py.get_type::<DataPackError>(),
        "datapack",
        "datapack_error",
    )?;
    add_exception_type(
        module,
        "DataPackIOError",
        py.get_type::<DataPackIOError>(),
        "io",
        "io_error",
    )?;
    add_exception_type(
        module,
        "DataPackFormatError",
        py.get_type::<DataPackFormatError>(),
        "format",
        "format_error",
    )?;
    add_exception_type(
        module,
        "DataPackConfigurationError",
        py.get_type::<DataPackConfigurationError>(),
        "configuration",
        "configuration_error",
    )?;
    add_exception_type(
        module,
        "DataPackAnalysisError",
        py.get_type::<DataPackAnalysisError>(),
        "analysis",
        "analysis_error",
    )?;
    add_exception_type(
        module,
        "DataPackOutputError",
        py.get_type::<DataPackOutputError>(),
        "output",
        "output_error",
    )?;
    add_exception_type(
        module,
        "DataPackOperationError",
        py.get_type::<DataPackOperationError>(),
        "operation",
        "operation_error",
    )?;
    add_exception_type(
        module,
        "DataPackTranslationError",
        py.get_type::<DataPackTranslationError>(),
        "translation",
        "translation_error",
    )?;
    add_exception_type(
        module,
        "CancelledError",
        py.get_type::<CancelledError>(),
        "cancellation",
        "cancelled",
    )?;
    module.add_class::<PyCancellationToken>()?;
    module.add_class::<PyV1CompressionOptions>()?;
    module.add_class::<PyV2CompressionOptions>()?;
    module.add_class::<PyProgressEvent>()?;
    module.add_function(wrap_pyfunction!(analyze, module)?)?;
    module.add_function(wrap_pyfunction!(compress, module)?)?;
    module.add_function(wrap_pyfunction!(decompress, module)?)?;
    module.add_function(wrap_pyfunction!(validate, module)?)?;
    module.add_function(wrap_pyfunction!(compare, module)?)?;
    Ok(())
}
