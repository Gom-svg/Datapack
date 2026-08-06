# RFC-001B — Compatible Analysis Core

- Status: Phase 1 complete; all mandatory integrated quality gates passed
- Date: 2026-08-06
- Baseline: `178d6be21d2e636f37f40e32f1f15f753a6cce33`
- Baseline tag: `rfc-001b0-baseline`
- Predecessors: [RFC-001A](RFC-001A-intelligence-engine-audit.md) and
  [RFC-001B0](../testing/RFC-001B0-baseline-report.md)

## 1. Decision and authority

Phase 1 extracts the analysis behavior that already governs `analyze`,
non-chunked v1 `compress`, and `benchmark`. It separates observed facts,
legacy policy, and presentation without changing their externally observable
results:

```text
Path
  -> AnalysisEngine
  -> DatasetFacts
  -> PlannerFeaturesV1
  -> PlannerPolicyV1
  -> DatasetAnalysis { facts, columns, plan }
  -> consumer / legacy presentation
```

The current planner parser and arithmetic remain the compatibility authority.
The extraction does not promote the public legacy
`analysis::analyze_bytes(&Path, &[u8]) -> DpackMetadata` function into the new
engine, and it does not make the new factual model public. All new engine,
model, accumulator, compatibility-adapter, and policy types remain private or
`pub(crate)`.

RFC-001A and RFC-001B0 remain unchanged. If this record conflicts with a frozen
fixture, CLI golden, or an executable compatibility test, the executable
baseline wins and Phase 1 must stop for investigation.

## 2. Revalidated baseline

The modernization baseline was independently revalidated under WSL/Linux at
the declared MSRV:

| Component | Version / value |
|---|---|
| Branch | `main`, matching `origin/main` at revalidation |
| HEAD | `178d6be21d2e636f37f40e32f1f15f753a6cce33` |
| Tag | `rfc-001b0-baseline` |
| `rustc` | `1.85.0 (4d91de4e4 2025-02-17)` |
| Cargo | `1.85.0 (d73d2caf9 2024-12-31)` |
| rustfmt | `1.8.0-stable (4d91de4e48 2025-02-17)` |
| clippy | `0.1.85 (4d91de4e48 2025-02-17)` |
| Target directory | `/home/gompr/.cache/datapack-modernization` |

At that tag, formatting, checking, the full 196-test suite, and clippy with
warnings denied passed. The specific compatibility-fixture, analyze-CLI, and
planning gates also passed. Those results establish the input baseline; the
final integrated Phase 1 reruns are recorded in Section 10.

## 3. Scope: RFC-001A implementation commits 2–5

This implementation covers the four compatible slices proposed by RFC-001A.
The word "commit" below identifies the RFC-001A scope item rather than a
one-to-one landing commit. The dependency-coupled production slices landed
together in `840d4c2` so that the shared engine, policy bridge, consumers, and
their unit tests form one coherent change.

| RFC-001A slice | Implemented decision |
|---|---|
| Commit 2 — factual model | `analysis/model.rs` and `analysis/accumulator.rs` hold internal facts independent of `DpackMetadata`, planner recommendations, and CLI text. The legacy parser remains unchanged in behavior. |
| Commit 3 — engine and policy | `AnalysisEngine` owns the established sample scan. `PlannerFeaturesV1` adapts honest facts to legacy inputs, and `PlannerPolicyV1` owns the frozen profile and plan formulas. |
| Commit 4 — legacy renderer | The existing analyze text renderer moved to the private `cli/analysis_report.rs` module. Its formatting, labels, precision, censor marker, and fallback line remain frozen by the existing goldens. |
| Commit 5 — passive metrics | Physical empty counts, byte-length bounds, name diagnostics, coverage, cardinality exactness, and final-newline state are recorded but do not influence `PlannerPolicyV1`. |

The extraction does not add a fourth analyzer. Production has one authoritative
planner result; differential behavior is exercised only by tests.

## 4. Layer boundaries

### 4.1 Facts

`AnalysisEngine` preserves the legacy prefix validation, header handling,
comma-delimited physical-line parser, sample defaults, sample overshoot, error
types, and timing boundary. `AnalysisAccumulator` emits `DatasetFacts`:

