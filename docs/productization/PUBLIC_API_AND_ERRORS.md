# DataPack Public API and Error Experience

Status: **IMPLEMENTED** in Productization P5. Local and hosted certification
status is recorded in the Productization report.

This document describes the product-facing operation, result, diagnostic, and
error contract shared by DataPack's Rust Application API, Python SDK, and CLI.
It does not change compression, archive, planner, progress, cancellation, or
transaction semantics.

## One operation model

The Rust Application layer remains the source of truth:

```text
DataPack Application operations and versioned reports
        |
        +-- Rust callers: typed requests, reports, and errors
        +-- Python: thin PyO3 adaptation and typed exceptions
        +-- CLI: human text or versioned report output
```

The six Rust operations are Analyze, Compress, Decompress, Validate, Compare,
and Benchmark. Python exposes the first five. The CLI also exposes Advisor,
Generate Test Data, and Tune, whose established contracts remain separate.

Existing simple, `*_with_progress`, and `*_with_control` Rust entry points are
unchanged. Existing Python call signatures are also unchanged. P5 adds
documentation, typing, and error identity rather than a parallel API or result
model.

## Result semantics

Successful Application reports are versioned structured facts:

- Analyze states actual sampling scope, completeness, bytes/records analyzed,
  reached limits, planner selection, reason codes, and diagnostics. Partial
  analysis never claims whole-file certainty.
- Compress states the archive version, selected mode and backend, input/archive
  byte sizes, non-fatal diagnostics, and profiling facts.
- Decompress states the archive version and mode, backend, archive/restored byte
  sizes, applicable verification fact, diagnostics, and profile. V1 reports
  `verified: None` because v1 does not store v2's hash guarantees.
- Validate separates a completed validation whose `valid` field is false from
  an inability to perform validation. Against-source mismatch is a structured
  `against` result, not an exception disguised as a boolean.
- Compare reports its Quick or Full scope, methodology, two measured
  competitors, independent winners, and limitations. A winner or difference is
  a successful result; an inability to compare is an operation failure.
- Benchmark reports performed work, scope, validation status, measurements,
  limitations, and retained artifacts. Unavailable work remains optional.

Python retains these Rust report shapes as dictionaries. P5's `.pyi` file adds
`TypedDict` return annotations for discoverability without replacing
dictionaries or duplicating report logic.

## Rust error identity

`DatapackError` remains the established exhaustive public enum. P5 adds no
variant and therefore does not break downstream exhaustive matching.

Every `DatapackError` now provides:

- `category() -> ErrorCategory`, a broad typed category; and
- `code() -> &'static str`, the stable snake-case variant identity.

`ErrorCategory` is non-exhaustive and serializes to snake case for future
adapter/IPC use. Current categories are `io`, `format`, `configuration`,
`analysis`, `output`, `operation`, and `cancellation`. Current ordinary error
codes are:

| Rust variant | Stable code |
| --- | --- |
| `Io` | `io_error` |
| `Bincode` | `metadata_serialization_error` |
| `InvalidFormat` | `invalid_format` |
| `InvalidProfile` | `invalid_profile` |
| `OutputNotWritable` | `output_not_writable` |
| `RowsOutOfRange` | `rows_out_of_range` |
| `AnalyzeRead` | `analysis_read` |
| `InvalidCsv` | `invalid_csv` |
| `ArchiveParse` | `archive_parse` |
| `OperationFailed` | `operation_failed` |

Human-readable `Display` messages and standard error source chains remain
available. Codes are deliberately few and stable; report diagnostics continue
to carry their established domain-specific codes such as
`DECLARED_OUTPUT_LIMIT_REACHED` and `AGAINST_MISMATCH`.

`OperationError` remains `Cancelled | Failed(DatapackError)`. It now mirrors
`category()` and `code()` helpers: cancellation has category `cancellation` and
code `cancelled`; ordinary failures delegate to the underlying
`DatapackError`. `OperationError::is_cancelled()` remains available.

## Paths, output, and limits

Common path failures identify both the role and the relevant path: input,
archive, output, or output parent. DataPack preserves the underlying OS error
text where one exists. Existing-destination errors name the destination and
the explicit `--force` CLI remedy. Operation failures state whether a previous
output was preserved or no final output was committed.

Resource safety is not relaxed. Validation reports use specific diagnostic
codes for output, chunk, and working-memory limits. Decompression and request
admission failures remain typed errors whose messages identify the configured
limit and observed declaration/configuration. DataPack never retries by
silently raising a safety bound.

Safe defaults remain:

