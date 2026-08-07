# RFC-005B — DataPack Compare

- Status: Implemented
- Date: 2026-08-06
- Baseline: `rfc-009-archive-validation`
- Related: [Comparison JSON V1](../reference/COMPARISON_JSON_V1.md) and
  [Comparison Methodology](../reference/COMPARISON_METHODOLOGY.md)

## 1. Decision

Phase 10 adds a factual two-way comparison command:

```text
datapack compare INPUT
datapack compare INPUT --mode quick
datapack compare INPUT --mode full
datapack compare INPUT --runs 5
datapack compare INPUT --mode quick --max-input-mb 32
datapack compare INPUT --json
datapack compare INPUT --json --pretty
```

The initial competitors are DataPack's current default v1 planner/encoder path
and a standalone zstd frame at level 3. Compare does not add a codec, planner
policy, archive mode, or serialized metadata variant. It measures the existing
operations over the same exact input bytes.

`--mode` defaults to `quick`. `--runs` defaults to `3` and accepts values from
1 through 25, inclusive. `--max-input-mb` is a positive MiB value available
only in Quick mode. Quick uses 64 MiB (67,108,864 bytes) when that option is
omitted. `--pretty` requires `--json`.

The successful text and JSON reports contain three independent factual winner
categories: storage ratio, measured compression time, and measured
decompression time. Compare does not calculate an overall winner, score,
confidence value, recommendation, or performance projection.

## 2. Comparison modes

### 2.1 Quick

Quick compares at most the configured prefix of the input. Snapshot
preparation is bounded and occurs before timing. Both competitors receive one
immutable file containing the same exact prefix bytes.

A Quick report is always labeled `partial`, even when the source is small
enough to fit within the configured limit. This conservative label prevents a
bounded exploratory mode from being confused with the Full protocol. The
report separately states whether the source was actually truncated:

- `scope.prefix_limited` is true only when the compared prefix is shorter than
  the source;
- `QUICK_MODE_PARTIAL` is always present in `limitations`; and
- `INPUT_PREFIX_LIMITED` is also present when the byte limit truncated the
  source.

Both restored Quick artifacts must match the exact measured prefix by length
and SHA-256. Their validation status is nevertheless
`partially_validated`, because the comparison mode does not claim a complete
source comparison.

### 2.2 Full

Full compares the complete input and does not accept `--max-input-mb`. Both
restored artifacts must match the complete source snapshot by length and
SHA-256. A successful Full report is labeled `full`, and each competitor is
`validated`. Full reports can still contain structured-analysis or encoder
fallback limitations; those describe how DataPack encoded the complete bytes,
not a partial input scope.

Full mode uses the existing bounded analysis, planner, encoder, storage, and
RawZstd fallback behavior. It does not remove their safety limits or imply a
fixed memory footprint.

## 3. Competitors and planning

The DataPack competitor uses the same v1 `PlannerPolicyV1` compatibility
policy, execution-plan enforcement, dictionary limits, structured-safety
gate, and RawZstd fallback used by the default compression path. Its artifact
size includes the complete `.dpack` v1 container. The report records the
archive mode actually emitted after the structured-safety gate.

The standalone competitor creates an ordinary zstd frame at level 3. It does
not receive DataPack metadata or container framing. The configured level is a
methodology fact, not a claim that level 3 is optimal for an input.

DataPack planning is performed once against the immutable comparison snapshot
before the timed compression runs. The shared bounded analyzer may inspect a
sample rather than every snapshot byte; it retains its 64 MiB planning sample
budget and its narrower record, column, cardinality, and memory limits. Its
elapsed time is reported separately and is excluded from the compression
winner. Snapshot creation in both Quick and Full modes is also excluded. A
structured candidate that cannot safely honor its execution plan continues to
use the existing RawZstd fallback rather than producing a partially executable
archive.

The compare-specific structured path buffers at most 64 MiB of snapshot bytes.
If a structured plan would require buffering a larger snapshot, Compare uses
RawZstd and reports `STRUCTURED_COMPARE_MEMORY_LIMIT`. RawZstd remains the
streaming safe fallback. Other bounded-analysis and encoder fallbacks are
reported through their stable limitation codes. The 64 MiB value bounds this
input buffer; it is not a total-RSS or allocator-overhead claim.

Compare does not silently force `PlannerPolicyV1` through a new parser or
change its compatibility decisions. It does not add structured v2 behavior;
the DataPack comparison artifact is v1.

## 4. Timing protocol

Compression and decompression use symmetric file-to-file boundaries for both
competitors. Each timed compression starts with the same prepared input file
and ends after the complete artifact has been written and flushed. Each timed
decompression starts with its complete artifact and ends after the complete
restored output has been written and flushed. The boundary does not include an
operating-system `fsync` guarantee.

The two competitors are interleaved within each run, and their first/second
order alternates between runs. This reduces a fixed ordering bias; it does not
remove scheduler, cache, thermal, storage, virtualization, or background-load
effects. No unreported warm-up run is used.

After each compression pair and outside both timed regions, Compare hashes the
complete DataPack and standalone-zstd artifacts. Each contender's first digest
establishes its invocation-local artifact identity; every later compression
run must reproduce that identity. Artifact hashing is command overhead, not
part of either compression sample. JSON V1 discloses this invariant as
`methodology.artifact_stability: "sha256_per_run"` without exposing a digest.

Elapsed samples are sorted and aggregated using a conventional median. The
default of three runs selects the middle sample. For an even run count, the
median is the arithmetic mean of the two central durations, computed without
unchecked duration arithmetic. Every sample and the median are reported.
Throughput is derived from the exact compared byte count and the corresponding
aggregate duration and is labeled in MiB/s (1 MiB = 1,048,576 bytes).

