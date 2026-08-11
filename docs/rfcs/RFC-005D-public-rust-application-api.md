# RFC-005D — Public Rust Application API

- Status: Implemented
- Date: 2026-08-06
- Baseline: `rfc-011-datapack-advisor`
- Related: [Application API V1](../reference/APPLICATION_API_V1.md)

## 1. Decision

Phase 12 introduces `datapack::application`, a public, path-based service
boundary for these existing operations:

- Analyze;
- Compress;
- Decompress;
- Validate;
- Compare; and
- the legacy Benchmark workflow.

Each operation accepts an owned request and returns a versioned report or
result through `datapack::error::Result`. A second entry point accepts a typed
progress observer. The service layer does not depend on Clap, read terminal
state, or write to stdout or stderr. Command-line parsing and presentation
remain in `src/cli`.

Advisor is not part of the Phase 12 public service surface. Its Phase 11
policy and DTO remain internal; this RFC does not imply a public Advisor API.

Phase 12 does not introduce an in-memory or stream-oriented application API.
Requests identify filesystem paths and use explicit byte or count limits.
This boundary is intended to support later bindings and applications without
making command-line types part of their contract.

## 2. Public operations

The public module provides a silent entry point and an observed entry point
for every operation:

| Operation | Request | Result |
|---|---|---|
| `analyze` / `analyze_with_progress` | `AnalyzeRequest` | `AnalysisReportV1` |
| `compress` / `compress_with_progress` | `CompressRequest` | `CompressionResultV1` |
| `decompress` / `decompress_with_progress` | `DecompressRequest` | `DecompressionResultV1` |
| `validate` / `validate_with_progress` | `ValidateRequest` | `ValidationReportV1` |
| `compare` / `compare_with_progress` | `CompareRequest` | `ComparisonReportV1` |
| `benchmark` / `benchmark_with_progress` | `BenchmarkRequest` | `BenchmarkReportV1` |

Requests own their `PathBuf` values and configuration. Constructors establish
the same defaults used by the corresponding normal CLI path where applicable;
callers can then change public request fields explicitly. Public request,
result, option, and progress types are non-exhaustive so consumers must not
assume that their current fields or variants are the complete future set.

Report and result names carry `V1` where they define a machine-consumable
versioned boundary. This version label does not promise that every internal
planner, storage, profile, or codec type is public or stable. Internal facts
are converted to dedicated DTOs rather than exposed directly.

## 3. Operation semantics

### 3.1 Analyze

Analyze uses the shared bounded, dialect-aware analysis engine and returns
`AnalysisReportV1`. It does not return `DatasetAnalysis`, `DatasetFacts`, or a
Clap command type. Sampling completeness, limitations, cardinality bounds,
planner policy, and diagnostics retain the semantics documented by Analysis
JSON V1.

### 3.2 Compress

Compress supports an explicit `CompressionFormat`:

- v1 uses `V1CompressionOptions`, the compatible analysis and
  `PlannerPolicyV1` path, execution-plan enforcement, and safe RawZstd
  fallback; and
- v2 uses `V2CompressionOptions` and the existing chunked RawZstd writer.

`CompressionResultV1` reports the archive version and mode actually written,
sizes, the actual `CodecBackendV1`, operation diagnostics, and observed
profile facts. `CodecBackendV1` distinguishes v1 in-memory zstd, v1 streaming
RawZstd, the v2 chunked compression backends, and v2 RawZstd-frame decoding.
It is an execution fact, not a new archive variant. The request does not
expose the frozen serialized metadata graph.

The default request is v1 with the existing Fast-mode defaults. Selecting v2
is explicit. Phase 12 does not enable structured v2 storage or add an archive
variant.

For v2 compression, `max_memory_bytes` is a configuration admission check:
`chunk_size_bytes * max_in_flight_chunks` must not exceed it. It is not an
absolute RSS ceiling and does not include allocator, zstd, thread-stack, or
other process overhead.

### 3.3 Decompress

Decompress detects v1 or v2 from the archive and writes the restored bytes to
the requested path. `DecompressionResultV1` distinguishes the actual archive
version, payload mode, and `CodecBackendV1`. Its `verified` field is optional
because the frozen v1 and v2 formats do not provide identical integrity
guarantees.

Configured output, chunk-count, and memory limits are request values in bytes
or counts. They are independent of CLI spelling such as MiB flags. The chunk
count limit applies only to v2, whose wire format has a chunk table; it is
inapplicable to v1.

### 3.4 Validate

