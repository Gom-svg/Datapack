# DataPack Rust Application API V1

`datapack::application` provides synchronous, path-based operations without
terminal or Clap dependencies. This page is a concise API map; individual
report semantics remain documented by their existing V1 references and RFCs.

## Operations

| Function pair | Request | Return type |
|---|---|---|
| `analyze`, `analyze_with_progress` | `AnalyzeRequest` | `AnalysisReportV1` |
| `compress`, `compress_with_progress` | `CompressRequest` | `CompressionResultV1` |
| `decompress`, `decompress_with_progress` | `DecompressRequest` | `DecompressionResultV1` |
| `validate`, `validate_with_progress` | `ValidateRequest` | `ValidationReportV1` |
| `compare`, `compare_with_progress` | `CompareRequest` | `ComparisonReportV1` |
| `benchmark`, `benchmark_with_progress` | `BenchmarkRequest` | `BenchmarkReportV1` |

Every function returns `datapack::error::Result<T>`. The non-progress form is
silent and performs the same operation without observer callbacks.

Advisor is not exported as a public application service in V1.

## Basic use

Request constructors supply defaults and own their paths. Because request
types are non-exhaustive, external callers should construct them with `new`
and then change public fields as needed.

```rust,no_run
use datapack::application::{self, AnalyzeRequest};

fn main() -> datapack::error::Result<()> {
    let mut request = AnalyzeRequest::new("input.csv");
    request.sample_mb = 32;

    let report = application::analyze(request)?;
    println!("analyzed {} bytes", report.sampling.bytes_analyzed);
    Ok(())
}
```

The example prints in the calling application. The DataPack service itself
does not write to stdout or stderr.

## Requests

### `AnalyzeRequest`

- `input`: input path;
- `sample_mb`: bounded analysis sample size in MiB; default 64.

### `CompressRequest`

- `input` and `output`: source and archive paths;
- `format`: `CompressionFormat::V1` or `CompressionFormat::V2`;
- `overwrite`: permit replacement of an existing regular output;
- `keep_partial`: retain the owned partial output after failure.

`V1CompressionOptions` contains `mode`, `sample_mb`, `verify_best`, and the
per-column dictionary value and memory limits. Its default is the compatible
Fast v1 path.

`V2CompressionOptions` contains chunk size in bytes, worker count,
max-in-flight chunks, backend, adaptive-level selection, and an optional
`max_memory_bytes` admission limit. The limit checks the configured
`chunk_size_bytes * max_in_flight_chunks` product before work starts. It is not
an absolute process-RSS ceiling and does not account for allocator, zstd,
thread-stack, or other implementation overhead. V2 remains chunked RawZstd.

### `DecompressRequest`

- `archive` and `output`: archive and restored-output paths;
- `verify`: v2 hash verification, enabled by default;
- optional output-byte and memory-byte limits;
- `max_chunks`: an optional v2 chunk-table limit;
- `overwrite` and `keep_partial` transactional-output controls.

Frozen v1 archives do not gain v2 per-chunk or global hashes through this
option. The result uses `verified: None` when that boolean would not describe
the archive guarantee truthfully. V1 has no chunk table, so `max_chunks` is
inapplicable to v1 and does not impose a v1 limit.

### `ValidateRequest`

- `archive`: archive path;
- `against`: optional original-source path;
- an optional output-byte limit;
- `max_chunks`: an optional v2 chunk-table limit, inapplicable to v1;
- `max_memory_bytes`: validation memory bound, default 512 MiB.

Validate creates no restored output.

### `CompareRequest`

- `input`: source path;
- `mode`: `CompareMode::Quick` or `CompareMode::Full`, default Quick;
- `runs`: timing runs, default 3;
- `max_input_mb`: optional Quick prefix bound.

The returned `ComparisonReportV1` follows the existing Comparison JSON V1
contract and methodology.

### `BenchmarkRequest`