- source and input identity;
- coverage and stop reason;
- columns identified by index;
- the physical header name and name diagnostics;
- observed values, physical byte lengths, numeric matches, and cardinality;
- passive empty/minimum/maximum metrics.

Facts do not contain `ArchiveMode`, `ColumnStrategy`, planner reasons, projected
sizes, or estimated savings.

### 4.2 Compatibility adapter

`PlannerFeaturesV1::from_facts` is the only bridge from facts to the established
planner feature semantics. In particular, it:

- uses `coverage.sampled_records` as the legacy sampled-row count;
- uses `coverage.bytes_read`, including characterized read overshoot, as the
  legacy sampled-byte count;
- preserves source size, mean length, total field bytes, numeric successes, and
  column index/name;
- maps a factual censored cardinality to the legacy sentinel
  `max(sampled_records, 8_193)` and marks the column censored.

This explicit adapter allows the factual result to say `AtLeast(8_193)` while
the existing renderer and policy continue to produce their frozen legacy
values, including the `>65535` presentation marker.

The adapter borrows `DatasetFacts` and projects column features lazily. It does
not allocate a second feature vector or clone column names before policy
evaluation. The returned factual model, legacy `ColumnProfile` values, and
`ColumnPlan` values deliberately remain separate result representations during
this compatibility phase; their name and reason allocations are excluded from
the fixed scalar-state evidence in Section 7 and from any total-memory claim.

### 4.3 Policy

`PlannerPolicyV1` contains the extracted `ColumnProfile` projection,
strategy-recommendation order, numeric/date heuristic, archive-mode selection,
estimated savings/memory arithmetic, reason strings, and `f32`/`f64`
conversions. The compatibility `build_plan` entry point delegates to it.

No passive metric is read by this policy. The thresholds and decision order are
unchanged, including the conservative RawZstd decisions characterized for the
Random and HighCardinality corpora even when a measured columnar candidate is
smaller. `apply_dictionary_limits` remains a separate legacy operation, and
the codec's independent `choose_column_mode` remains unchanged.

The engine also preserves the old timing behavior: prefix validation occurs
before the planning timer, and elapsed milliseconds are sampled once for plan
construction and again for the returned `planning_time_ms`.

### 4.4 Presentation

The private renderer consumes `DatasetAnalysis` and performs presentation only.
It retains the exact legacy stdout contract:

- default output versus `--plan` output;
- spacing, headings, ordering, and one-decimal rendering;
- 14-character display-name truncation;
- the censored `>65535` string;
- the RawZstd fallback line and exact planner reasons.

It does not infer facts or recalculate planner policy. Existing goldens continue
to normalize only the integer planning-time value.

## 5. Production consumers and chunked bypass

All three legacy planning consumers call the same private
`analysis::analyze_path` facade:

1. `datapack analyze` obtains one `DatasetAnalysis` and passes it to the legacy
   renderer.
2. Non-chunked v1 `datapack compress` obtains one `DatasetAnalysis`, clones its
   plan, applies the existing dictionary-limit compatibility step, and selects
   the established v1 encoder path.
3. `datapack benchmark`, including `--estimate-only`, obtains one
   `DatasetAnalysis` with the established 64 MiB planning sample before applying
   the benchmark measurement limit. This preserves the ordering characterized
   by RFC-001A.

`datapack compress --chunked` is deliberately different. The chunked branch
returns before analysis and writes the frozen v2 RawZstd format directly. It
therefore continues to accept non-CSV and invalid-UTF-8 binary input that the
legacy CSV planner rejects. The integration test verifies this bypass, v2 mode
identifier, chunk modes, verified restore, and byte identity.

The benchmark command still performs its legacy planning step even when a
chunked comparison is requested; this RFC does not change that behavior.

## 6. Exact passive fact semantics

### 6.1 Physical fields, empties, and UTF-8 lengths

Facts describe the `&str` slices produced by the unchanged legacy physical-line
parser, not normalized logical CSV values:

- `empty_values` increments only when the physical field slice has zero bytes;
- quoted `""` is two bytes and is not counted as empty;
- quotes and escaped-quote representation remain part of physical length;
- minimum, maximum, mean, and total lengths use UTF-8 byte counts (`str::len`),
  so `é` is two bytes;
- minimum and maximum are `None` when the column has no observed data values;
- the empty counter uses saturating addition.

Blank physical lines are analyzed for coverage bytes but do not increment the
sampled-record count or any column observations.

