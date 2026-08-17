# RFC-006 — Performance and correctness regression suite

- Status: Implemented in Phase 16
- Scope: benchmark harness, deterministic workloads, correctness gates, and observational reporting
- Related: [Performance regression methodology](../reference/PERFORMANCE_REGRESSION_METHODOLOGY.md)

## Resume provenance

Phase 16 resumed from an incomplete uncommitted benchmark-harness scaffolding
state discovered after the clean Phase 15 checkpoint. The pre-existing diff
renamed the placeholder bench target and removed the placeholder source, but
the replacement harness had not yet been created. The changes were
forensically inspected before being explicitly adopted.

That partial state was not certified. It changed only the Cargo bench target:
`size_placeholder` became `performance_regression`, with an explicit source
path, `harness = false`, and `test = false`. It added no dependency and made no
runtime or archive-format change.

## Decision

DataPack has two distinct evidence classes:

| Class | May fail a deterministic gate? | Examples |
| --- | --- | --- |
| Correctness regression | Yes | source identity, selected format/mode, restoration identity, validation results, v2 chunk structure, same-run artifact stability |
| Performance observation | No timing threshold | elapsed samples, median duration, throughput, ratio, environment and filesystem context |

The suite never turns a slow run into a correctness failure. A corrupt,
structurally invalid, non-byte-exact, unexpectedly routed, or same-run
nondeterministic result is an error and produces no successful report.

## Architecture

`benches/performance_regression.rs` is a small opt-in executable. It parses
only suite controls, creates an isolated workspace, calls the shared runner,
and emits JSON. `benches/support/mod.rs` owns scenario definitions,
environment capture, correctness assertions, timing collection, and report
DTOs. `tests/performance_regression.rs` includes that same support source for
focused regression coverage.

The support code calls existing production boundaries:

```text
generation::generate_to_path
        |
        v
application::compress
        |
        +--> application::decompress
        |
        +--> application::validate --against generated source

application::compare (separate factual-contract regression)
```

It does not parse CLI output and does not implement a compressor, planner,
validator, comparer, archive parser, or alternate result model for those
operations. Keeping support below `benches/` avoids adding a benchmark-only
module to the public Rust Application API.

## Dependency decision

No benchmark dependency is added. Criterion would be useful for repeated
in-process microbenchmarks, statistical outlier handling, and instruction-level
work, but those are not this suite's file-to-file mission. Existing Rust 1.85
compatible facilities are sufficient:

- `Instant` for elapsed observations;
- the existing `serde` and `serde_json` dependencies for typed JSON;
- the existing `sha2` dependency for file identities;
- the existing `tempfile` development dependency for isolated workspaces; and
- the existing deterministic generator and Application API for execution.

Consequently `Cargo.lock` does not change.

## Deterministic workloads

Both presets use the repository generator and explicit seeds. No generated
dataset or archive is tracked.

### Smoke preset

The smoke preset is suitable for routine regression execution:

| Scenario | Profile | Rows | Seed | Archive expectation |
| --- | --- | ---:| ---:| --- |
| `repetitive_structured_v1` | repetitive | 2,000 | 42 | v1 `CsvColumnarDictionary` |
| `realistic_structured_v1` | realistic | 1,500 | 2,026 | v1 `CsvColumnarDictionary` |
| `high_cardinality_v1` | high-cardinality | 1,000 | 7 | v1 `RawZstd` |
| `random_low_redundancy_v1` | random | 1,000 | 99 | v1 `RawZstd` |
| `realistic_v2_multichunk` | realistic | 4,000 | 20,260,316 | v2 chunked RawZstd, 32 KiB chunks, 2 workers, 2 in flight |

The smoke source byte lengths and SHA-256 identities are committed regression
facts. These freeze deterministic generator outputs for the recorded profile,
row count, and seed. Archive hashes are deliberately not frozen across
DataPack or zstd releases.

### Representative preset

The representative preset is manual and opt-in:

| Scenario | Rows | Seed | Relevant configuration |
| --- | ---:| ---:| --- |
| repetitive structured v1 | 50,000 | 42 | planner-selected structured v1 |
| realistic structured v1 | 25,000 | 2,026 | planner-selected structured v1 |
| high-cardinality v1 | 15,000 | 7 | RawZstd fallback |
| random/low-redundancy v1 | 25,000 | 99 | RawZstd fallback |
| realistic v2 multi-chunk | 75,000 | 20,260,316 | 1 MiB chunks, 4 workers, 8 in flight |

These sizes exercise useful file-to-file work without adding giant fixtures or
reconstructing any historical external dataset.

## Correctness gates

Every scenario records and checks:

- generated source size and SHA-256;
- expected archive version and intentionally fixed selected mode;
- complete archive size and SHA-256 for each run;
- identical archive size and bytes across repeated runs in the same build;
- restored size and SHA-256;
- an independent streaming byte-for-byte file comparison;
- `validate --against` success and source match;
- header, metadata, payload structure, decompression, and restored-length
  validation status; and