Validate calls the Phase 9 validation engine and returns
`ValidationReportV1`. It does not create a restored output. An optional
`against` path enables exact comparison with source bytes. The report retains
the documented distinction between v1 and v2 validation guarantees. Its
`max_chunks` request value likewise applies only to v2 and is inapplicable to
v1.

### 3.5 Compare

Compare returns the existing `ComparisonReportV1` for Quick or Full mode. It
retains the Phase 10 immutable-snapshot, timing, validation, and cleanup
methodology. The application API does not add a score, confidence value, or
performance prediction.

### 3.6 Legacy Benchmark

Benchmark exposes the historical, configurable Benchmark workflow as a typed
service for compatibility. `BenchmarkRequest` represents its quick,
estimate-only, prefix, optional standalone-zstd, optional round-trip/hash, and
optional v2-candidate controls. `BenchmarkReportV1` labels the measured scope,
validation status, structural partial reasons, and which optional measurements
were performed. Scope and validation are typed as `BenchmarkScopeV1` and
`BenchmarkValidationStatusV1`, rather than caller-parsed strings.

The report preserves the legacy measurement facts needed by non-terminal
callers: performed-operation flags; primary DataPack and standalone-zstd
sizes, ratios, timings, and throughput; optional decompression and SHA-256
facts; planner selection, correctness, memory estimate, and timing; total and
profile timings; optional chunked-candidate facts; and any structured encoder
fallback error. Fields for work that was not performed are optional rather
than populated with synthetic measurements.

`partial_reasons` is a vector whose elements preserve reason boundaries. It is
not a semicolon-delimited presentation field. `BenchmarkArtifactsV1` returns
all retained DataPack, restored, standalone-zstd, v2 chunked, v2 restored, and
sample-prefix paths. Paths are present only when `keep_artifacts` requested
retention and the corresponding artifact was produced.

Each Benchmark invocation reserves a collision-safe identity containing the
input stem, process id, and a monotonically increasing in-process sequence.
All six possible paths share that identity and have distinct suffixes. This
prevents concurrent or repeated in-process invocations from sharing the old
fixed temporary names while preserving deterministic ownership and cleanup of
the paths reserved by one invocation.

Retention is explicit because restored and sampled artifacts can contain
uncompressed source bytes. A caller that sets `keep_artifacts` owns their
privacy and cleanup after the report returns.

This does not merge Benchmark with Compare or change either methodology.
Compare remains the narrower user-focused factual comparison. Benchmark
retains legacy partial and opt-out modes, which callers must interpret from
the returned scope, validation status, and partial reasons.

## 4. Progress contract

Every service has a `*_with_progress` form accepting
`&mut dyn ProgressObserver`. The observer receives `ProgressEvent` values with
typed operation, phase, and lifecycle enums plus raw byte and item counters.

Progress events are deliberately privacy-safe. They contain no paths, file or
column names, rows, field values, hashes, diagnostic messages, or rendered
text. An event reports only:

- the operation and phase;
- `started`, `advanced`, or `completed` state;
- completed and optional total bytes; and
- completed and optional total items.

Callbacks are synchronous and run on the operation's calling thread. The v2
parallel storage pipeline translates worker counters only from its ordered
caller-side writer; it does not call application observers from worker
threads. Callback execution time can contribute to total wall-clock duration,
so observers should return promptly.

Benchmark emits an outer `Benchmarking` start/completion pair. Each inner
phase receives a `started` event, zero or more `advanced` snapshots, and a
`completed` event; a fast phase can complete without an `advanced` snapshot.
Callbacks for measured readers or writers can run inside a measurement
boundary, so their wall time can perturb reported Benchmark timing. Consumers
that need the least callback-contaminated measurement should call silent
`benchmark()` rather than `benchmark_with_progress()`.

The contract does not guarantee an `advanced` event for every buffer, record,
or chunk, nor a fixed reporting interval. Callers must use lifecycle state and
absolute counters rather than count events. Calling the entry point without
`_with_progress` is equivalent to using a silent observer.

Progress is observational. Returning from `on_event` does not cancel, pause,
or alter an operation.

## 5. Terminal and CLI boundary

The application module and the storage paths it invokes do not print status,
warnings, profiles, JSON, or errors. Expected non-fatal conditions, including
safe structured fallback and a committed-output backup-cleanup failure, are
returned as typed result diagnostics. Fatal conditions remain
`DatapackError` values.