### 6.2 Cardinality

The established tracker retains at most 8,192 distinct stable hashes per
column. The factual representation is:

- `Exact(n)` while all observed hashes fit in the tracker;
- `AtLeast(8_193)` when a new distinct hash is observed after the tracker is
  full.

On censorship, the retained hash map is cleared as before. The factual lower
bound remains 8,193 regardless of later sample size. Only
`PlannerFeaturesV1` converts it to the larger legacy sentinel described above;
censored legacy repetition remains zero.

### 6.3 Coverage: bytes read versus bytes analyzed

Coverage separates I/O behavior from the bytes that contributed to facts:

- `bytes_read` includes the header and every physical line read from the
  `BufReader`, including a line read in full and then discarded after a byte
  budget overshoot;
- `bytes_analyzed` includes the header, blank lines within analyzed coverage,
  and data lines actually parsed; it excludes a discarded overshoot line;
- if the first data line itself crosses the byte budget, the legacy
  `sampled_records == 0` exception still analyzes that complete line;
- a record limit stops before reading the next physical line;
- `sampled_records` counts nonblank data lines accepted after the mandatory
  first-line header.

`stop_reason` is `Complete` when analyzed bytes cover the source,
`RecordLimit` when the row bound stopped the scan, and otherwise `ByteLimit`.
The configured maximum bytes and records are retained in coverage so a future
report can state the sampling contract without changing policy.

### 6.4 Final newline

`final_newline` is known only for complete analysis:

- `Some(true)` means the final analyzed physical line ended in LF, CRLF, or the
  legacy-recognized trailing CR;
- `Some(false)` means the completely analyzed source ended without a line
  ending;
- `None` means partial coverage cannot establish the source's final-newline
  state.

This is passive information and does not alter planning or encoding.

### 6.5 Column names

Column identity remains the zero-based index. The mandatory first physical line
is still interpreted as the header; no header inference is introduced. Matching
outer quotes are trimmed exactly as before. For each resulting name,
`ColumnNameStatus` records:

- whether the name is empty;
- the index of the first equal earlier name, if it is a duplicate.

The duplicate-name lookup is transient accumulator state. Empty or duplicate
names remain separate columns and do not change the legacy display name or
policy input.

## 7. Fixed scalar-state evidence

The passive per-column accumulator adds this fixed scalar state:

```text
empty_values         u64
min_value_len_bytes  u64
max_value_len_bytes  u64
                     ---
                      24 bytes
```

A unit test asserts
`size_of::<PassiveColumnMetrics>() == 3 * size_of::<u64>()`, which is 24 bytes
on the supported target. This is evidence only for those three scalar fields.
It explicitly excludes:

- the pre-existing per-column cardinality `HashMap` and its allocations;
- the column-name allocation;
- `ColumnFacts`, `DatasetFacts`, `DatasetAnalysis`, and other result DTOs;
- allocator and collection capacity overhead;
- the transient duplicate-name index;
- reader buffers, parser temporaries, and the rest of the process.

No RSS or working-set measurement was performed. The 24-byte assertion is not
a total-memory bound, throughput result, performance improvement, or evidence
that the legacy sampler is strictly memory bounded. In particular, the existing
8,192-hash budget is still per column rather than global, and oversized physical
headers/records retain their characterized overshoot behavior.

## 8. Frozen compatibility and non-goals

Phase 1 does not change:

- `.dpack` v1 or v2 magic, versions, fields, offsets, ordering, identifiers,
  bincode metadata, zstd payload interpretation, hashes, or validation rules;
- the `DCSV01` columnar payload format, parser, cost model, or byte-exact
  reconstruction check;
- `DpackMetadata`, `FileType`, `PayloadKind`, or any transitively serialized
  metadata type;
- frozen compatibility fixture bytes, sizes, SHA-256 values, or provenance;
- legacy analyze goldens, error messages, stdout/stderr routing, or exit codes;
- the public `analysis::analyze_bytes` signature or its CSV, TXT/log, and
  unknown-extension routing;
- planner thresholds, reason strings, candidate selection, dictionary-limit
  behavior, or final encoder fallback;
- default sample limits or the acceptance/error behavior of oversized records.

The following remain out of scope:

