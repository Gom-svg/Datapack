# RFC-003 — Hard-Bounded Analysis and Sampling

- Status: Phase 4 complete; all mandatory integrated quality gates passed
- Date: 2026-08-06
- Baseline: `rfc-003-analysis-json`
- Related: [RFC-001B](RFC-001B-compatible-analysis-core.md),
  [RFC-002A](RFC-002A-structured-analysis-json.md), and
  [Analysis JSON V1](../reference/ANALYSIS_JSON_V1.md)

## 1. Decision

Phase 4 replaces the compatible analyzer's unbounded `read_line` allocation
and independent per-column cardinality budgets with a bounded byte-oriented
physical-record reader and a shared cardinality-entry budget.

The normal compatible pipeline remains:

```text
bounded physical-record reader
  -> unchanged comma/quote compatibility parser
  -> bounded AnalysisAccumulator
  -> DatasetFacts
  -> PlannerFeaturesV1
  -> PlannerPolicyV1
```

When a structural hard limit prevents safe planning, the result remains a
successful but explicitly limited `DatasetAnalysis`. Its archive recommendation
is replaced with a safe RawZstd fallback. `analyze` can therefore report the
limit, while non-chunked `compress` can still archive the original bytes.

This phase does not introduce the shared logical-record/dialect parser planned
for Phase 5. It does not reinterpret quoted multiline input or change normal
legacy CSV decisions.

## 2. Default internal limits

| Limit | Default | Meaning |
|---|---:|---|
| sample bytes | `--sample-mb`, 1–2048 MiB | Maximum bytes consumed by the bounded scanner for one analysis scope |
| sample records | 10,000 | Existing maximum accepted nonblank data records |
| header bytes | 1 MiB | Maximum first physical record, including line ending |
| record bytes | 8 MiB | Maximum later physical record, including line ending |
| columns | 4,096 | Maximum fields retained from the header |
| per-column cardinality entries | 8,192 | Existing compatibility threshold |
| global cardinality entries | 262,144 | Maximum hashes retained concurrently across all columns |
| analysis accounting budget | 64 MiB | Budget for bounded record capacity, retained header/column state, and cardinality-entry charges |

Limit and accounting arithmetic uses checked or saturating operations. Limits
are internal in Phase 4; no new CLI flags or public Rust API are added.

The accounting budget is not a process-RSS claim. Allocator bookkeeping,
library buffers, the returned planner/report DTOs, and unrelated process state
are not measurable through this accounting. The hard safety claim comes from
the individual byte, column, record, row, and entry caps: every source-shaped
retained collection has a fixed maximum independent of source size.

The accounting model uses these conservative logical charges:

```text
base = max(max_header_bytes, max_record_bytes)
     + 2 * retained_header_bytes
     + 512 * column_count

global_cardinality_capacity = min(
    262144,
    (64 MiB - base) / 32
)
```

If a checked conversion, multiplication, addition, or subtraction fails, the
base does not fit and analysis returns a typed memory limitation. The 32-byte
entry charge and 512-byte column charge are deterministic admission values,
not measurements of `HashMap` buckets, allocator metadata, or RSS. Tests verify
these decisions; the individual byte, row, column, and entry caps provide the
source-size-independent bound.

## 3. Bounded physical-record reader

The reader operates on bytes and keeps at most the applicable header or record
limit in its reusable buffer. It uses a limited view of the existing fixed-size
`BufReader` and searches for LF without asking `read_until` or `read_line` to
grow past the selected cap.

At an exact cap without LF, the reader peeks through `BufRead::fill_buf` to
distinguish end-of-file from additional source data. Peeking may populate the
fixed `BufReader` buffer but does not consume or retain more source-shaped
record data. Coverage counts consumed source bytes, not kernel or `BufReader`
prefetch.

The effective per-read cap is the minimum of:

- remaining sample bytes;
- remaining benchmark planning scope, when one exists;
- the header or data-record hard limit.

Consequences:

- `coverage.bytes_read` never exceeds the effective sample/planning scope;
- `coverage.bytes_analyzed` excludes an incomplete record fragment;
- an exact-size unterminated final record is accepted when EOF proves it is
  complete;
- a line ending beyond the cap does not make an oversized record acceptable;
- CRLF and the compatibility parser's trailing-CR behavior remain unchanged;
- UTF-8 conversion occurs only after a complete bounded record is available.

When the remaining sample budget exactly equals the structural header/record
cap, the structural cap has precedence. LF at that exact cap is accepted; an
additional byte produces the typed hard-limit outcome.

