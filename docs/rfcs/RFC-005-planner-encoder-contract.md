# RFC-005 — Planner/Encoder Execution Contract

- Status: Implemented and technically certified
- Date: 2026-08-06
- Baseline: `rfc-007-structured-delimited-compression`
- Related: [RFC-001B](RFC-001B-compatible-analysis-core.md) and
  [RFC-004B](RFC-004B-safe-structured-delimited-compression.md)

## 1. Decision

Phase 8 adds a crate-private `ColumnExecutionPlan` between
`PlannerPolicyV1` and the DCSV01 writer. The plan carries one dense ordered
mode per parsed column plus hard dictionary entry and logical-byte limits.
CLI compression and the existing benchmark encoder route both pass this plan
to a separate planned codec entry point.

`PlannerPolicyV1` remains the compatibility authority for profiles,
recommendations, archive selection, estimates, thresholds, and reason text.
`apply_dictionary_limits` still converts sampled Dictionary recommendations to
Plain when their sampled profile exceeds a CLI limit. Phase 8 makes that
post-limit plan executable; it does not introduce `PlannerPolicyV2` or alter
V1 formulas.

## 2. Compatibility projection

The frozen DCSV01 payload has only two column mode bytes:

| DCSV01 byte | Execution mode |
|---:|---|
| `0` | `Plain` lexical values |
| `1` | `Dictionary` values and compact IDs |

No Raw, Delta, RLE, or BitPacking column tag exists. The execution adapter
therefore uses this explicit compatibility projection:

| PlannerPolicyV1 strategy | DCSV01 execution mode |
|---|---|
| `Dictionary` | `Dictionary` |
| `Plain` | `Plain` |
| `DeltaCandidate` | `Plain` |
| `Raw` | `Plain` |

`DeltaCandidate` remains an observation only. The adapter does not activate a
transform that the frozen payload cannot represent. DCSV01 Plain already
retains each field's raw lexical bytes, so projecting `Raw` to Plain does not
normalize data. Whole-archive RawZstd remains the safe fallback when a
structured contract cannot be honored.

The execution model is crate-private. It does not add a public API, serialized
metadata type, payload variant, or DCSV01 mode.

## 3. Structural contract

Before writing any planned columns, the codec requires:

- execution-plan length equal to the parsed column count;
- dense indexes in exact order `0..column_count`;
- a supported delimiter already confirmed by full-input canonical detection;
- a DCSV01-compatible parsed document; and
- checked row, column, value-length, dictionary-count, and dictionary-code
  conversions.

A missing, duplicated, reordered, or out-of-range plan entry is not guessed or
replanned. The structured candidate is rejected, its partial in-memory buffer
is discarded, and the common archive adapter emits RawZstd instead.

The public legacy `columnar::encode(bytes)` and public storage helpers retain
their historical exact-cost chooser. This preserves frozen fixture
reproduction and established non-CLI callers. Only the crate-private CLI and
benchmark routes consume the execution plan.

## 4. Dictionary enforcement

The CLI defaults remain 65,535 distinct values and 64 MiB of logical
dictionary entries per column. The byte budget counts each distinct raw value
plus its four-byte DCSV01 length prefix. It is not an RSS estimate or a claim
about allocator overhead.

For a planned Dictionary column, the writer:

1. reserves the bounded entry table fallibly;
2. walks values in source order and assigns deterministic first-occurrence
   codes;
3. checks the next distinct count and logical bytes before retaining the new
   entry;
4. rejects before exceeding either configured limit; and
5. writes Dictionary mode only after the complete bounded dictionary is
   available.

DCSV01 stores the header as the first value in each encoded column, so the
full-input limit includes that header entry. This intentionally differs from
PlannerPolicyV1's data-row cardinality fact. Unsampled tail values can likewise
make the full input exceed a sampled recommendation.

When either condition prevents the planned Dictionary mode, the codec does
not silently switch that column to Plain. The structured candidate is
unexecutable under its contract and the archive adapter uses RawZstd. This
keeps the invariant that every emitted structured column mode equals its
execution plan and ensures the configured limits cannot be bypassed.

A planned Plain column never constructs a dictionary map. Zero dictionary
limits therefore turn sampled Dictionary recommendations into Plain before
encoding, while a later full-input breach follows the global RawZstd fallback.

## 5. Global selection behavior

Default/fast compression executes the effective archive and column plans once.
`--mode best` retains its established near-threshold comparison behavior.
`--verify-best` retains its explicit RawZstd-versus-structured comparison. In
both comparison paths, the structured candidate uses the same execution plan
and hard limits as the default path.

Benchmark keeps its existing CLI and methodology. When its legacy comma plan
selects structured execution, it now uses the same plan contract with the CLI
default dictionary limits. A structured refusal is reported through the
existing selected-mode/candidate-error path and executes RawZstd; run counts,
medians, timing boundaries, hashes, and measured scope are unchanged.

V2 bypasses v1 analysis and planning and remains chunked RawZstd.

## 6. Verification

Focused tests establish:

- Dictionary and Plain plan modes override the legacy exact-cost chooser in
  both directions;
- Raw and DeltaCandidate project to DCSV01 Plain;
- exact dictionary entry and byte boundaries succeed, while one-over limits
  are rejected before retention;
- the header counts toward actual dictionary limits;
- a full-input value unseen in the sampled plan produces RawZstd and exact
  restoration;
- malformed plan shape produces RawZstd rather than guessing or corrupting a
  payload;
- default, best, and verify-best cannot re-enable Dictionary after CLI limit
  demotion;
- the four Phase 7 delimiters retain byte-exact structured execution;
- public legacy DCSV01 fixture reproduction remains byte-exact; and
- all PlannerPolicyV1 characterization results remain unchanged.

No performance claim is made. This phase verifies control-flow and resource
invariants, not throughput or compression-ratio improvement.