Archive sizes and timing results describe only this invocation, input scope,
configuration, machine, and filesystem. They are not generalized performance
claims. In particular, runs whose source or temporary artifacts are under
`/mnt/c` in WSL include the effects of the WSL/NTFS path and must be identified
as such when results are shared.

The complete timing definition is maintained in
`docs/reference/COMPARISON_METHODOLOGY.md`.

## 5. Exact validation

Comparison never treats successful decompression alone as identity proof. For
each competitor it:

1. records the exact comparison-input length and SHA-256;
2. hashes the complete artifact after every compression run and requires that
   contender's artifact identity to remain stable;
3. restores the generated artifact during every timed decompression run;
4. outside each timed region, records that run's complete restored length and
   SHA-256; and
5. requires every restored identity to match the comparison-input identity
   before producing a successful report.

Every decompression sample is validated directly. Because every compression
run produced the same complete artifact bytes, these restoration checks also
validate every reported compression sample transitively. Compare does not
assume that equal artifact sizes imply equal artifact contents. JSON V1 names
this validation method
`methodology.validation: "sha256_roundtrip_per_run"`.

Quick hashes the exact bounded prefix. Full hashes the complete snapshot.
Snapshot creation and all snapshot, per-run artifact, and restored-output hash
calculation are outside the compression and decompression timings. No digest
value is emitted.

A changed per-run artifact identity, restored-length mismatch, or SHA-256
mismatch is a corruption-risk failure. Compare emits no partial winner report
and exits unsuccessfully rather than ranking an unstable or invalid artifact.

## 6. Reports and winners

The default renderer is human-readable. `--json` emits the versioned,
privacy-safe `ComparisonReportV1` DTO documented in
`docs/reference/COMPARISON_JSON_V1.md`; `--pretty` changes whitespace only.
Neither renderer serializes internal planner, codec, storage, or benchmark
structs directly.

Winner categories are determined independently:

| Category | Factual basis |
|---|---|
| Storage | For nonempty input, smaller complete artifact, equivalently the larger ratio; for empty input, a tie because both ratios are zero. |
| Compression | Shorter aggregate file-to-file compression duration. |
| Decompression | Shorter aggregate file-to-file decompression duration. |

An exact equality is reported as a tie. Empty input also produces a storage
tie regardless of differing container sizes because both storage ratios are
zero. The winner names mean only that one reported measurement is smaller
under this invocation's methodology. Compare does not apply an undocumented
significance threshold, combine the categories, or turn their results into
advice. Deterministic recommendations remain the separate Advisor phase.

## 7. Temporary files, privacy, and failure behavior

Comparison artifacts, bounded Quick input, and restored outputs use
collision-resistant temporary paths created without overwriting an existing
file. Compare has no `--keep-temp` option. Cleanup is completed before a
success report is emitted, so an explicit cleanup failure makes the command
unsuccessful. After an operation failure, the guard attempts best-effort
cleanup; artifacts can remain if the filesystem also rejects removal.

The JSON report contains no input path, file name, temporary path, raw field
value, sample row, reconstructed bytes, or source, artifact, or restored-output
digest. Any reported limitation is a stable bounded code, not a raw
parser/storage error or user content.

Exit behavior is:

- exit `0` only after both contenders retain stable per-run artifact identity,
  exact restoration checks pass, and required cleanup succeeds;
- exit `1` for an unreadable input, invalid semantic option, planning,
  compression, decompression, identity, or cleanup failure; and
- exit `2` when Clap rejects the command line, including `--pretty` without
  `--json`.

Operational failures use stderr and leave stdout empty. V1 does not define a
JSON error envelope and never emits a success-shaped report with missing
competitor metrics.

## 8. Compatibility

Phase 10 does not change:

- the v1 serialized metadata graph, header, or payload bytes;
- the DCSV01 payload representation;
- the v2 fixed header, chunk table, or payload layout;
- existing compression command defaults;
- legacy decompression behavior;
- benchmark command methodology or output;
- frozen compatibility fixtures;
- legacy analyze text or goldens;
- `PlannerPolicyV1` decisions;
- public Rust APIs; or
- the Cargo dependency graph.

Compare invokes the existing frozen writers and readers. It neither stores
comparison metadata in an archive nor introduces a new wire-format guarantee.

## 9. Verification scope

Focused tests cover CLI defaults and option conflicts; Quick prefixes below,
at, and above the byte bound; the always-partial Quick label; complete Full
scope; run-count bounds and odd/even median aggregation; symmetric zstd and
DataPack compression/decompression; stable per-run artifact identity for both
contenders; exact restored length/SHA-256 identity after every timed
decompression; factual winner and tie selection; compact/pretty JSON
equivalence; schema tokens and privacy; required cleanup before success,
best-effort cleanup after failure, and non-clobbering creation.

Timing tests verify arithmetic and control-flow invariants using supplied
samples. They do not assert that either codec must be faster or smaller on a
particular machine. Existing compatibility, analyze-golden, round-trip,
security, planner, and validation suites remain authoritative for their
respective frozen behavior.

## 10. Limitations

- Quick is an explicitly partial comparison and cannot predict the result for
  unexamined source bytes.
- Full measurements remain affected by the local operating system,
  filesystem, caches, scheduler, and concurrent work.
- The initial comparison contains only DataPack v1 and standalone zstd level
  3; it is not a broad codec survey.
- The reported planning time is visible but excluded from the compression
  winner.
- A winner is a measured category result, not an archival recommendation or a
  statistically significant general conclusion.