The mandatory fixed 4 KiB prefix validation remains unchanged in this phase.
It occurs before the timed/scoped scanner, can reread that prefix, and is not
included in `coverage.bytes_read`.

## 4. Bounded field parsing

The comma/quote state machine and physical-field semantics remain compatible.
Field counting is separated from field retention:

- the parser counts all fields in a bounded physical record;
- it retains at most the configured maximum number of field slices;
- normal inconsistent-width errors retain their exact observed count;
- a header above 4,096 fields becomes a limited outcome without allocating an
  accumulator per field;
- data rows cannot force an unbounded vector merely by containing delimiters.

Column indexes, quote handling, escaped quotes, empty fields, header trimming,
and every normal PlannerPolicyV1 input remain unchanged.

The factual model records an optional observed header column count. It is
`null` in JSON only when the header could not be read completely. It may be
larger than the returned column array when a column or memory limit prevented
column fact construction.

## 5. Shared cardinality budget

The established per-column tracker retains no more than 8,192 stable hashes.
Phase 4 additionally shares a concurrent entry budget across all columns.

For a newly observed hash:

1. an already-retained hash consumes no budget;
2. a column below its local threshold reserves one global entry;
3. a column crossing the local threshold becomes `AtLeast(8193)`, clears its
   retained set, and releases those global entries;
4. if no global entry is available first, that column becomes
   `AtLeast(retained + 1)`, clears its set, and releases its retained entries;
5. observation continues for other scalar facts and later records.

Only global-budget exhaustion marks the whole analysis memory-limited and
forces RawZstd. Crossing the established per-column threshold alone retains
its RFC-001B behavior and does not change otherwise-compatible policy choices.

`PlannerFeaturesV1` continues to map every censored factual cardinality through
the frozen compatibility sentinel. JSON continues to emit the factual lower
bound, never a fake exact count.

## 6. Limited outcomes

The internal result records one or more typed limitations:

- incomplete header at the sample/scope boundary;
- header byte limit reached;
- record byte limit reached;
- column limit reached;
- analysis accounting budget reached;
- global cardinality-entry budget reached.

Coverage records one terminal stop reason, while the facts can also contain one
or more typed limitations. For example, a full byte scan can have partial
cardinality facts, while a byte-sample stop can coexist with cardinality
censorship.

Analysis JSON V1 reports:

- `scope` from source-byte coverage;
- `completeness = partial` whenever a hard limitation exists;
- `limited = true` for any hard limitation or ordinary sample stop;
- the terminal stop reason in `limit_reached`, or the first hard limitation
  when the byte scan completed; concurrent limitations remain visible as
  diagnostics;
- stable diagnostics for every hard limitation;
- `final_newline = null` unless every source byte was consumed;
- planner `selection_scope = safe_fallback` when the hard-limit adapter, rather
  than PlannerPolicyV1, requires RawZstd.

No path, filename, header name, or row value is added to a diagnostic. Phase 4
retains `schema_version: 1` because later modernization phases extend the
authorized `AnalysisReportV1` boundary. Its code domains are explicitly open,
and `column_count` is now documented as integer-or-null for a bounded incomplete
header. In-bound Phase 3 reports keep the same keys and value types.

The legacy text renderer adds an `Analysis limited:` line only for these new
hard-limit outcomes. Existing successful and failing golden cases do not enter
that branch and must remain byte-identical.

## 7. Compression fallback

Non-chunked v1 compression behaves as follows:

1. a normal analysis continues through PlannerPolicyV1 and existing mode/
   `--verify-best` behavior;
2. a typed hard-limited analysis immediately selects streaming v1 RawZstd;
3. an input rejected as structured CSV by the compatibility analyzer also
   selects streaming v1 RawZstd;
4. invalid CLI limits, path errors, and unrelated I/O failures remain errors;
5. the RawZstd writer, transactional output, force policy, profiling, and
   frozen v1 bytes remain unchanged.

The fallback never sends an analysis-limited input to the columnar writer and
does not read the entire input merely to compare unsafe structured candidates.
Chunked v2 already bypasses analysis and is unchanged.

`analyze` itself retains established malformed/unsupported CSV errors. The
RawZstd recovery is a compression adapter decision, not a claim that malformed
input has structured facts.

## 8. Benchmark planning scope

`benchmark --max-input-mb N` is validated before planning. The benchmark then
uses:

```text
measured scope = min(source size, N MiB)
planning sample = min(measured scope, 64 MiB)
```

The analysis facts retain the full source size, record the measured planning
scope separately, and PlannerPolicyV1 projects only to that scope. Planning
therefore never consumes more bytes than the prefix the benchmark claims to
measure.