The CLI converts Clap values into application requests or equivalent
crate-private service requests and renders returned reports, diagnostics,
profiles, and progress. Analyze retains a crate-private application adapter
that also supplies its frozen legacy text renderer with internal display
facts. The legacy Benchmark CLI and public Benchmark facade share the same
extracted terminal-free benchmark engine; the CLI consumes additional
crate-private execution details needed by its historical renderer. These
adapters do not duplicate analysis, compression, validation, comparison, or
benchmark algorithms.

Legacy text renderers remain CLI presentation code. This keeps command names,
flags, defaults, exit behavior, and legacy Analyze output out of the reusable
core.

No application request or result contains a `clap::Args`, `clap::ValueEnum`,
or another parser-owned type.

## 6. Output transactionality

Compress and Decompress continue to use sibling temporary outputs. A final
path is installed only after the operation has produced and flushed a complete
result. Existing regular files are protected unless `overwrite` is true.
Ordinary failures leave the previous final output intact or leave no final
output. `keep_partial` can explicitly preserve an owned partial artifact for
diagnosis.

Failure to remove the backup of a replaced output after a successful commit
does not reverse the completed commit. It is returned as the
`OUTPUT_BACKUP_CLEANUP_FAILED` diagnostic so a non-terminal caller can decide
how to present it.

Analyze and Validate do not create result files. Compare and Benchmark use
owned temporary workspaces or artifacts according to their documented
methodologies and cleanup settings.

Benchmark reserves a per-invocation family of collision-safe artifact paths.
Absent `keep_artifacts`, the owning guard removes that family at the end of
the run. With retention enabled, `BenchmarkArtifactsV1` reports every path
actually produced; skipped or in-memory-only artifacts remain `None`.

## 7. Cancellation decision

Phase 12 does not expose a cancellation token or claim bounded-latency
cooperative cancellation.

The current engine contains bulk operations without a uniform cancellation
checkpoint, including structured v1 encode/restore work, zstd frame calls,
parts of bounded parsing and validation, and legacy Benchmark measurement
steps. Adding a token only between outer phases would allow an operation to
continue for an unbounded interval after cancellation was requested. It could
also perturb Benchmark timing if checks or callbacks were inserted inside a
measured region without a separate methodology decision.

Transactional output protects final paths on ordinary returned errors, but it
does not by itself define cancellation timing, cleanup, or the race between a
cancellation request and final commit. A future cancellation contract must
add polling to every potentially long-running codec, parser, hashing, chunk,
and benchmark loop; define a cancellation error; explicitly clean owned
workspaces; and specify that a completed commit remains a successful
operation. Until those invariants can be tested across all six services,
cancellation remains deferred to productization work.

## 8. Compatibility and security

The application layer is orchestration and DTO conversion. Phase 12 does not
change:

- the v1 serialized metadata graph, header, or payload representation;
- the v2 fixed header, chunk table, or payload representation;
- frozen v1/v2 compatibility fixtures;
- legacy Analyze goldens;
- `PlannerPolicyV1` decisions;
- structured-compression eligibility or RawZstd fallback authority;
- compression defaults under existing CLI commands;
- Compare or legacy Benchmark methodology; or
- the Cargo dependency graph.

Progress events never contain user data. Application operations retain
existing bounded-analysis, validation, and decompression limits. Chunk-count
limits apply to v2 only; v1 has no chunk-table semantic to limit. The v2
compression memory value is an admission bound on configured chunk bytes in
flight, not a claim to cap complete process RSS. Phase 12 adds no unsafe code
and no v3 writer or reader behavior.

## 9. Verification

Phase 12 tests call the public services directly, rather than constructing
Clap commands. They cover request defaults, report versions, successful
Analyze/Compress/Decompress/Validate/Compare/Benchmark calls, exact restored
bytes, transactional overwrite protection, progress lifecycle and privacy
shape, and terminal-free service execution. CLI regression tests verify that
commands continue to use the shared service engines and adapters while
retaining their existing output contracts.

All frozen compatibility, legacy Analyze, round-trip, security-hardening, and
planner characterization suites remain mandatory certification gates.

## 10. Limitations

- The application API is path-based; byte-slice, generic `Read`/`Write`, and
  asynchronous services are not part of V1.
- Cancellation is explicitly deferred and no cancellation latency is claimed.
- Progress delivery is synchronous and its cadence is not a performance
  guarantee; observers can add wall time, including to Benchmark measurements.
- Benchmark retains historical optional-validation modes; callers must inspect
  its returned scope and validation facts.
- Advisor is not exposed as a public Phase 12 service.
- Versioned DTOs do not make internal planner, codec, storage, or CLI types
  public.