The legacy Benchmark request contains its historical quick/run,
standalone-zstd, round-trip, hash, estimate-only, prefix, optional-v2, and
artifact-preservation controls. Its report states the typed measured scope,
validation status, structural partial reasons, performed-operation flags,
primary and standalone-zstd measurements, planner/profile facts, optional v2
measurements, and retained artifact paths. Benchmark is not an alias for
Compare and does not inherit Compare's narrower methodology.

## Results

All application result/report DTOs are owned and versioned. They are separate
from internal analysis, planner, codec, storage, and CLI structs.

- `AnalysisReportV1` exposes bounded facts, sampling truth, planner reasons,
  and diagnostics without raw sample values.
- `CompressionResultV1` records the archive version and actual selected mode,
  byte sizes, `CodecBackendV1`, diagnostics, and operation profile.
- `DecompressionResultV1` records the detected archive version and mode,
  byte sizes, `CodecBackendV1`, applicable verification fact, diagnostics, and
  profile.
- `ValidationReportV1` records format-specific checks and optional
  against-source status.
- `ComparisonReportV1` records the factual two-contender measurement and
  exact validation contract.
- `BenchmarkReportV1` records the legacy Benchmark methodology without
  requiring callers to parse CLI text or flattened reason strings.

The V1 suffix versions the report shape. It is not a commitment to expose or
stabilize internal engine types.

### Codec backend facts

`CodecBackendV1` reports the backend that actually handled Compress or
Decompress. Its current values distinguish the in-memory v1 zstd path, the v1
streaming RawZstd path, the two v2 compression backends, and the universal v2
RawZstd-frame decoder. This is an execution fact; it does not add a wire
format or imply that backend-specific archives exist.

### Benchmark report

`BenchmarkScopeV1` is one of `EstimateOnly`, `Full`, `Partial`, or `Sampled`.
`BenchmarkValidationStatusV1` is one of `NotValidated`,
`PartiallyValidated`, or `Validated`. Callers should inspect both values:
scope describes what was measured, while validation status describes the
byte-identity evidence obtained for that scope.

The report exposes these performed-operation facts directly:

- `input_sampled`;
- `zstd_baseline_performed`;
- `roundtrip_performed`; and
- `hash_performed`.

Unavailable work is represented with `Option`, not a fabricated zero. The
report includes DataPack and standalone-zstd artifact sizes and ratios,
DataPack compression/decompression median times and MiB/s, standalone-zstd
compression time and MiB/s, SHA-256 match status, and the optional v2
candidate report. It also includes selected and estimated modes,
`plan_was_correct`, the planner memory estimate and planning time,
`columnar_candidate_error`, run count, total elapsed time, and the complete
`BenchmarkProfileV1` timing projection.

`partial_reasons` is a `Vec<String>`. Each reason remains a separate element,
including a reason whose text itself contains punctuation such as a semicolon;
callers never need to split a presentation string.

When `keep_artifacts` is false, every field in `BenchmarkArtifactsV1` is
`None` and owned temporary artifacts are cleaned up. When it is true, the
report returns paths for every artifact that was actually produced and
retained:

- `datapack`;
- `restored`;
- `zstd`;
- `chunked`;
- `chunked_restored`; and
- `chunked_sample`.

An artifact can remain `None` when its corresponding operation was skipped or
the selected benchmark execution path did not materialize that artifact. A
benchmark invocation reserves one collision-safe run identity, formed from
the input stem, process id, and a monotonically increasing in-process
sequence. All artifacts for that invocation share the identity and use
distinct suffixes, so concurrent or repeated in-process runs do not reuse the
legacy fixed names. `DATAPACK_TEMP_DIR` overrides the artifact root; otherwise
the input parent is used when available, with the system temporary directory
as the fallback.

Retained artifacts can include uncompressed restored bytes or a sampled input
prefix. Callers enabling `keep_artifacts` own the privacy and cleanup of every
returned path.

## Progress

Use the corresponding `*_with_progress` function with a mutable callback or a
`ProgressObserver` implementation:

```rust,no_run
use datapack::application::{self, CompressRequest, ProgressEvent};

fn main() -> datapack::error::Result<()> {
    let request = CompressRequest::new("input.csv", "input.dpack");
    let mut observer = |event: &ProgressEvent| {
        let _ = (
            event.operation,
            event.phase,
            event.state,
            event.completed_bytes,
            event.total_bytes,
        );
    };

    application::compress_with_progress(request, &mut observer)?;
    Ok(())
}
```

`ProgressEvent` contains typed `OperationKind`, `ProgressPhase`, and
`ProgressState` values plus absolute byte/item counters. It never contains a
path, row, field value, name, hash, diagnostic, or rendered message.

Totals are optional and are never represented by a sentinel value. Item
counters currently describe ordered v2 chunks when applicable. Use
`ProgressEvent::percentage()` to derive a percentage from integer byte facts;
unknown totals return `None`, and zero-byte work is handled without division by
zero. Elapsed time, throughput, and ETA remain adapter/presentation concerns.

Observers run synchronously on the operation's calling thread. Delivery
cadence is not a stable event-count promise, and a small phase may have only
lifecycle events. Streaming I/O uses deterministic bounded byte milestones;
v2 advances at ordered chunk-write milestones. Benchmark emits an outer
`Benchmarking` lifecycle. Each reported inner phase receives `Started`, zero or
more `Advanced` snapshots, and `Completed`; fast phases may omit `Advanced`.

The terminal `Finalizing/Completed` event is emitted only for a successfully
returned operation and, where applicable, only after transactional output
commit. A failed operation never emits that terminal-success fact. Earlier
phase completion does not claim that the complete operation succeeded.

Observer callback time is caller wall time and can be included in operation
duration. In particular, Benchmark progress callbacks may run while a measured
reader or writer is active and can perturb reported timing. Callers that need
the least callback-contaminated Benchmark measurements should use the silent
`benchmark()` entry point. Progress observation cannot cancel or otherwise
alter the operation.

An observer panic follows normal Rust unwinding. Transaction guards prevent a
pre-commit observer panic from publishing a partial final output. Progress is
not a cancellation or error-return channel.

See `docs/productization/PROGRESS_API.md` for exact operation/phase identifiers,
per-operation granularity, Python callback behavior, and the P4 boundary.

## Errors and outputs

Operational and format failures return `DatapackError`. Safe fallbacks and
non-fatal output-cleanup conditions can instead appear in a successful
operation's diagnostics.

Compress and Decompress use transactional sibling temporary files. A final
output is committed only after successful production and flushing. Existing
outputs require `overwrite`; `keep_partial` explicitly requests preservation
of an owned partial artifact after failure.

Analyze and Validate do not create result files. Compare and Benchmark manage
their own temporary artifacts according to their documented cleanup settings.

## Cancellation

Application API V1 has no cancellation token. Current bulk parser, codec,
zstd, validation, and legacy Benchmark calls do not provide uniform polling
points, so DataPack does not claim bounded-latency cooperative cancellation.
Progress callbacks are observational and must not be treated as cancellation
hooks.

Cancellation remains deferred until every long-running path can honor the
same cleanup, transactional-commit, and Benchmark timing invariants.

## Python consumer (non-normative)

The separate Python SDK foundation is a thin consumer of this Rust API. It
currently exposes Analyze, Compress, Decompress, Validate, and Compare and
converts the existing Rust reports to Python dictionaries; it does not define
a second engine or result schema. P3 adds optional structured progress
callbacks to those five calls by adapting these exact Rust facts. Benchmark,
Advisor, and cancellation are not part of the Python surface. This note does
not extend or change the normative Rust Application API V1 contract. See the
[Python SDK foundation](PYTHON_SDK_FOUNDATION.md).

## Compatibility

Using the application layer does not select a new wire format. It writes and
reads the existing frozen v1 and v2 formats, preserves RawZstd fallback, and
does not implement `.dpack` v3.
