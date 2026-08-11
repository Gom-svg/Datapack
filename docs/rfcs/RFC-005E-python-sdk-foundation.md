# RFC-005E: Python SDK foundation

Status: Accepted and implemented in Phase 13

## Summary

DataPack provides an initial Python package built with PyO3 and maturin. The
package is a presentation and translation adapter over the public Rust
Application API introduced by RFC-005D. Rust remains the only implementation
of DataPack operations.

The foundation exposes path-based Analyze, Compress, Decompress, Validate,
and Compare operations. It deliberately does not promise production package
distribution, public cancellation, progress callbacks, or Python-owned
archive behavior.

## Motivation

The CLI is not an appropriate reusable boundary for Python or future desktop
consumers. Invoking CLI subprocesses would couple callers to Clap, terminal
rendering, text parsing, and process-level error handling. Reimplementing the
engine in Python would create competing planner, validation, security, and
wire-format semantics.

RFC-005D established owned requests and versioned results independent of
terminal I/O. Phase 13 proves that boundary with a real second consumer.

## Goals

- call the Rust Application API directly;
- expose a small synchronous path-based Python surface;
- preserve Rust defaults and transactional behavior;
- return versioned Rust report facts as ordinary Python data;
- translate Rust failures into typed Python exceptions;
- accept `pathlib.Path` and other `os.PathLike` values;
- release interpreter ownership around long Rust work;
- avoid mandatory dataframe dependencies;
- document foundation limitations honestly.

## Non-goals

- reimplementing any DataPack algorithm in Python;
- providing a dataframe API;
- exposing Benchmark before its artifact and timing contract is designed;
- adding Advisor to the Rust Application API;
- exposing progress callbacks in measured operations;
- claiming cooperative cancellation;
- publishing wheels or declaring a production support matrix;
- implementing Desktop, GPU, cloud, SaaS, or telemetry behavior.

## Decision

### Package layout

The binding lives in a separate, non-published Rust crate under `python/`.
Maturin builds a mixed Python package:

```text
python/
  Cargo.toml
  pyproject.toml
  src/lib.rs
  python/datapack/
    __init__.py
    _native.pyi
    py.typed
  tests/
```

The compiled extension is `datapack._native`. The pure Python package
re-exports the supported public names as `datapack.*`. A separate crate keeps
PyO3 and extension-module linkage out of the core Rust package and CLI.

The binding uses a path dependency on the root crate. Therefore, the accepted
Phase 13 build unit is a complete repository checkout, not a standalone source
distribution.

### Operation surface

The foundation exposes:

- `analyze`
- `compress`
- `decompress`
- `validate`
- `compare`

Each function constructs its request through the public Rust constructor and
then mutates only documented public fields. This is required because requests
are non-exhaustive and preserves future Rust source compatibility.

Compression accepts either `V1CompressionOptions` or
`V2CompressionOptions`. Keeping version-specific options separate avoids a
single request containing fields that are irrelevant to one archive version.
Omitting the Python option object selects `V1CompressionOptions::default()`.
V2 defaults come from `V2CompressionOptions::default()` rather than duplicated
Python constants.

### Result translation

The Rust service returns an owned, serializable report. The binding serializes
that report with `serde_json` and decodes it into standard Python dictionaries,
lists, scalars, and `None`.

This translation intentionally reuses the existing report shape. It does not
create Python-specific archive facts or a competing schema. Fields excluded
from Rust serialization, including CLI-only notices and storage-private
details, remain excluded.

### Exceptions

`DataPackError` is the Python base exception. Typed subclasses cover I/O,
format, configuration, analysis, output, transactional operation, and result
translation failures.

Mapping is performed by matching the Rust `DatapackError` variant. Display
messages are retained as context but are never parsed to determine the Python
type. Binding-level option errors map to `DataPackConfigurationError`.

Validate retains its report-oriented contract: a structurally readable but
invalid archive normally produces `valid = false`; an operational inability
to validate raises.

### Paths

PyO3 extracts path arguments into owned `PathBuf` values before Rust work
begins. This accepts Python strings and `os.PathLike` objects without the
binding performing lossy path-string conversion.

### Interpreter ownership

All Python-owned arguments are converted before entering the service. The
binding calls `Python::detach` around the Rust operation and Rust-side report
serialization. Python object construction occurs only after reattachment.

No borrowed Python object crosses the detached closure. This lets other
Python threads run during DataPack work and avoids holding interpreter
ownership while native worker threads execute.

This does not create an asynchronous API and does not imply cancellation.

## Security and compatibility

The binding does not bypass Application API validation. Resource limits,
overwrite policy, temporary-output behavior, v1/v2 verification distinctions,
and archive parsing remain Rust-owned.

The Python layer introduces no wire-format changes. It neither reads nor
writes metadata directly. Frozen v1 and v2 fixtures therefore remain governed
by the existing Rust compatibility tests.

`max_chunks` is a v2 limit. Byte-named resource arguments are bytes. The v2
compression memory option is an admission bound, not a claim about total RSS.
Python defaults must not weaken these meanings.

The binding source also forbids unsafe Rust. PyO3 and other dependencies are
audited and compiled as external crates; no application-owned unsafe block is
introduced.

## Progress and cancellation

Progress callbacks are deferred. The Rust typed progress API is synchronous
and observer timing is part of the calling thread. Bridging callbacks into
Python, especially during Compare, requires an explicit policy for exception
propagation, reattachment, reentrancy, and timing contamination.

Cancellation is **deferred by design**. Existing codecs do not provide a
bounded-latency cooperative stop contract. The SDK must not infer cancellation
from interpreter detachment or claim that `KeyboardInterrupt` can safely stop
an in-progress codec operation.

## Packaging decision

PyO3 is pinned to 0.29.0 for the Phase 13 checkpoint. It supports the project
Rust 1.85 toolchain. The extension uses the CPython stable ABI with a Python
3.9 floor. Maturin is constrained to the compatible 1.x series.

This is a foundation checkpoint. Publishing, platform wheel matrices, signing,
release automation, and long-term Python API compatibility policy remain
future productization work.

## Test strategy

Phase certification MUST include:

- Rust format, check, strict Clippy, and test-build gates for the binding crate;
- an isolated maturin wheel build and installation;
- import and type-export sanity;
- `pathlib.Path` input;
- Analyze report translation;
- v1 compression, validation, and byte-exact decompression;
- multi-chunk v2 compression, chunk-table/hash validation, and byte-exact
  decompression;
- Compare with validated contender round trips;
- structural Rust-error mapping;
- binding-option failure mapping;
- root Rust regression certification.

The tests MUST NOT update frozen archives or goldens.

## Deferred work

- progress callback policy;
- cooperative cancellation after codec support exists;
- optional Benchmark exposure and artifact lifecycle;
- richer Python typing generated from report schemas;
- standalone source distributions and release wheels;
- platform CI/release matrix;
- async convenience APIs;
- productization integrations.

None of these deferrals authorizes a Python reimplementation of the engine.
