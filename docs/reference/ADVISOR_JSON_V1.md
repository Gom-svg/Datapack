# Advisor JSON V1

`datapack advisor INPUT --json` emits one compact UTF-8 JSON document followed
by a newline. `--pretty` changes whitespace only and requires `--json`.

This document defines `AdvisorReportV1`. It is a stable DTO boundary rather
than a serialization of analysis, planner, compression, or comparison
structs. Fields and string values listed here are contract values for schema
version `1`.

## Invocation and exit status

```text
datapack advisor input.csv --json
datapack advisor input.tsv --sample-mb 32 --json
datapack advisor input.psv --json --pretty
```

`--sample-mb` defaults to 64 and accepts the same 1 through 2,048 MiB range as
`datapack analyze`. It controls only the shared bounded analysis. Advisor does
not run Compare, ingest an earlier Comparison report, run Benchmark, perform
compression, decompression, or validation, or use a second parser. Prior
Comparison-report ingestion is deferred beyond Advisor JSON V1.

- Exit `0`: a complete advisory report was emitted. Partial analysis and a
  typed structured-analysis-unavailable result are valid report outcomes.
- Exit `1`: an operational or semantic failure occurred. Stdout is empty and
  the normal CLI error is written to stderr.
- Exit `2`: Clap rejected the command line, including `--pretty` without
  `--json`.

An ambiguous, malformed, unsupported, or unstructured input can make the
shared structured analyzer return `InvalidCsv`. Advisor converts that specific
outcome into `analysis.status: "unavailable"` and safe RawZstd advice. It does
not convert I/O, allocation, invalid-limit, or internal-format errors into a
success report. V1 has no JSON error envelope.

## Root object

The root object has exactly these fields:

| Field | JSON type | Meaning |
|---|---|---|
| `schema_version` | integer | Always `1`. |
| `report_type` | string | Always `"advisor"`. |
| `policy` | object | Identity of the deterministic advisory mapping. |
| `analysis` | object | Privacy-safe facts on which the recommendations are based. |
| `recommendations` | array | Ordered advisory results with typed stable evidence. |

No field contains a confidence, score, overall winner, projected performance,
or AI/LLM output.

## `policy`

The root policy object has exactly:

| Field | JSON type | V1 value |
|---|---|---|
| `name` | string | `"AdvisorPolicyV1"` |
| `version` | integer | `1` |

The policy is a deterministic conversion of current analysis facts and
`PlannerPolicyV1` results. The policy version is distinct from the report
schema version so a future report can identify both its document shape and
its recommendation rules explicitly.

## `analysis`

The analysis object has exactly these fields:

| Field | JSON type | Meaning |
|---|---|---|
| `status` | string | `"available"` or `"unavailable"`. |
| `scope` | string | `"full"`, `"sampled"`, or `"unavailable"`. |
| `completeness` | string | `"complete"`, `"partial"`, or `"unavailable"`. |
| `source_size_bytes` | integer | Complete source length obtained from filesystem metadata. |
| `bytes_analyzed` | integer or null | Bytes represented by complete records accepted by the selected analyzer, or null when analysis is unavailable. |
| `records_analyzed` | integer or null | Accepted data-record count, excluding the header, or null when analysis is unavailable. |
| `planner` | object or null | Existing planner selection or compatibility/safety fallback, or null when no `DatasetAnalysis` was produced. |
| `evidence` | array of objects | Typed facts and policy reasons relevant to advice, in deterministic order. |
| `high_cardinality_columns` | array of objects | Conservative censored-cardinality observations, ordered by column index. |

`scope` describes analyzed source-byte coverage. `completeness` also accounts
for hard analysis limitations. Full byte coverage can therefore coexist with
`completeness: "partial"`, for example when cardinality tracking reaches its
shared memory budget.

`bytes_analyzed` is not a whole-process I/O count or memory measurement. It
has the same complete-record semantics as the analysis report. A sampled
analysis can still contain a `PlannerPolicyV1` recommendation, but both its
planner selection and Advisor's compression message apply only to the
analyzed scope.

### `planner`

When available, the planner object has exactly:

| Field | JSON type | Meaning |
|---|---|---|
| `policy` | object | `{ "name": "PlannerPolicyV1", "version": 1 }`. |
| `selected_archive_mode` | string | `"csv_columnar_dictionary"` or `"raw_zstd"`. |
| `reason_code` | string | Stable reason for the planner selection or safety/compatibility fallback. |
| `selection_scope` | string | `"planner_recommendation"`, `"safe_fallback"`, or `"format_fallback"`. |

The report does not include the raw planner message, planning time, estimated
savings, or estimated memory. A selected structured mode is advisory and is
not proof that complete-input eligibility or the encoder will emit a
columnar archive. The encoder's existing safety checks and RawZstd fallback
remain authoritative.

`selection_scope` identifies which policy boundary has authority for the
reported mode:

- `planner_recommendation` is an actual `PlannerPolicyV1` recommendation;
- `safe_fallback` is a safety override caused by bounded-analysis limits; and
- `format_fallback` is a compatibility override for a format that is not
  eligible for the structured path.

Only `planner_recommendation` is attributed to `PlannerPolicyV1` in Advisor's
message. A safety or compatibility fallback can select RawZstd after the base
planner ran, so describing either fallback as a planner recommendation would
be false. V1 renders those RawZstd cases as follows:

| Selection scope | Evidence source | Compression recommendation message |
|---|---|---|
| `safe_fallback` | `safety_policy` | `Analysis safety limits require the byte-preserving RawZstd fallback.` |
| `format_fallback` | `compatibility_policy` | `The compatibility adapter requires the byte-preserving RawZstd fallback.` |

Current V1 selection reason codes used as advisory evidence are:

- `HIGH_REPETITION_DETECTED`;
- `INSUFFICIENT_REPETITION_MAJORITY`;
- `PROJECTED_DICTIONARY_SAVINGS_BELOW_THRESHOLD`;
- `ANALYSIS_LIMITED_RAW_ZSTD_FALLBACK`; and
- `STRUCTURED_COMPRESSION_NOT_ENABLED_FOR_DIALECT`.

Consumers must branch on `reason_code`, not planner presentation text.

### Typed `evidence`

Every evidence item has exactly these fields:

| Field | JSON type | Meaning |
|---|---|---|
| `source` | string | Stable policy/fact namespace described below. |
| `code` | string | Stable reason or diagnostic within that namespace. |
| `column_index` | integer or null | Zero-based affected column, or null for dataset-level evidence. |

The `(source, code, column_index)` tuple, rather than `code` alone, is the
machine-readable explanation. V1 defines six source values:

| Source | Meaning |
|---|---|
| `analysis_status` | Availability of the shared structured analysis. |
| `analysis_diagnostic` | A factual sampling, safety-limit, or cardinality diagnostic. |
| `planner_policy` | A recommendation made by `PlannerPolicyV1`. |
| `column_policy` | A `PlannerPolicyV1` reason for one indexed column. |
| `safety_policy` | A RawZstd override required by an analysis safety limit. |
| `compatibility_policy` | A RawZstd override required by the compatibility boundary for a format. |

For an available analysis, `evidence` begins with the selected-mode evidence.
Its source follows `planner.selection_scope`: `planner_policy`,
`safety_policy`, or `compatibility_policy`. Applicable analysis diagnostics
and column evidence follow in deterministic order. Current limit-related codes
under `analysis_diagnostic` include:

- `SAMPLE_BYTE_LIMIT_REACHED`;
- `SAMPLE_RECORD_LIMIT_REACHED`;
- `INCOMPLETE_HEADER_SAMPLE`;
- `HEADER_BYTE_LIMIT_REACHED`;
- `RECORD_BYTE_LIMIT_REACHED`;
- `COLUMN_LIMIT_REACHED`;
- `ANALYSIS_MEMORY_LIMIT_REACHED`; and
- `CARDINALITY_MEMORY_LIMIT_REACHED`.

Column cardinality censorship can also contribute
`CARDINALITY_LIMIT_REACHED` with its column index. When the corresponding
column policy selected its cardinality-threshold branch,
`CARDINALITY_THRESHOLD_EXCEEDED` appears separately with source
`column_policy` and the same index. Consumers must tolerate future evidence
codes and must not infer complete analysis merely because they do not
recognize a tuple.

