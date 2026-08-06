# DataPack Analysis JSON V1

`datapack analyze INPUT --json` emits one compact JSON document followed by a
newline. Add `--pretty` for indented output:

```console
datapack analyze input.csv --json
datapack analyze input.csv --json --pretty
```

`--pretty` requires `--json`. Successful JSON mode writes no legacy table text.
Analysis failures retain the normal DataPack error text and exit code; V1 does
not define a JSON error envelope.

The legacy `--plan` flag controls detail only in text mode. JSON V1 always
contains its planner section, so `--json --plan` is accepted and produces the
same JSON value as `--json`.

## Contract

Every report begins with:

```json
{
  "schema_version": 1,
  "report_type": "analysis"
}
```

Consumers must use `schema_version`, not the DataPack package version or the
`.dpack` archive version, to select this contract.

The complete V1 shape is:

```json
{
  "schema_version": 1,
  "report_type": "analysis",
  "dataset": {
    "source_size_bytes": 1234,
    "column_count": 2,
    "parser": {
      "format": "csv",
      "delimiter": ",",
      "record_model": "physical_line",
      "header_mode": "first_record"
    },
    "columns": [
      {
        "index": 0,
        "name_status": {
          "is_empty": false,
          "duplicate_of": null
        },
        "observed_values": 100,
        "empty_values": 0,
        "numeric_values": 100,
        "value_length_bytes": {
          "minimum": 1,
          "maximum": 3,
          "mean": 1.92,
          "total": 192
        },
        "cardinality": {
          "kind": "exact",
          "value": 100
        },
        "repetition_rate": {
          "kind": "exact",
          "value": 0.0
        },
        "planner": {
          "selected_strategy": "delta_candidate",
          "reason": {
            "code": "NUMERIC_OR_DATE_HEURISTIC_MATCHED",
            "message": "Numeric/date heuristic matched"
          },
          "estimated_dictionary_size_kib": 2,
          "estimated_encoded_size_bytes": 300,
          "estimated_raw_size_bytes": 292
        }
      }
    ]
  },
  "sampling": {
    "scope": "full",
    "completeness": "complete",
    "limited": false,
    "limit_reached": null,
    "source_size_bytes": 1234,
    "bytes_read": 1234,
    "bytes_analyzed": 1234,
    "records_analyzed": 100,
    "configured_max_bytes": 67108864,
    "configured_max_records": 10000,
    "final_newline": true
  },
  "planner": {
    "policy": {
      "name": "PlannerPolicyV1",
      "version": 1
    },
    "selection_scope": "planner_recommendation",
    "candidate_archive_modes": [
      "csv_columnar_dictionary",
      "raw_zstd"
    ],
    "candidate_column_strategies": [
      "dictionary",
      "plain",
      "delta_candidate",
      "raw"
    ],
    "selected_archive_mode": "raw_zstd",
    "reason": {
      "code": "INSUFFICIENT_REPETITION_MAJORITY",
      "message": "Insufficient repetition across majority of columns for dictionary gains."
    },
    "estimated_savings_percent": 0.0,
    "estimated_dictionary_memory_mib": 0.0
  },
  "diagnostics": []
}
```

Values in this example are illustrative. Field names, enum strings, reason
codes, and diagnostic codes are contract values.

## Sampling semantics

The compatible V1 analyzer uses the first physical record as a header and
counts accepted nonblank data records in `records_analyzed`.

| Condition | `scope` | `completeness` | `limited` | `limit_reached` |
|---|---|---|---:|---|
| Every source byte analyzed | `full` | `complete` | `false` | `null` |
| Byte budget stopped analysis | `sampled` | `partial` | `true` | `byte_limit` |
| Record budget stopped analysis | `sampled` | `partial` | `true` | `record_limit` |

`bytes_read` can exceed `configured_max_bytes` in V1 because the compatible
reader may read a complete physical line before deciding not to analyze it.
`bytes_analyzed` excludes such a discarded overshoot line. These are truthful
coverage fields, not hard-memory guarantees.

`final_newline` is `true` or `false` only for complete coverage. It is `null`
for a partial sample.

## Estimates and unknown values

Cardinality is tagged:

- `{"kind":"exact","value":N}` means the compatible tracker retained every
  observed distinct hash.
- `{"kind":"at_least","value":8193}` means its 8,192-entry tracking limit was
  exceeded. It is not an exact count.
- `{"kind":"unknown","value":null}` is reserved for an analyzer that cannot
  establish a defensible value or lower bound.

Repetition is exact only when the sample has observed values and cardinality is
exact. Otherwise it is `{"kind":"unknown","value":null}`. The internal legacy
planner sentinel is not emitted as a factual value.

Minimum, maximum, and mean byte length are `null` when no values were observed.
`numeric_values` counts physical fields accepted by the legacy integer or float
parser; it is not a type declaration.

All planner sizes, savings, and memory numbers are estimates. MiB and KiB names
use 1024-based units. `selected_archive_mode` and `selected_strategy` identify
the result selected by `PlannerPolicyV1`; they do not prove which encoding an
archive writer ultimately executed.

## Stable codes

Archive reason codes:

- `INSUFFICIENT_REPETITION_MAJORITY`
- `PROJECTED_DICTIONARY_SAVINGS_BELOW_THRESHOLD`
- `HIGH_REPETITION_DETECTED`

Column reason codes:

- `CARDINALITY_THRESHOLD_EXCEEDED`
- `VERY_LOW_CARDINALITY_HIGH_REPETITION`
- `REPETITION_SUPPORTS_DICTIONARY`
- `NUMERIC_OR_DATE_HEURISTIC_MATCHED`
- `NO_STRONG_DICTIONARY_SIGNAL`

Diagnostic codes:

- `SAMPLE_BYTE_LIMIT_REACHED`
- `SAMPLE_RECORD_LIMIT_REACHED`
- `CARDINALITY_LIMIT_REACHED`
- `EMPTY_COLUMN_NAME`
- `DUPLICATE_COLUMN_NAME`

Every diagnostic contains `code`, `severity`, `message`, and a nullable
`column_index`. Consumers should branch on `code`; messages are for people.

## Privacy

JSON V1 never contains:

- the input path or filename;
- header or column names;
- raw field values;
- sampled rows;
- timestamps or host identifiers.

Column indexes and aggregate facts allow a caller that already knows the input
schema to correlate results without DataPack reproducing source identifiers or
content.

## Determinism and legacy output

The report omits planner wall-clock time and orders columns and diagnostics
deterministically. Compact and pretty modes contain the same JSON value.

Running `datapack analyze INPUT` without `--json` continues to use the frozen
legacy text renderer. JSON V1 does not change `.dpack` v1/v2 bytes, compression
defaults, or benchmark methodology.
