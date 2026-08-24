# DataPack Python distribution

This directory contains a thin PyO3 extension over DataPack's public Rust
Application API. Rust remains the source of truth for analysis, planning,
compression, decompression, validation, comparison, security limits, and
byte-exact restoration.

The distribution name is `datapack-engine`; the import package remains
`datapack`. Version 0.1.0 is an alpha productization version. The project has
not been published or reserved on PyPI or TestPyPI. A separately authorized
public release may eventually install with `pip install datapack-engine` while
application code continues to use `import datapack`. Do not install the
unrelated PyPI distribution named `datapack` expecting this engine.

## Supported wheel contract

- GIL-enabled CPython 3.9 through 3.14;
- stable ABI with a CPython 3.9 floor (`cp39-abi3`);
- Linux GNU x86_64 at glibc 2.17 or newer
  (`manylinux_2_17_x86_64` / `manylinux2014_x86_64`); and
- Windows MSVC x86_64 (`win_amd64`).

P2 locally certifies one unchanged Linux ABI3 wheel across all six declared
CPython minors. The committed hosted workflow builds one wheel per platform
and reuses it across the same six-version matrix. The P2 Windows workflow is
implemented but has not run at this unpushed checkpoint.

Until registry publication is explicitly authorized, install only a reviewed
wheel artifact:

```bash
python -m pip install /path/to/datapack_engine-0.1.0-cp39-abi3-PLATFORM.whl
python -c "import datapack; print(datapack.__version__)"
```

A matching wheel contains the native engine adapter and requires no source
checkout, Rust, Cargo, maturin, compiler, or Python runtime dependency at
installation time. The wheel does not expose the `datapack` CLI executable;
the CLI remains a separate Rust artifact concern.

The foundation exposes synchronous, path-based functions:

```python
from pathlib import Path
import datapack

source = Path("input.csv")
archive = Path("input.dpack")
restored = Path("restored.csv")

facts = datapack.analyze(source)
compressed = datapack.compress(
    source,
    archive,
    options=datapack.V1CompressionOptions(mode="fast"),
)
validation = datapack.validate(archive, against=source)
restored_facts = datapack.decompress(archive, restored)
comparison = datapack.compare(source, mode="quick", runs=3)
```

Each call also accepts an optional keyword-only structured progress callback:

```python
def on_progress(event: datapack.ProgressEvent) -> None:
    if event.percentage is None:
        print(event.operation, event.stage, event.completed_bytes)
    else:
        print(event.operation, event.stage, f"{event.percentage:.1f}%")

datapack.compress(source, archive, progress=on_progress)
```

`ProgressEvent` is immutable. It exposes snake-case operation/stage/state,
integer byte and item counters, optional totals, Rust-derived percentage, and
a terminal-success flag. Unknown totals are `None`; DataPack never fabricates
a percentage. A callback exception is reported through Python's unraisable
exception hook, disables further callbacks for that operation, and does not
cancel or otherwise alter the Rust operation.

Each successful call returns the corresponding versioned Rust report as a
plain Python dictionary. Both `str` and `os.PathLike` paths are accepted.
Compression format is selected by an immutable `V1CompressionOptions` or
`V2CompressionOptions` object; omitting `options` uses the Rust v1 defaults.
Rust error variants map directly to typed subclasses of `DataPackError`, such
as `DataPackAnalysisError`, `DataPackFormatError`, and `DataPackOutputError`.
This mapping does not parse display strings.

Resource arguments named `*_bytes` are byte counts; `sample_mb` and
`max_input_mb` are MiB-style application controls. `max_chunks` applies to v2,
and the v2 compression memory option is an admission check rather than a total
process-RSS ceiling. `compare(max_input_mb=...)` is valid only in Quick mode.

Compression and decompression use the Rust transactional output path.
`overwrite=False` preserves existing destinations, while `keep_partial=True`
retains an operation-owned temporary output after failure. Successful reports
may still contain non-fatal diagnostics. V1 decompression reports
`verified=None` because v1 lacks v2's stored hash guarantees; use
`validate(against=...)` when complete restored identity must be checked.
Validation of a readable but invalid archive normally returns a report with
`valid=False`; failures to perform the operation raise a typed exception.

## Build and certify from a checkout

The certified build inputs are Rust/Cargo 1.85.0, maturin 1.14.1, PyO3 0.29.0,
and a Python 3.9-or-newer interpreter. The extension uses the CPython stable
ABI with a Python 3.9 floor. A complete checkout is required to build because
the binding crate has an exact path dependency on the root Rust crate.

```bash
cd python
python -m venv .venv
. .venv/bin/activate
python -m pip install "maturin==1.14.1"
maturin develop
python -m unittest discover -s tests -v
```

The distributable Linux wheel must be built in a manylinux2014 environment or
with maturin's supported Zig strategy. A host build must not be relabeled as a
more portable wheel. From the repository root, the local P2 strategy is:

```bash
python -m pip install "maturin[zig]==1.14.1"
export RUSTFLAGS="--remap-path-prefix=$(pwd)=datapack-src --remap-path-prefix=$HOME=build-home"
maturin build --manifest-path python/Cargo.toml --release --locked --strip --zig --compatibility manylinux2014 --auditwheel check --interpreter python --out python/dist
python scripts/certify_python_wheel.py --wheel-dir python/dist --platform linux
```

The certification script checks the exact wheel tag, metadata, license,
runtime-content allowlist, absence of machine-specific paths, fresh wheel-only
installation, import resolution, version, and representative v1/v2 SDK
behavior from outside the repository. Its temporary environment deliberately
has no Rust, Cargo, or maturin on `PATH`.

Long-running Rust application calls detach from the Python interpreter, so
other Python threads are not needlessly held while DataPack works. A progress
callback safely reattaches for that synchronous callback and then detaches
again. An optional `datapack.CancellationToken` shares the Rust atomic
cancellation state, so another Python thread or an explicit progress callback
can call `cancel()`. Cancellation raises `datapack.CancelledError`. Result
translation reattaches only after the Rust operation completes.

## Current scope and limitations

This is a distributable pre-1.0 alpha binary package, not a claim of production
readiness or a public release.

- There is no pandas, Polars, Spark, or other dataframe dependency.
- There is no Python reimplementation of DataPack algorithms.
- Cooperative cancellation is explicit and checked at natural safe boundaries;
  it is not instantaneous. Python `KeyboardInterrupt` conversion remains
  deferred, and progress callback return values/exceptions do not cancel work.
- The legacy Benchmark workflow is not exposed in the Python surface. Analyze,
  Validate, Compare, and several v1 transforms expose truthful coarse progress
  rather than fabricated intermediate percentages.
- Calls are synchronous. Applications may place them on their own worker
  threads; the extension releases interpreter ownership during Rust work.
- Wheels are built from a complete DataPack repository checkout because this
  crate has a path dependency on the root Rust crate.
- Source-distribution support is deferred. Although the audit proved an sdist
  can be assembled, it would introduce a Rust/native-toolchain user contract
  and does not automatically produce the certified Linux portability tag.
- macOS, Linux musl, non-x86_64 targets, PyPy, and free-threaded CPython are not
  part of the P2 wheel contract.
- CI artifacts are temporary certification inputs, not GitHub Releases.
- PyPI, TestPyPI, and crates.io have not been modified.
- Desktop and GPU functionality are not included.

See
[`docs/productization/PYTHON_DISTRIBUTION.md`](../docs/productization/PYTHON_DISTRIBUTION.md)
for the complete distribution, CI, isolation, sdist, and publication policy.
