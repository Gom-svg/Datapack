#![forbid(unsafe_code)]

use std::path::PathBuf;

use datapack::application::{
    self, AnalyzeRequest, CompareMode, CompareRequest, CompressRequest, CompressionBackend,
    CompressionFormat, CompressionMode, DecompressRequest, V1CompressionOptions,
    V2CompressionOptions, ValidateRequest,
};
use datapack::error::DatapackError as RustDataPackError;
use pyo3::create_exception;
use pyo3::exceptions::PyException;
use pyo3::prelude::*;
use pyo3::types::PyModule;
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

#[pyclass(
    name = "V1CompressionOptions",
    module = "datapack",
    frozen,
    skip_from_py_object
)]
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

#[pyfunction]
#[pyo3(signature = (input, *, sample_mb = 64))]
fn analyze(py: Python<'_>, input: PathBuf, sample_mb: u64) -> PyResult<Py<PyAny>> {
    let mut request = AnalyzeRequest::new(input);
    request.sample_mb = sample_mb;
    service_report(py, "analyze", move || application::analyze(request))
}

#[pyfunction]
#[pyo3(signature = (input, output, *, options = None, overwrite = false, keep_partial = false))]
fn compress(
    py: Python<'_>,
    input: PathBuf,
    output: PathBuf,
    options: Option<&Bound<'_, PyAny>>,
    overwrite: bool,
    keep_partial: bool,
) -> PyResult<Py<PyAny>> {
    let mut request = CompressRequest::new(input, output);
    request.format = compression_format(options)?;
    request.overwrite = overwrite;
    request.keep_partial = keep_partial;
    service_report(py, "compress", move || application::compress(request))
}

#[allow(clippy::too_many_arguments)]
#[pyfunction]
#[pyo3(signature = (
    archive,
    output,
    *,
    verify = true,
    max_output_bytes = None,
    max_chunks = None,
    max_memory_bytes = None,
    overwrite = false,
    keep_partial = false
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
) -> PyResult<Py<PyAny>> {
    let mut request = DecompressRequest::new(archive, output);
    request.verify = verify;
    request.max_output_bytes = max_output_bytes;
    request.max_chunks = max_chunks;
    request.max_memory_bytes = max_memory_bytes;
    request.overwrite = overwrite;
    request.keep_partial = keep_partial;
    service_report(py, "decompress", move || application::decompress(request))
}

#[pyfunction]
#[pyo3(signature = (
    archive,
    *,
    against = None,
    max_output_bytes = None,
    max_chunks = None,
    max_memory_bytes = None
))]
fn validate(
    py: Python<'_>,
    archive: PathBuf,
    against: Option<PathBuf>,
    max_output_bytes: Option<u64>,
    max_chunks: Option<u64>,
    max_memory_bytes: Option<u64>,
) -> PyResult<Py<PyAny>> {
    let mut request = ValidateRequest::new(archive);
    request.against = against;
    request.max_output_bytes = max_output_bytes;
    request.max_chunks = max_chunks;
    if let Some(value) = max_memory_bytes {
        request.max_memory_bytes = value;
    }
    service_report(py, "validate", move || application::validate(request))
}

#[pyfunction]
#[pyo3(signature = (input, *, mode = "quick", runs = 3, max_input_mb = None))]
fn compare(
    py: Python<'_>,
    input: PathBuf,
    mode: &str,
    runs: usize,
    max_input_mb: Option<u64>,
) -> PyResult<Py<PyAny>> {
    let mut request = CompareRequest::new(input);
    request.mode = parse_compare_mode(mode)?;
    request.runs = runs;
    request.max_input_mb = max_input_mb;
    service_report(py, "compare", move || application::compare(request))
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

#[pymodule(gil_used = false)]
fn _native(module: &Bound<'_, PyModule>) -> PyResult<()> {
    let py = module.py();
    module.add("DataPackError", py.get_type::<DataPackError>())?;
    module.add("DataPackIOError", py.get_type::<DataPackIOError>())?;
    module.add("DataPackFormatError", py.get_type::<DataPackFormatError>())?;
    module.add(
        "DataPackConfigurationError",
        py.get_type::<DataPackConfigurationError>(),
    )?;
    module.add(
        "DataPackAnalysisError",
        py.get_type::<DataPackAnalysisError>(),
    )?;
    module.add("DataPackOutputError", py.get_type::<DataPackOutputError>())?;
    module.add(
        "DataPackOperationError",
        py.get_type::<DataPackOperationError>(),
    )?;
    module.add(
        "DataPackTranslationError",
        py.get_type::<DataPackTranslationError>(),
    )?;
    module.add_class::<PyV1CompressionOptions>()?;
    module.add_class::<PyV2CompressionOptions>()?;
    module.add_function(wrap_pyfunction!(analyze, module)?)?;
    module.add_function(wrap_pyfunction!(compress, module)?)?;
    module.add_function(wrap_pyfunction!(decompress, module)?)?;
    module.add_function(wrap_pyfunction!(validate, module)?)?;
    module.add_function(wrap_pyfunction!(compare, module)?)?;
    Ok(())
}