- output replacement disabled (`overwrite=False`; no CLI `--force`);
- v2 decompression verification enabled (`verify=True`; no CLI `--no-verify`);
- failed partial retention disabled (`keep_partial=False`; no CLI
  `--keep-temp`);
- bounded analysis, validation, chunk-table, and pipeline behavior; and
- transactional publication only after successful completion.

## Python exceptions and typing

All SDK failures remain catchable through `datapack.DataPackError`.
`datapack.CancelledError` remains a direct subclass and explicit cancellation
outcome. Existing subclasses are unchanged:

- `DataPackIOError` — `io` / `io_error`;
- `DataPackFormatError` — `format` / `format_error`;
- `DataPackConfigurationError` — `configuration` / `configuration_error`;
- `DataPackAnalysisError` — `analysis` / `analysis_error`;
- `DataPackOutputError` — `output` / `output_error`;
- `DataPackOperationError` — `operation` / `operation_error`;
- `DataPackTranslationError` — `translation` / `translation_error`; and
- `CancelledError` — `cancellation` / `cancelled`.

Each exception class exposes stable `category` and `code` attributes. These
are intentionally class-level adapter identities: Rust variants map to the
subclasses structurally, without display-message parsing, while the Rust API
retains its finer variant code. Existing `except DataPackError` code continues
to catch every SDK operation error.

The installed extension provides concise docstrings for all five operations,
`ProgressEvent`, `CancellationToken`, option types, and exception types. The
stub declares path-like inputs, literal option values, progress callback
types, structured report `TypedDict`s, cancellation, and optional totals while
remaining compatible with Python 3.9.

```python
from pathlib import Path
import datapack

source = Path("input.csv")
archive = Path("input.dpack")
restored = Path("restored.csv")
token = datapack.CancellationToken()

def progress(event: datapack.ProgressEvent) -> None:
    print(event.operation, event.stage, event.percentage)
    # Cancellation, when desired, is explicit: token.cancel()

try:
    analysis = datapack.analyze(source)
    compressed = datapack.compress(
        source,
        archive,
        progress=progress,
        cancellation=token,
    )
    validation = datapack.validate(archive, against=source)
    if not validation["valid"]:
        for diagnostic in validation["diagnostics"]:
            print(diagnostic["code"], diagnostic["message"])
    decompressed = datapack.decompress(archive, restored)
    comparison = datapack.compare(source, mode="quick", runs=3)
except datapack.CancelledError:
    print("operation cancelled")
except datapack.DataPackError as error:
    print(error.category, error.code, str(error))
```

Progress callback exceptions retain the P3 policy: report through
`sys.unraisablehook`, disable later callbacks for that operation, and let the
Rust operation continue. They do not become cancellation. Explicit tokens and
typed `CancelledError` retain the P4 policy.

## CLI policy

The CLI uses these stable practical exit classes:

| Status | Meaning |
| --- | --- |
| `0` | Command and requested operation succeeded. |
| `1` | General command/operation failure, including a completed validation whose report is invalid. |
| `2` | Clap usage/argument failure, invalid structured input, or the established output-writability status. |
| `3` | The established `--rows` range failure. |

The historical numeric mapping is unchanged. New CLI error rendering is
`error[stable_code]: human-readable context`. Users should use the bracketed
code for stable failure identity and treat the remaining prose as diagnostic
text.

Report/data output remains on stdout. Runtime errors, progress, and profiling
remain on stderr. For `validate --json`, a completed invalid validation keeps
its structured report on stdout and returns nonzero with a stable error code on
stderr. A failure that prevents report creation leaves stdout empty. Compare
winners are report facts and do not create an error exit. Clap usage failures
remain exit 2 and are rendered by Clap.

Command help now identifies source/destination roles, bounded analysis and
comparison scopes, overwrite protection, verification defaults, resource
limits, partial retention, and JSON/report options without requiring Rust
source inspection.

## Compatibility and limits

P5 is additive for Rust and Python calls. It changes normal CLI error text to
include stable identity while preserving all established exit values and
stdout/stderr roles. It adds no dependency, network call, telemetry, unsafe
code, async runtime, archive format, or engine execution path.

Desktop/IPC remains **DEFERRED**, but `ErrorCategory`, stable string codes,
Serde-compatible categories, and versioned reports can be adapted without
serializing Rust implementation internals. Python `KeyboardInterrupt` and CLI
Ctrl+C integration remain **DEFERRED**. PyPI, TestPyPI, crates.io, public
releases, `.dpack` v3, GPU, and P6 are not part of P5.
