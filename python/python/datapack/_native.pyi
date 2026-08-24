from os import PathLike
from typing import Any, Callable, Dict, Optional, Union

Pathish = Union[str, PathLike[str]]
Report = Dict[str, Any]
ProgressCallback = Callable[["ProgressEvent"], None]

class DataPackError(Exception):
    """Base exception for DataPack SDK failures."""

class DataPackIOError(DataPackError): ...
class DataPackFormatError(DataPackError): ...
class DataPackConfigurationError(DataPackError): ...
class DataPackAnalysisError(DataPackError): ...
class DataPackOutputError(DataPackError): ...
class DataPackOperationError(DataPackError): ...
class DataPackTranslationError(DataPackError): ...

class ProgressEvent:
    """Read-only progress facts emitted by the Rust application API."""

    operation: str
    stage: str
    state: str
    completed_bytes: int
    total_bytes: Optional[int]
    completed_items: int
    total_items: Optional[int]
    percentage: Optional[float]
    terminal: bool

class V1CompressionOptions:
    mode: str
    sample_mb: int
    verify_best: bool
    max_dictionary_values: int
    max_dictionary_mb: int

    def __init__(
        self,
        *,
        mode: str = "fast",
        sample_mb: int = 64,
        verify_best: bool = False,
        max_dictionary_values: int = 65_535,
        max_dictionary_mb: int = 64,
    ) -> None: ...

class V2CompressionOptions:
    chunk_size_bytes: int
    threads: int
    max_in_flight_chunks: int
    backend: str
    adaptive_level: bool
    max_memory_bytes: Optional[int]

    def __init__(
        self,
        *,
        chunk_size_bytes: Optional[int] = None,
        threads: Optional[int] = None,
        max_in_flight_chunks: Optional[int] = None,
        backend: str = "chunked_raw_zstd",
        adaptive_level: bool = False,
        max_memory_bytes: Optional[int] = None,
    ) -> None: ...

def analyze(
    input: Pathish,
    *,
    sample_mb: int = 64,
    progress: Optional[ProgressCallback] = None,
) -> Report: ...
def compress(
    input: Pathish,
    output: Pathish,
    *,
    options: Optional[Union[V1CompressionOptions, V2CompressionOptions]] = None,
    overwrite: bool = False,
    keep_partial: bool = False,
    progress: Optional[ProgressCallback] = None,
) -> Report: ...
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
) -> Report: ...
def validate(
    archive: Pathish,
    *,
    against: Optional[Pathish] = None,
    max_output_bytes: Optional[int] = None,
    max_chunks: Optional[int] = None,
    max_memory_bytes: Optional[int] = None,
    progress: Optional[ProgressCallback] = None,
) -> Report: ...
def compare(
    input: Pathish,
    *,
    mode: str = "quick",
    runs: int = 3,
    max_input_mb: Optional[int] = None,
    progress: Optional[ProgressCallback] = None,
) -> Report: ...
