# Comparison JSON V1

`datapack compare INPUT --json` emits one compact UTF-8 JSON document followed
by a newline. `--pretty` changes whitespace only and requires `--json`.

This document defines `ComparisonReportV1`. It is a stable DTO boundary rather
than a serialization of planner, benchmark, codec, or archive metadata
structs. Fields and string values listed here are contract values for schema
version `1`.

## Invocation and exit status

```text
datapack compare input.csv --json
datapack compare input.csv --mode quick --max-input-mb 32 --runs 3 --json
datapack compare input.csv --mode full --runs 5 --json --pretty
```

`--mode` defaults to `quick`; `--runs` defaults to `3` and must be in
`1..=25`. Quick compares at most 64 MiB when `--max-input-mb` is omitted. An
explicit maximum must be greater than zero and is rejected in Full mode.

- Exit `0`: both contenders reproduced stable artifact bytes across every run,
  restored bytes matched the exact comparison input by length and SHA-256,
  temporary artifacts were cleaned, and one report was emitted.
- Exit `1`: an operational or semantic failure occurred. Stdout is empty and
  the normal CLI error is written to stderr.
- Exit `2`: Clap rejected the command line, including `--pretty` without
  `--json`.

V1 has no JSON error envelope. It never emits a partial report containing only
one contender or winner categories derived from an invalid artifact.

## Root object

| Field | JSON type | Meaning |
|---|---|---|
| `schema_version` | integer | Always `1`. |
| `report_type` | string | Always `"comparison"`. |
| `mode` | string | `"quick"` or `"full"`. |
| `scope` | object | Exact source and compared-byte scope. |
| `methodology` | object | Run, aggregation, timing-boundary, planning, zstd-level, artifact-stability, and validation facts. |
| `datapack` | object | DataPack v1 artifact, timing, and validation facts. |
| `standalone_zstd` | object | Standalone zstd artifact, timing, and validation facts. |
| `winners` | object | Three independent factual category results. |
| `limitations` | array of objects | Stable limitation codes and explanatory presentation text. |

The report contains no overall result. A successful partial Quick report is a
complete report for that mode, not a report with a missing contender.

## `scope`

| Field | JSON type | Meaning |
|---|---|---|
| `kind` | string | `"partial"` for every Quick report; `"full"` for a successful Full report. |
| `source_size_bytes` | integer | Complete source size. |
| `compared_size_bytes` | integer | Exact byte count supplied to both contenders. |
| `prefix_limited` | boolean | True only when the Quick bound made the compared prefix shorter than the complete source. |

`source_size_bytes` and `compared_size_bytes` can be equal in Quick mode.
`kind` still remains `"partial"`; `prefix_limited` separately discloses
whether the byte bound actually truncated the source.

Quick uses 67,108,864 bytes for the implicit 64 MiB default. With
`--max-input-mb N`, its bound is the checked product of `N` and 1,048,576. The
configured bound is invocation metadata rather than a V1 report field; the
report records the two factual byte counts and whether limiting occurred.

## `methodology`

| Field | JSON type | Meaning |
|---|---|---|
| `runs` | integer | Number of measured compression and decompression samples per contender. |
| `aggregation` | string | Always `"median"` in V1. |
| `timing_boundary` | string | Always `"file_to_file"` in V1. |
| `planning_included` | boolean | Always false in V1. |
| `planning_time_ms` | number | DataPack planning elapsed time, reported outside compression timing. |
| `zstd_level` | integer | Always `3` in V1. |
| `artifact_stability` | string | Always `"sha256_per_run"` in V1. |
| `validation` | string | Always `"sha256_roundtrip_per_run"` in V1; successful reporting also requires stable per-run artifact identities. |

DataPack planning runs once against the immutable compared-byte snapshot. The
shared analyzer is bounded to a 64 MiB planning sample and its narrower safety
limits, so planning need not inspect every snapshot byte. Its elapsed time is
reported but cannot affect the compression winner directly. Snapshot creation
in both modes, snapshot hashing, per-run artifact stability hashing,
restored-output hashing, and report rendering are also outside the timed
compression/decompression regions.

`artifact_stability: "sha256_per_run"` means that the complete artifact from
every compression run is SHA-256 hashed and compared with that contender's
first-run artifact identity. Digests remain private. This check is symmetric:
it applies independently to DataPack and standalone zstd.

For each contender and operation, duration samples are sorted from shortest to
longest. An odd count uses the middle value. An even count uses the arithmetic
mean of the two central durations, calculated without unchecked duration
arithmetic. This conventional median is the value in `median_ms`.

`file_to_file` means that both contenders start each measured operation with a
complete input file and finish after the complete output file is written and
flushed. It does not imply `fsync`, cache eviction, codec-only CPU timing, or
physical-media persistence. See
`docs/reference/COMPARISON_METHODOLOGY.md` for the complete protocol.

## Contender objects