Without `--max-input-mb`, the established 64 MiB planning default remains.
Benchmark timing, compression runs, median calculations, validation, and
temporary-file methodology are otherwise unchanged.

## 9. Compatibility boundaries

Phase 4 does not change:

- `.dpack` v1 or v2 serialized representation;
- v1/v2 readers, writers, hashes, or payload layouts;
- frozen fixture bytes;
- normal legacy analyze text or its nine goldens;
- `PlannerPolicyV1` thresholds, normal-range formulas, decision order, or
  reason strings; aggregate projected sizes now saturate instead of panicking
  on overflow;
- the public legacy `analysis::analyze_bytes` API;
- columnar codec parsing or byte restoration;
- CLI flag names and default values for accepted, in-limit legacy CSV;
- benchmark run counts, codecs, timers, medians, or validation methodology;
- Cargo dependencies.

Benchmark projection deliberately uses the measured prefix rather than the full
source when `--max-input-mb` is supplied. Strict byte sampling also removes the
previous discarded-line overshoot from planner scale at a sample boundary.
Malformed/unsupported non-chunked compression deliberately changes from an
analysis error to transactional RawZstd recovery. These are the scoped Phase 4
behavior changes, not silent compatibility drift.

The RFC-001B characterization tests that demonstrated nominal overshoot are
updated into hard-bound assertions; they are not deleted. Known parser
divergence tests remain.

## 10. Verification plan

Focused tests cover:

- a header above 1 MiB with bounded read coverage and RawZstd compression;
- a data record above 8 MiB, including an unterminated case;
- an exact header/record/sample boundary;
- more than 4,096 columns without per-column allocation amplification;
- low injected analysis-memory and global-cardinality budgets;
- truthful JSON coverage, diagnostics, nullable fields, and safe-fallback
  planner scope;
- benchmark planning bytes at or below `--max-input-mb`;
- malformed structured input archived through streaming RawZstd;
- byte-exact restoration of every hard-limit/malformed fallback archive
  produced by the focused CLI tests;
- unchanged nine legacy analyze goldens;
- unchanged compatibility fixtures, deterministic v1 behavior, round-trip,
  security, planner characterization, and full quality gates.

No benchmark performance result is produced by this phase. The repository is
under `/mnt/c` in WSL, so any incidental timings include WSL/NTFS effects and
are not evidence for a performance claim.

Implementation is confined to the private analysis model/engine/report,
PlannerPolicyV1 adapter arithmetic, CLI analyze/compress/benchmark adapters,
focused tests, and documentation. Storage, metadata, manifests, fixtures, and
goldens are not implementation surfaces for this phase.

## 11. Implementation and certification

The implementation landed in these production surfaces:

- `src/analysis/engine.rs`: bounded byte reader, explicit limits, coverage
  scope, typed limited outcomes, and safe plan adapter;
- `src/analysis/accumulator.rs`: shared cardinality-entry budget and allocation
  release on censorship;
- `src/analysis/model.rs` and `src/analysis/report.rs`: factual limitation
  state and truthful JSON V1 conversion;
- `src/planning/mod.rs`: measured-scope projection and saturating aggregate
  size arithmetic;
- `src/cli/analysis_report.rs`, `src/cli/compress.rs`, and
  `src/cli/benchmark/mod.rs`: limited-result presentation, streaming RawZstd
  recovery, and benchmark scope enforcement.

Certification ran under WSL/Linux with Rust and Cargo 1.85.0 and
`CARGO_TARGET_DIR="$HOME/.cache/datapack-modernization"`:

| Gate | Result |
|---|---|
| `cargo fmt --check` | PASS |
| `cargo check` | PASS |
| `cargo test` | PASS — 234 passed, 0 failed, 0 ignored |
| `cargo clippy --all-targets --all-features -- -D warnings` | PASS |
| `compatibility_fixtures` | PASS — 4/4 |
| `analyze_cli` | PASS — 9/9, unchanged goldens |
| `round_trip` | PASS — 31/31 |
| `security_hardening` | PASS — 69/69 |
| `planning::tests` | PASS — 32/32 |
| `analysis_json_cli` | PASS — 9/9 |
| `analysis_core_consistency` | PASS — 3/3 |
| `bounded_analysis` | PASS — 6/6 |
| `legacy_analysis_api` | PASS — 4/4 |

The final protected-surface review found no changes to `Cargo.toml`,
`Cargo.lock`, serialized metadata, v1/v2 storage code, frozen compatibility
fixtures, or legacy analyze goldens. The WSL repository remains under
`/mnt/c`; incidental timings therefore include WSL/NTFS effects and support no
performance claim.
