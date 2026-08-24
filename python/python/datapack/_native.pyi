from os import PathLike
from typing import Any, Callable, Dict, List, Literal, Optional, TypedDict, Union

Pathish = Union[str, PathLike[str]]
Report = Dict[str, Any]
ProgressCallback = Callable[["ProgressEvent"], None]
OperationName = Literal["analyze", "compress", "decompress", "validate", "compare"]
ProgressStage = Literal[
    "analyzing",
    "planning",
    "reading_input",
    "hashing",
    "compressing",
    "writing_archive",
    "reading_archive",
    "decompressing",
    "writing_output",
    "validating",
    "comparing",
    "cleaning_up",
    "finalizing",
]
ProgressStateName = Literal["started", "advanced", "completed"]

class OperationDiagnostic(TypedDict):
    code: str
    message: str
    column_index: Optional[int]

class AnalysisReport(TypedDict):
    schema_version: int
    report_type: Literal["analysis"]
    dataset: Dict[str, Any]
    sampling: Dict[str, Any]
    planner: Dict[str, Any]
    diagnostics: List[Dict[str, Any]]

class CompressionResult(TypedDict):
    schema_version: int
    report_type: Literal["compression"]
    archive_version: int
    selected_mode: str
    input_size_bytes: int
    archive_size_bytes: int
    backend: str
    diagnostics: List[OperationDiagnostic]
    profile: Dict[str, Any]

class DecompressionResult(TypedDict):
    schema_version: int
    report_type: Literal["decompression"]
    archive_version: int
    selected_mode: str
    archive_size_bytes: int
    restored_size_bytes: int
    verified: Optional[bool]
    backend: str
    diagnostics: List[OperationDiagnostic]
    profile: Dict[str, Any]

class ValidationReport(TypedDict):
    schema_version: int
    report_type: Literal["validation"]
    valid: bool
    archive: Dict[str, Any]
    checks: Dict[str, str]
    against: Dict[str, Any]
    diagnostics: List[Dict[str, Any]]

class ComparisonReport(TypedDict):
    schema_version: int
    report_type: Literal["comparison"]
    mode: Literal["quick", "full"]
    scope: Dict[str, Any]
    methodology: Dict[str, Any]
    datapack: Dict[str, Any]
    standalone_zstd: Dict[str, Any]
    winners: Dict[str, str]
    limitations: List[Dict[str, str]]

class DataPackError(Exception):
    """Base exception for DataPack SDK failures."""

    category: str
    code: str

class DataPackIOError(DataPackError):
    """A DataPack filesystem operation failed."""

class DataPackFormatError(DataPackError):
    """Input data or an archive had an invalid format."""

class DataPackConfigurationError(DataPackError):
    """A DataPack request option was invalid."""

class DataPackAnalysisError(DataPackError):
    """DataPack could not analyze the requested input."""

class DataPackOutputError(DataPackError):
    """DataPack could not safely create or replace the requested output."""

class DataPackOperationError(DataPackError):
    """A transactional DataPack operation failed."""

class DataPackTranslationError(DataPackError):
    """A Rust application report could not be translated to Python."""

class CancelledError(DataPackError):
    """A DataPack operation was cooperatively cancelled."""

class CancellationToken:
    """Thread-safe, monotonic cooperative cancellation control."""

    def __init__(self) -> None: ...
    def cancel(self) -> None: ...
    @property
    def is_cancelled(self) -> bool: ...

class ProgressEvent:
    """Read-only progress facts emitted by the Rust application API."""

    operation: OperationName
    stage: ProgressStage
    state: ProgressStateName
    completed_bytes: int
    total_bytes: Optional[int]
    completed_items: int
    total_items: Optional[int]
    percentage: Optional[float]
    terminal: bool

class V1CompressionOptions:
    """Immutable planner-selected v1 options with bounded analysis/dictionaries."""

    mode: Literal["fast", "best"]
    sample_mb: int
    verify_best: bool
    max_dictionary_values: int
    max_dictionary_mb: int

    def __init__(
        self,
        *,
        mode: Literal["fast", "best"] = "fast",
        sample_mb: int = 64,
        verify_best: bool = False,
        max_dictionary_values: int = 65_535,
        max_dictionary_mb: int = 64,
    ) -> None: ...

class V2CompressionOptions:
    """Immutable bounded-pipeline v2 options with a memory admission limit."""

    chunk_size_bytes: int
    threads: int
    max_in_flight_chunks: int
    backend: Literal["chunked_raw_zstd", "zstd_mt_experimental"]
    adaptive_level: bool
    max_memory_bytes: Optional[int]

    def __init__(
        self,
        *,
        chunk_size_bytes: Optional[int] = None,
        threads: Optional[int] = None,
        max_in_flight_chunks: Optional[int] = None,
        backend: Literal[
            "chunked_raw_zstd", "zstd_mt_experimental"
        ] = "chunked_raw_zstd",
        adaptive_level: bool = False,
        max_memory_bytes: Optional[int] = None,
    ) -> None: ...

def analyze(
    input: Pathish,
    *,
    sample_mb: int = 64,
    progress: Optional[ProgressCallback] = None,
    cancellation: Optional[CancellationToken] = None,
) -> AnalysisReport: ...
def compress(
    input: Pathish,
    output: Pathish,
    *,
    options: Optional[Union[V1CompressionOptions, V2CompressionOptions]] = None,
    overwrite: bool = False,
    keep_partial: bool = False,
    progress: Optional[ProgressCallback] = None,
    cancellation: Optional[CancellationToken] = None,
) -> CompressionResult: ...
def decompress(
    archive: Pathish,
    output: Pathish,
    *,
    verify: bool = True,
    max_output_bytes: Optional[int] = None,
    max_chunks: Optional[int] = None,
    max_memory_bytes: Optional[int] = None,
    overwrite: bool = False,
    keep_partial: bool = False,
    progress: Optional[ProgressCallback] = None,
    cancellation: Optional[CancellationToken] = None,
) -> DecompressionResult: ...
def validate(
    archive: Pathish,
    *,
    against: Optional[Pathish] = None,
    max_output_bytes: Optional[int] = None,
    max_chunks: Optional[int] = None,
    max_memory_bytes: Optional[int] = None,
    progress: Optional[ProgressCallback] = None,
    cancellation: Optional[CancellationToken] = None,
) -> ValidationReport: ...
def compare(
    input: Pathish,
    *,
    mode: Literal["quick", "full"] = "quick",
    runs: int = 3,
    max_input_mb: Optional[int] = None,
    progress: Optional[ProgressCallback] = None,
    cancellation: Optional[CancellationToken] = None,
) -> ComparisonReport: ...