`datapack` and `standalone_zstd` share this shape:

| Field | JSON type | Meaning |
|---|---|---|
| `artifact_format` | string | `"dpack_v1"` for DataPack or `"zstd"` for standalone zstd. |
| `selected_mode` | string or null | Actual stable DataPack archive mode; null for standalone zstd. |
| `artifact_size_bytes` | integer | Complete generated artifact size. DataPack includes its v1 container. |
| `compression_ratio` | number | `scope.compared_size_bytes / artifact_size_bytes`; larger means fewer artifact bytes for this input. |
| `compression` | object | Compression samples, median, and derived throughput. |
| `decompression` | object | Decompression samples, median, and derived throughput. |
| `validation` | object | Exact restored-byte validation facts. |

DataPack `selected_mode` values are:

- `raw_zstd`; and
- `csv_columnar_dictionary`.

The frozen structured payload continues to use its historical serialized
variant name for comma, tab, pipe, and semicolon inputs. No delimiter-specific
serialized enum is implied. The field records the mode actually emitted after
the existing structured-safety gate and RawZstd fallback, not merely a sampled
recommendation.

Standalone zstd uses level 3 as declared in `methodology.zstd_level`. Both
contender objects describe the same `scope.compared_size_bytes`.

The ratio name is not a percentage. For the same nonempty compared input,
selecting the larger ratio is equivalent to selecting the smaller complete
artifact. When `scope.compared_size_bytes` is zero, both compression ratios are
`0.0`; V1 reports the storage winner as `"tie"` regardless of differing empty
container sizes. An implementation must represent empty-input edge cases
without emitting JSON NaN or infinity.

### Timing objects

`compression` and `decompression` each contain:

| Field | JSON type | Meaning |
|---|---|---|
| `samples_ms` | array of numbers | Every measured duration in run order, in milliseconds. Its length equals `methodology.runs`. |
| `median_ms` | number | Conventional median of `samples_ms`. |
| `throughput_mib_per_second` | number or null | Compared MiB divided by median seconds; null when a finite rate is not defined truthfully. |

Sample values are observations rather than promises. Consumers must not infer
an ordering, warm-up, or significance claim from their magnitude. Winner
selection uses the underlying measured duration values rather than rounded
human-readable text.

### Validation objects

`validation` contains:

| Field | JSON type | Meaning |
|---|---|---|
| `status` | string | `"partially_validated"` in Quick mode or `"validated"` in Full mode. |
| `restored_size_bytes` | integer | Complete restored byte count for this contender. |
| `sha256_match` | boolean | Always true in a successful V1 report. |

`restored_size_bytes` must equal `scope.compared_size_bytes`.
`methodology.validation: "sha256_roundtrip_per_run"` means that every timed
decompression output is length-checked and SHA-256 checked against the
immutable snapshot immediately after its timed region. Hash values are never
emitted. A false SHA-256 or length comparison aborts the command before report
serialization, so V1 does not use `sha256_match: false` as a normal comparison
result. A successful contender-level `sha256_match: true` summarizes all of
that contender's decompression runs, not only the last overwritten output.

After every compression pair, Compare hashes both complete artifacts outside
the timed regions. Within each contender, every run must reproduce the first
run's artifact digest. This invariant is not represented by a digest or a
separate boolean field: a successful report itself guarantees stable artifact
identity. Since every compression sample produced the same artifact bytes and
every decompression sample is directly validated against the snapshot, the
successful report also validates every compression sample transitively. Equal
artifact sizes alone are not treated as identity.

Quick validation covers the exact compared prefix but remains
`partially_validated` with respect to the complete-source protocol. Full
validation covers the complete Full snapshot.

## `winners`

| Field | JSON value | Basis |
|---|---|---|
| `best_storage_ratio` | `"datapack"`, `"standalone_zstd"`, or `"tie"` | For nonempty input, smaller `artifact_size_bytes`, equivalently larger ratio; for empty input, always `"tie"`. |
| `fastest_compression` | `"datapack"`, `"standalone_zstd"`, or `"tie"` | Shorter compression median. |
| `fastest_decompression` | `"datapack"`, `"standalone_zstd"`, or `"tie"` | Shorter decompression median. |

Exact equality produces `"tie"`. The empty-input storage result is also a tie
because neither zero ratio is better, even if the container byte counts differ.
These are independent factual results for one invocation. The object has no
overall winner, score, confidence, weighting, recommendation, or inferred
statistical significance.

## `limitations`

Each limitation has exactly these fields:

| Field | JSON type | Meaning |
|---|---|---|
| `code` | string | Stable machine-readable limitation category. |
| `message` | string | Human-readable context; not a stable branching key. |

Stable V1 codes are:

| Code | Trigger | Current message |
|---|---|---|
| `QUICK_MODE_PARTIAL` | Present in every Quick report. | Quick mode is a bounded comparison and is not a full-input certification. |
| `INPUT_PREFIX_LIMITED` | The Quick byte bound made `scope.compared_size_bytes` smaller than `scope.source_size_bytes`. | Only the configured input prefix was compared. |
| `STRUCTURED_ANALYSIS_UNAVAILABLE` | The shared structured analyzer could not establish a supported structured input, so comparison selected RawZstd. | Structured analysis was unavailable; DataPack used RawZstd. |
| `STRUCTURED_ANALYSIS_LIMITED` | Structured analysis reached a safety bound that requires raw fallback. | Structured analysis reached a safety limit; DataPack used RawZstd. |
| `STRUCTURED_COMPARE_MEMORY_LIMIT` | A structured plan applied, but the snapshot exceeded the fixed 64 MiB compare-specific structured buffering ceiling. | The structured comparison buffering ceiling was reached; DataPack used RawZstd. |
| `STRUCTURED_ENCODER_FALLBACK` | The bounded structured encoder could not safely honor the execution plan and emitted RawZstd instead. | The structured encoder could not safely honor the plan; DataPack used RawZstd. |

A small source that fits under the limit has `scope.prefix_limited: false` and
at least `QUICK_MODE_PARTIAL`; it can also contain a structured fallback code.
A Full report omits the two Quick-only codes but can contain any applicable
structured fallback code. Such a limitation does not make Full byte scope
partial: it truthfully records why DataPack used RawZstd while still comparing
and validating the complete snapshot. Consumers must tolerate future
limitation codes but must not infer that an unknown code means full scope.

## Complete Quick example

All sizes and timings below are illustrative contract values, not measured
DataPack performance claims.

```json
{
  "schema_version": 1,
  "report_type": "comparison",
  "mode": "quick",
  "scope": {
    "kind": "partial",
    "source_size_bytes": 104857600,
    "compared_size_bytes": 67108864,
    "prefix_limited": true
  },
  "methodology": {
    "runs": 3,
    "aggregation": "median",
    "timing_boundary": "file_to_file",
    "planning_included": false,
    "planning_time_ms": 12.0,
    "zstd_level": 3,
    "artifact_stability": "sha256_per_run",
    "validation": "sha256_roundtrip_per_run"
  },
  "datapack": {
    "artifact_format": "dpack_v1",
    "selected_mode": "csv_columnar_dictionary",
    "artifact_size_bytes": 20000000,
    "compression_ratio": 3.3554432,
    "compression": {
      "samples_ms": [102.0, 100.0, 101.0],
      "median_ms": 101.0,
      "throughput_mib_per_second": 633.6633663366337
    },
    "decompression": {
      "samples_ms": [80.0, 82.0, 81.0],
      "median_ms": 81.0,
      "throughput_mib_per_second": 790.1234567901234
    },
    "validation": {
      "status": "partially_validated",
      "restored_size_bytes": 67108864,
      "sha256_match": true
    }
  },
  "standalone_zstd": {
    "artifact_format": "zstd",
    "selected_mode": null,
    "artifact_size_bytes": 24000000,
    "compression_ratio": 2.7962026666666665,
    "compression": {
      "samples_ms": [92.0, 90.0, 91.0],
      "median_ms": 91.0,
      "throughput_mib_per_second": 703.2967032967034
    },
    "decompression": {
      "samples_ms": [70.0, 72.0, 71.0],
      "median_ms": 71.0,
      "throughput_mib_per_second": 901.4084507042253
    },
    "validation": {
      "status": "partially_validated",
      "restored_size_bytes": 67108864,
      "sha256_match": true
    }
  },
  "winners": {
    "best_storage_ratio": "datapack",
    "fastest_compression": "standalone_zstd",
    "fastest_decompression": "standalone_zstd"
  },
  "limitations": [
    {
      "code": "QUICK_MODE_PARTIAL",
      "message": "Quick mode is a bounded comparison and is not a full-input certification."
    },
    {
      "code": "INPUT_PREFIX_LIMITED",
      "message": "Only the configured input prefix was compared."
    }
  ]
}
```

## Privacy and deterministic structure

The report contains no source path, file name, extension, temporary path, raw
field value, sample row, restored bytes, source digest, artifact digest, or
restored digest. The `artifact_stability` token names a method, not a digest.
Limitation codes are stable; messages do not contain raw user data or unstable
internal error strings.

Timings are observations and therefore are not deterministic across runs. The
schema shape, enum strings, size definitions, ratio formula, median arithmetic,
validation requirements, and winner arithmetic are deterministic for a given
set of measured facts.

## Compatibility and evolution

Consumers must read `schema_version` and `report_type` before interpreting the
remaining object. They must tolerate future limitation codes. Removing a
field, changing a field type, changing the ratio or timing basis, changing an
existing status meaning, or changing median arithmetic requires a new schema
version.

Comparison JSON versioning is independent of `.dpack` wire-format versioning.
The report is never embedded in an archive and does not change v1 or v2 bytes.
