# DataPack Python SDK foundation

This directory contains a thin PyO3 extension over DataPack's public Rust
Application API. Rust remains the source of truth for analysis, planning,
compression, decompression, validation, comparison, security limits, and
byte-exact restoration.

The authorized future PyPI distribution name is `datapack-engine`; the import
package remains `datapack`. It has not been published or reserved. A separately
authorized public release may eventually install with
`pip install datapack-engine` while application code continues to use
`import datapack`. Do not install the unrelated PyPI distribution named `datapack`
expecting this engine.

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

## Build and test from a checkout

Rust 1.85 or newer and Python 3.9 or newer are required. The extension uses
the CPython stable ABI with a Python 3.9 floor.

```bash
cd python
python -m venv .venv
. .venv/bin/activate
python -m pip install "maturin>=1.0,<2.0"
maturin develop
python -m unittest discover -s tests -v
```

Long-running Rust application calls detach from the Python interpreter, so
other Python threads are not needlessly held while DataPack works. Result
translation reattaches only after the Rust operation completes.

## Current scope and limitations

This is an SDK foundation, not a claim of production packaging readiness.

- There is no pandas, Polars, Spark, or other dataframe dependency.
- There is no Python reimplementation of DataPack algorithms.
- Cancellation is deliberately not exposed because the codecs do not yet
  provide bounded-latency cooperative cancellation.
- Progress callbacks and the legacy Benchmark workflow are not exposed in this
  first Python surface.
- Calls are synchronous. Applications may place them on their own worker
  threads; the extension releases interpreter ownership during Rust work.
- Wheels are built from a complete DataPack repository checkout because this
  crate has a path dependency on the root Rust crate.
- Desktop and GPU functionality are not included.