When analysis is unavailable, `evidence` contains exactly one dataset-level
item: `analysis_status`, `STRUCTURED_ANALYSIS_UNAVAILABLE`, and a null column
index. The raw parser or detector message is not emitted.

### `high_cardinality_columns`

Each item has exactly:

| Field | JSON type | Meaning |
|---|---|---|
| `column_index` | integer | Zero-based column index. |
| `cardinality` | object | A truthful censored lower bound. |

The cardinality object has exactly:

| Field | JSON type | V1 value |
|---|---|---|
| `kind` | string | Always `"at_least"`. |
| `value` | integer | A defensible observed lower bound of at least 8,193. |

Advisor emits a high-cardinality item only when the analysis fact is
`at_least 8193` or greater. It does not promote the value to an exact count.
Shared cardinality-memory pressure can censor a column at a smaller lower
bound; that smaller value is not labeled high-cardinality in Advisor V1.
Column names and values are never emitted.

## `recommendations`

Each recommendation has exactly these fields:

| Field | JSON type | Meaning |
|---|---|---|
| `code` | string | Stable recommendation identifier. |
| `category` | string | Stable category token. |
| `message` | string | Fixed human-readable explanation. |
| `evidence` | array of objects | Nonempty typed evidence that caused the recommendation. |

Consumers must branch on `code` and typed `evidence`, not match `message`.
Every V1 recommendation has at least one evidence item, and each item is also
present in `analysis.evidence`. Messages do not
interpolate paths, column names, values, raw parser errors, or timing results.

Recommendations use a fixed order:

1. exactly one compression-strategy recommendation;
2. `HIGH_CARDINALITY_OBSERVED`, when applicable;
3. `ANALYSIS_LIMITED`, when applicable; and
4. `FULL_COMPARISON_RECOMMENDED`, when analysis is partial or unavailable.

### Stable recommendation codes

| Code | Category | Trigger and evidence |
|---|---|---|
| `STRUCTURED_COMPRESSION_RECOMMENDED` | `compression_strategy` | The existing planner recommendation selected `csv_columnar_dictionary`; evidence uses `planner_policy`. |
| `RAW_ZSTD_RECOMMENDED` | `compression_strategy` | The planner selected `raw_zstd`, a safety/compatibility policy required fallback, or structured analysis was unavailable; the evidence source identifies which case applies. |
| `HIGH_CARDINALITY_OBSERVED` | `dataset_observation` | At least one item appears in `high_cardinality_columns`; evidence identifies cardinality censorship/threshold facts. |
| `ANALYSIS_LIMITED` | `analysis_limitation` | Analysis completeness is partial; evidence contains the applicable typed limit diagnostics. |
| `FULL_COMPARISON_RECOMMENDED` | `next_action` | Analysis is partial or unavailable; evidence contains the typed limit diagnostic or analysis-status fact. |

There is always exactly one compression-strategy recommendation. A
high-cardinality observation can coexist with an overall structured planner
selection. A sampled structured recommendation can coexist with
`ANALYSIS_LIMITED` and `FULL_COMPARISON_RECOMMENDED`.

`FULL_COMPARISON_RECOMMENDED` means that `datapack compare INPUT --mode full`
can measure and round-trip the complete input explicitly. It does not promise
that Full mode will remove record, column, cardinality, or analysis-memory
limits, and it does not predict a measured winner. Advisor V1 neither runs
Compare nor accepts a prior Comparison report. Ingestion of an existing
Comparison report is deferred beyond this contract.

## Available partial example

Values below illustrate the V1 shape and partial-analysis semantics; they are
not performance claims:

```json
{
  "schema_version": 1,
  "report_type": "advisor",
  "policy": {
    "name": "AdvisorPolicyV1",
    "version": 1
  },
  "analysis": {
    "status": "available",
    "scope": "sampled",
    "completeness": "partial",
    "source_size_bytes": 104857600,
    "bytes_analyzed": 67108840,
    "records_analyzed": 10000,
    "planner": {
      "policy": {
        "name": "PlannerPolicyV1",
        "version": 1
      },
      "selected_archive_mode": "csv_columnar_dictionary",
      "reason_code": "HIGH_REPETITION_DETECTED",
      "selection_scope": "planner_recommendation"
    },
    "evidence": [
      {
        "source": "planner_policy",
        "code": "HIGH_REPETITION_DETECTED",
        "column_index": null
      },
      {
        "source": "analysis_diagnostic",
        "code": "SAMPLE_RECORD_LIMIT_REACHED",
        "column_index": null
      }
    ],
    "high_cardinality_columns": []
  },
  "recommendations": [
    {
      "code": "STRUCTURED_COMPRESSION_RECOMMENDED",
      "category": "compression_strategy",
      "message": "PlannerPolicyV1 recommends structured compression for the analyzed scope; compression retains its safe RawZstd fallback.",
      "evidence": [
        {
          "source": "planner_policy",
          "code": "HIGH_REPETITION_DETECTED",
          "column_index": null
        }
      ]
    },
    {
      "code": "ANALYSIS_LIMITED",
      "category": "analysis_limitation",
      "message": "Analysis is partial because a configured sampling or safety limit was reached.",
      "evidence": [
        {
          "source": "analysis_diagnostic",
          "code": "SAMPLE_RECORD_LIMIT_REACHED",
          "column_index": null
        }
      ]
    },
    {
      "code": "FULL_COMPARISON_RECOMMENDED",
      "category": "next_action",
      "message": "Run Compare in Full mode when complete-input measurements and round-trip validation are needed.",
      "evidence": [
        {
          "source": "analysis_diagnostic",
          "code": "SAMPLE_RECORD_LIMIT_REACHED",
          "column_index": null
        }
      ]
    }
  ]
}
```

## Structured-analysis-unavailable example

```json
{
  "schema_version": 1,
  "report_type": "advisor",
  "policy": {
    "name": "AdvisorPolicyV1",
    "version": 1
  },
  "analysis": {
    "status": "unavailable",
    "scope": "unavailable",
    "completeness": "unavailable",
    "source_size_bytes": 4096,
    "bytes_analyzed": null,
    "records_analyzed": null,
    "planner": null,
    "evidence": [
      {
        "source": "analysis_status",
        "code": "STRUCTURED_ANALYSIS_UNAVAILABLE",
        "column_index": null
      }
    ],
    "high_cardinality_columns": []
  },
  "recommendations": [
    {
      "code": "RAW_ZSTD_RECOMMENDED",
      "category": "compression_strategy",
      "message": "Structured analysis is unavailable; use the byte-preserving RawZstd fallback.",
      "evidence": [
        {
          "source": "analysis_status",
          "code": "STRUCTURED_ANALYSIS_UNAVAILABLE",
          "column_index": null
        }
      ]
    },
    {
      "code": "FULL_COMPARISON_RECOMMENDED",
      "category": "next_action",
      "message": "Run Compare in Full mode when complete-input measurements and round-trip validation are needed.",
      "evidence": [
        {
          "source": "analysis_status",
          "code": "STRUCTURED_ANALYSIS_UNAVAILABLE",
          "column_index": null
        }
      ]
    }
  ]
}
```

## Privacy and determinism

Advisor JSON contains no input path or file name, header or column name, raw
field value, sample row, reconstructed content, temporary path, hash, timing,
throughput, score, confidence, or AI-generated field. Column-specific facts
use only zero-based indexes and tagged lower bounds.

Advisor performs no compression, comparison, validation, or temporary-file
operation. The report omits the nondeterministic planning elapsed time.
Repeated Advisor invocations over unchanged bytes and options therefore
produce identical JSON content.

## Compatibility boundary

`AdvisorReportV1` is not stored in `.dpack` archives. It does not alter or
extend the frozen v1/v2 metadata graph or wire representation. Advisor advice
does not override full-input structured eligibility, the planner/encoder
execution contract, byte-exact verification, or RawZstd fallback. The command
also does not change legacy analyze text, Compare methodology, Benchmark
methodology, compression defaults, or public Rust APIs.