- replacing the line-local planner parser with the RFC-aware columnar parser;
- header inference, delimiter autodetection, TSV/PSV/semicolon compression, or
  multiline-record acceptance in the legacy planner;
- new default hard limits, global cardinality budgeting, or changed sample
  semantics;
- a public Rust analysis model, Python API, or JSON report contract;
- making `ColumnPlan` executable or aligning it with `choose_column_mode`;
- entropy/type/date inference, trial compression, or new performance claims.

A hash change in a frozen fixture is a compatibility event. Fixtures must never
be regenerated to make a test pass. Any compatibility failure, wire ambiguity,
corruption risk, unexplained regression, or mandatory gate failure stops this
phase.

## 9. Additive tests

The implementation adds tests without changing fixtures or goldens:

| Test location | Additive coverage |
|---|---|
| `analysis/accumulator.rs` | Saturating physical-empty counter and the exact three-`u64`/24-byte scalar-state assertion. |
| `planning/tests.rs` | Honest `AtLeast(8_193)` facts behind the legacy sentinel; physical empty and UTF-8 byte lengths; empty/duplicate header diagnostics; complete, byte-limited, and record-limited coverage; discarded overshoot; known and unknown final newline. Existing policy assertions remain in place. |
| `tests/analysis_core_consistency.rs` | Agreement among analyze, benchmark estimate-only, and non-chunked v1 compress; same-build deterministic v1 bytes; payload mode and byte-exact restore; chunked binary bypass and verified v2 restore; and separate legacy rendering of duplicate long column names with the frozen 14-character truncation. |
| `tests/legacy_analysis_api.rs` | Compile-time function signature assignment; exact CSV metadata; TXT and case-insensitive log routing; unknown-path text routing; and an in-memory bincode serialize/deserialize smoke check. It does not create or compare an archive. |

The existing `tests/analyze_cli.rs`, `tests/compatibility_fixtures.rs`, frozen
goldens, frozen archives, round-trip tests, and security-hardening tests remain
the compatibility authority.

## 10. Gate ledger

Only results actually observed at the time of this record are marked as passed.
An earlier partial checkpoint is not promoted to a final integrated result.

| Scope | Command / gate | Result | Notes |
|---|---|---|---|
| Frozen baseline | `cargo fmt --check` | PASS | Revalidated at `rfc-001b0-baseline` under Rust 1.85.0. |
| Frozen baseline | `cargo check` | PASS | Revalidated at the baseline tag. |
| Frozen baseline | `cargo test` | PASS | 196 passed, 0 failed. |
| Frozen baseline | `cargo clippy --all-targets --all-features -- -D warnings` | PASS | Revalidated at the baseline tag. |
| Phase 1 integrated | `cargo fmt --check` | PASS | Final combined committed tree. |
| Phase 1 integrated | `cargo check` | PASS | Final combined committed tree. |
| Phase 1 targeted | `cargo test analysis::accumulator` | PASS | 2 passed, 0 failed. |
| Phase 1 targeted | `cargo test planning::tests` | PASS | 26 passed, 0 failed, including the 22 frozen policy tests. |
| Phase 1 targeted | `cargo test --test analysis_core_consistency` | PASS | 3 passed, 0 failed. |
| Phase 1 targeted | `cargo test --test legacy_analysis_api` | PASS | 4 passed, 0 failed. |
| Phase 1 compatibility | `cargo test --test compatibility_fixtures` | PASS | 4 passed, 0 failed; frozen v1/v2 restore remained byte exact. |
| Phase 1 compatibility | `cargo test --test analyze_cli` | PASS | 9 passed, 0 failed; legacy goldens remained unchanged. |
| Phase 1 supporting | `cargo test --test round_trip` | PASS | 31 passed, 0 failed. |
| Phase 1 supporting | `cargo test --test security_hardening` | PASS | 69 passed, 0 failed. |
| Phase 1 full suite | `cargo test` | PASS | 209 passed, 0 failed. |
| Phase 1 clippy | `cargo clippy --all-targets --all-features -- -D warnings` | PASS | No warnings. |

All Cargo build and test commands for this program use:

```text
CARGO_TARGET_DIR="$HOME/.cache/datapack-modernization"
```

Every Phase 1 mandatory gate passed with the declared MSRV toolchain. No
snapshot, golden, frozen fixture, wire-format implementation, or expected
legacy policy value was changed.
