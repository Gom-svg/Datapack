# Python SDK foundation

Status: implemented foundation (Phase 13)

The Python package in `python/` is a thin PyO3 and maturin adapter over the
public Rust Application API. It does not contain an independent compressor,
parser, planner, validator, or comparison implementation.

The supported dependency direction is:

```text
Python caller
    |
    v
datapack Python adapter
    |
    v
datapack::application
    |
    v
DataPack core engine
```

Rust remains the source of truth for archive behavior, defaults, security
limits, transaction handling, and byte-exact restoration.

## Status and requirements

- Package status: alpha SDK foundation, not production distribution readiness
- Authorized future PyPI distribution: `datapack-engine` (not published or reserved)
- Python import package: `datapack`
- Python: 3.9 or newer
- Rust: 1.85 or newer
- Binding: PyO3 0.29.0
- Build backend: maturin 1.x
- Wheel ABI: CPython stable ABI with a Python 3.9 floor
- Dataframe dependencies: none

Wheel builds currently require a complete DataPack checkout because the
binding crate depends on the root Rust crate by path.

## Public surface

The `datapack` package exports synchronous, path-based operations:

```python
datapack.analyze(input, *, sample_mb=64)

datapack.compress(
    input,
    output,
    *,
    options=None,
    overwrite=False,
    keep_partial=False,
)

datapack.decompress(
    archive,
    output,
    *,
    verify=True,
    max_output_bytes=None,
    max_chunks=None,
    max_memory_bytes=None,
    overwrite=False,
    keep_partial=False,
)

datapack.validate(
    archive,
    *,
    against=None,
    max_output_bytes=None,
    max_chunks=None,
    max_memory_bytes=None,
)

datapack.compare(
    input,
    *,
    mode="quick",
    runs=3,
    max_input_mb=None,
)
```

Paths accept strings and `os.PathLike` values, including `pathlib.Path`.
Successful calls return ordinary Python dictionaries derived from the
existing versioned Rust report serialization. The Python layer introduces no
second result schema.

Benchmark and Advisor are not exposed by this foundation. Benchmark has
artifact-lifecycle and observer-timing concerns that require a deliberate
Python contract. Advisor is not part of the Phase 12 Rust Application API.

## Compression options

Compression uses one of two immutable option objects. Omitting `options`
selects the Rust v1 defaults.

```python
datapack.V1CompressionOptions(
    mode="fast",                 # "fast" or "best"
    sample_mb=64,
    verify_best=False,
    max_dictionary_values=65_535,
    max_dictionary_mb=64,
)

datapack.V2CompressionOptions(
    chunk_size_bytes=None,       # None uses the Rust default
    threads=None,                # None uses the Rust default
    max_in_flight_chunks=None,   # None uses the Rust default
    backend="chunked_raw_zstd",
    adaptive_level=False,
    max_memory_bytes=None,
)
```

The objects map directly into `V1CompressionOptions` and
`V2CompressionOptions`. Python does not reproduce dynamic v2 defaults.

## Results and errors

Reports retain their Rust `schema_version` and `report_type`. Compression and
decompression reports can contain non-fatal diagnostics even when the
operation succeeds.

Rust error variants map structurally, without parsing display strings, into
typed subclasses of `DataPackError`:

- `DataPackIOError`
- `DataPackFormatError`
- `DataPackConfigurationError`
- `DataPackAnalysisError`
- `DataPackOutputError`
- `DataPackOperationError`
- `DataPackTranslationError`

An invalid archive normally makes `validate()` return a report whose `valid`
field is false. Operational failures, such as an unreadable path, raise an
exception.

## Limits and transaction semantics

Application and Python resource limits use bytes unless the argument name
explicitly ends in `_mb`.

- `max_chunks` applies to v2 chunk tables.
- v2 compression `max_memory_bytes` is an admission check derived from chunk
  size and in-flight work; it is not a total process-RSS ceiling.
- `compare(max_input_mb=...)` is valid only in Quick mode.
- v1 archives do not store the native per-chunk/global hash guarantees present
  in v2. Accordingly, v1 decompression reports `verified` as `None`.
- `validate(against=...)` provides a full restored-identity comparison for v1
  or v2.
- `overwrite=False` preserves an existing destination.
- `keep_partial=True` retains the operation-owned temporary output after a
  failed write path. It does not weaken commit validation.

All successful output replacement is performed by the Rust transactional
output machinery.

## Interpreter concurrency and cancellation

The adapter extracts owned arguments, then uses `Python::detach` around the
complete Rust service operation and report serialization. It reattaches only
to construct the returned Python objects. This permits other Python threads to
make progress while Rust performs file I/O or CPU work.

Detaching from the interpreter is not cancellation. Cooperative cancellation
is **deferred by design** because current codec operations do not promise
bounded-latency interruption. The foundation exposes neither a cancellation
token nor a misleading best-effort substitute.

Progress callbacks are also not exposed in this phase. In particular, Python
callbacks during Compare could contaminate measured timings.

## Build and certification

From a complete checkout:

```bash
cd python
python -m venv .venv
. .venv/bin/activate
python -m pip install "maturin>=1.0,<2.0"
maturin develop
python -m unittest discover -s tests -v
```

Phase 13 certification covers Rust check/format/Clippy, a maturin wheel build,
wheel installation into an isolated environment, Python API tests, v1 and
multi-chunk v2 byte-exact round trips, v2 hash validation, and typed failures.

## Explicit non-goals

This foundation does not implement:

- compression or parsing logic in Python
- pandas, Polars, Spark, or dataframe integration
- async operations
- progress callbacks
- cancellation
- Benchmark or Advisor bindings
- production wheel publication or a release matrix
- Desktop, GPU, cloud, SaaS, or telemetry functionality
