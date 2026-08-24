# Python SDK foundation

Status: implemented SDK foundation; distributable wheel foundation certified
on Linux and Windows in Productization P2; progress/cancellation certified in
P3/P4; public API and error polish implemented in P5

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

- Package status: pre-1.0 alpha binary distribution, not production readiness
- Python distribution: `datapack-engine` (not published or reserved)
- Python import package: `datapack`
- Python: 3.9 or newer
- Rust: 1.85 or newer
- Binding: PyO3 0.29.0
- Build backend: maturin 1.x
- Wheel ABI: CPython stable ABI with a Python 3.9 floor (`cp39-abi3`)
- Linux wheel: GNU x86_64, manylinux2014 / glibc 2.17 floor
- Windows wheel: MSVC x86_64; hosted build/test matrix certified through
  CPython 3.9–3.14
- Dataframe dependencies: none

Wheel builds require a complete DataPack checkout because the binding crate
depends on the root Rust crate by path. Installed wheels do not require the
checkout or a Rust toolchain.

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
    progress=None,
    cancellation=None,
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
    progress=None,
    cancellation=None,
)

datapack.validate(
    archive,
    *,
    against=None,
    max_output_bytes=None,
    max_chunks=None,
    max_memory_bytes=None,
    progress=None,
    cancellation=None,
)

datapack.compare(
    input,
    *,
    mode="quick",
    runs=3,
    max_input_mb=None,
    progress=None,
    cancellation=None,
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
- `CancelledError`

Each exception class exposes stable `category` and `code` attributes for
normal machine-readable handling. Every subclass, including `CancelledError`,
remains catchable as `DataPackError`. Human-readable exception strings retain
operation and path context.

An invalid archive normally makes `validate()` return a report whose `valid`
field is false. Operational failures, such as an unreadable path, raise an
exception. Compare winner/difference fields likewise belong to a successful
report; inability to perform the comparison raises an exception.

The installed public functions, `ProgressEvent`, `CancellationToken`, option
classes, and exception types have runtime docstrings. `_native.pyi` preserves
dictionary returns while describing them with report-specific `TypedDict`s,
path-like inputs, literal option values, optional totals, progress callbacks,
and cancellation.

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
complete Rust service operation and report serialization. It reattaches for a
configured synchronous progress callback and to construct returned Python
objects. This permits other Python threads to make progress while Rust performs
file I/O or CPU work between callbacks.

Detaching from the interpreter is not cancellation. P4 separately exposes
`datapack.CancellationToken`, whose shared Rust atomic state can be requested
from another Python thread while long Rust work remains detached. The five
public operations accept keyword-only `cancellation=` and raise the typed
`datapack.CancelledError` when a safe checkpoint accepts the request.
Cancellation is cooperative and does not promise instantaneous interruption.

P3 exposes optional keyword-only callbacks receiving immutable
`datapack.ProgressEvent` objects. The Rust Application layer supplies the
operation, stage, state, byte/item counters, optional totals, percentage, and
terminal-success fact. Python does not recalculate progress. A callback
exception is reported through `sys.unraisablehook`, disables later callbacks
for that operation, and does not cancel the Rust work. Python callbacks during
Compare can add caller wall time; Compare therefore keeps coarse progress.
An observer may explicitly call `token.cancel()`; neither its return value nor
an exception requests cancellation. Portable `KeyboardInterrupt` conversion is
deferred and is not claimed by this foundation.

## Build and certification

From a complete checkout:

```bash
cd python
python -m venv .venv
. .venv/bin/activate
python -m pip install "maturin==1.14.1"
maturin develop
python -m unittest discover -s tests -v
```

Phase 13 certification covers Rust check/format/Clippy, a maturin wheel build,
wheel installation into an isolated environment, Python API tests, v1 and
multi-chunk v2 byte-exact round trips, v2 hash validation, and typed failures.

Productization P2 adds exact wheel tag and content inspection, license and
metadata validation, machine-path hygiene, `pip --no-index --no-deps`
installation in a temporary environment outside the checkout, and installed
execution with repository paths absent from `sys.path`. The same Linux
manylinux2014 wheel is locally certified across CPython 3.9 through 3.14. The
hosted workflow builds once per Linux/Windows platform and downloads that same
platform wheel into each of six Python-version jobs.

See [Python Distribution](../productization/PYTHON_DISTRIBUTION.md) for exact
artifact tags, platform policy, CI design, sdist status, and publication
boundaries, and [Public API and Error Experience](../productization/PUBLIC_API_AND_ERRORS.md)
for result/error distinctions and examples.

## Explicit non-goals

This foundation does not implement:

- compression or parsing logic in Python
- pandas, Polars, Spark, or dataframe integration
- async operations
- automatic `KeyboardInterrupt`/Ctrl+C conversion
- Benchmark or Advisor bindings
- public wheel publication
- Desktop, GPU, cloud, SaaS, or telemetry functionality
