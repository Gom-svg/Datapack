"""Thin path-based Python access to the DataPack Rust application API."""

from ._native import (
    DataPackAnalysisError,
    DataPackConfigurationError,
    DataPackError,
    DataPackFormatError,
    DataPackIOError,
    DataPackOperationError,
    DataPackOutputError,
    DataPackTranslationError,
    V1CompressionOptions,
    V2CompressionOptions,
    analyze,
    compare,
    compress,
    decompress,
    validate,
)

__all__ = [
    "DataPackError",
    "DataPackAnalysisError",
    "DataPackConfigurationError",
    "DataPackFormatError",
    "DataPackIOError",
    "DataPackOperationError",
    "DataPackOutputError",
    "DataPackTranslationError",
    "V1CompressionOptions",
    "V2CompressionOptions",
    "analyze",
    "compare",
    "compress",
    "decompress",
    "validate",
]

__version__ = "0.1.0"