- the format-specific validation capability surface.

V2 additionally requires more than one chunk, the arithmetically expected
chunk count, and passed chunk-table, per-chunk SHA-256, global SHA-256, and
trailing-data checks. V1 accurately records its frozen-format limitations:
the chunk table is not applicable, and stored per-chunk/global hashes and an
exact trailing-data boundary are unavailable.

The same-build artifact check guards promised deterministic execution for a
single DataPack/zstd build. It does not promise that a later zstd or DataPack
release emits the same frame bytes. The frozen v1/v2 compatibility archives
remain the cross-change wire-compatibility gates.

The focused tests also run full-scope Compare and require its factual scope,
per-run SHA-256 artifact-stability method, per-run restoration validation, and
validated contenders to remain intact. Existing Python tests continue to
prove that Python delegates to the Rust Application API; the benchmark suite
does not introduce Python compression logic.

## Observational measurements

Compression timing surrounds the complete `application::compress` call;
decompression timing surrounds the complete `application::decompress` call.
Generation, source hashing, archive hashing, byte comparison, and validation
are outside those timed regions. The report retains every sample and a median.
Throughput uses source bytes divided by the median elapsed seconds.

There are no hidden warm-ups. No cache is flushed, no CPU governor or thermal
state is controlled, and no system-load isolation is claimed. Later runs may
benefit from filesystem caches. These limitations are serialized in every
report.

## Environment and filesystem metadata

Each JSON report records, where available:

- UTC Unix timestamp, Git commit and dirty state;
- Rust and Cargo versions and optimized/debug build state;
- OS, kernel, architecture, CPU model, logical CPUs, and Linux memory facts;
- WSL detection and distribution;
- workspace, source, archive, and report-output path classifications; and
- optional operator-supplied GPU inventory.

Filesystem classes distinguish WSL Windows-drive mounts such as `/mnt/c`, WSL
Linux-native paths such as `/tmp`, native Linux, and native Windows. This is a
path/environment classification, not a promise that the exact physical
filesystem type can be discovered portably.

`DATAPACK_PERF_GPU_INFO` may record inventory text. It is informational only;
the report always sets `gpu_used_by_datapack` to false because current
DataPack execution is CPU-based. No GPU code is introduced.

## Report contract

The JSON root is
`performance_regression_observation` with `schema_version = 1`. It explicitly
labels correctness as `deterministic_ci_gate`, timings as
`observational_only`, and `timing_failure_thresholds` as false. Results can be
written outside the repository with `--output`; JSON is also printed to
standard output.

```bash
cargo bench --locked --bench performance_regression -- \
  --preset smoke --runs 2 \
  --output /tmp/datapack-performance-smoke.json

cargo bench --locked --bench performance_regression -- \
  --preset representative --runs 3 \
  --work-dir /path/to/classified/storage \
  --output /tmp/datapack-performance-representative.json
```

`cargo bench` uses Cargo's optimized bench profile. Debug runs may diagnose
behavior but are not official performance observations.

## Historical and external evidence boundaries

Historical benchmark tables and the historical 7,392,492,161-byte external
run remain evidence from their recorded environments. This suite neither
rewrites those values nor requires the unavailable source.

External Python Beta Test 001 remains separate technical-beta integrity
evidence: a 1,708,674,492-byte source produced a 360,142,313-byte, 26-chunk v2
archive and passed validation, SHA-256 identity, and independent byte
comparison. Its dev-profile wheel, WSL, `/mnt/c`, NTFS, and OneDrive timing
conditions make its elapsed values unsuitable as an official baseline. The
source is not tracked and CI does not depend on it.

## Compatibility and security

Phase 16 does not modify `src/`, v1/v2 writers or readers, metadata,
compression policy, Python bindings, frozen fixtures, or analyze goldens. The
bench target is excluded from normal `cargo test` discovery by `test = false`;
the small focused integration tests intentionally exercise the shared support.
All generated files live in an isolated temporary directory and are removed by
the existing `tempfile` lifecycle.

No wall-clock value participates in a pass/fail comparison. No unbounded
scenario is accepted: preset row counts are fixed, generated rows remain under
the generator's existing limit, run count is limited to 1 through 25, v2
in-flight work is explicit, and application resource checks remain active.

## Limitations

- The suite is a reproducibility and regression framework, not a laboratory
  benchmark controller.
- It does not evict caches, pin CPUs, suppress other processes, read thermal
  sensors, or claim physical-media persistence.
- Filesystem classification is conservative and cannot identify every mount,
  network filesystem, overlay, or Windows storage configuration.
- The representative preset is deliberately modest; it does not replace
  large-file release qualification.
- Structured v1 and chunked v2 optimize different properties. Phase 16 does
  not compare them as interchangeable formats or change either format.
- Future structured chunking belongs to the v3 design and implementation
  program, not mutation of v1 or v2.
