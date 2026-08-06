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

Phase 4 extends V1 with bounded-analysis outcomes. Reports for inputs that stay
within every hard limit retain their Phase 3 shape and value types. Limit and
diagnostic code domains are intentionally extensible: consumers must tolerate
unknown future codes, and `dataset.column_count` is an unsigned integer or
`null` when a complete header was not available. This V1 extension is required
to report a safe partial result instead of inventing a column count.

Phase 6 extends the documented value domains in `dataset.parser` for
semicolon-, tab-, and pipe-delimited analysis. It does not add fields to the
report object or change the existing comma parser values.

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

`dataset.parser` names the parser used to produce the facts. For comma input it
continues to identify the legacy physical-line compatibility parser. For a
successfully detected alternate delimiter it identifies the canonical
logical-record parser. On a partial sample, the parser object does not claim
that unexamined bytes satisfy the selected dialect. `dataset.columns` contains
only columns for which factual accumulators could safely be built. It can be
empty while `column_count` is known (for example, at the column limit), and
both are empty/`null` when the header itself was incomplete.

## Parser and delimiter values

The stable Phase 6 parser values are:

| `format` | `delimiter` | `record_model` | `header_mode` |
|---|---|---|---|
| `csv` | `,` | `physical_line` | `first_record` |
| `semicolon_delimited` | `;` | `logical_record` | `first_record` |
| `tsv` | tab character | `logical_record` | `first_record` |
| `psv` | `|` | `logical_record` | `first_record` |

The tab delimiter is a one-character string whose JSON representation uses
the `\t` escape. Format and record-model tokens are lowercase and
underscore-separated exactly as shown.

CLI detection evaluates a bounded prefix of at most 32 physical records and
1 MiB in the canonical candidate order comma, semicolon, tab, then pipe. The
detector re-evaluates at complete physical boundaries so later evidence can
resolve delimiter decoys in the header. A uniquely detected comma retains the
legacy physical-line analyzer. A uniquely detected alternate delimiter uses
the logical-record analyzer, whose first logical record is the header and whose
subsequent records may contain quoted newlines.

Equal best candidates are an error rather than an inferred winner. The error
lists candidate labels in canonical order, exits with code 2, and produces no
JSON stdout. Undetected complete input consults the legacy comma analyzer so
specific compatibility outcomes remain stable; its generic non-CSV preflight
failure is reported instead as unsupported/unstructured input. Completed
malformed input retains the legacy error authority. A detection cap after a
safe complete prefix may select that prefix's parser, after which normal
sampling fields disclose the partial coverage. If the first quoted logical
header itself crosses the cap and no parser can be established, analysis exits
with a bounded detection error and no JSON stdout. Consequently V1 has neither
an `unknown`/`ambiguous` parser token nor a JSON error envelope.

## Sampling semantics

The legacy comma analyzer uses the first physical record as its header. The
alternate-delimiter analyzer uses the first logical record as its header.
Both count accepted nonblank data records in `records_analyzed`; quoted
physical newlines inside one alternate record do not increment that count.

| Condition | `scope` | `completeness` | `limited` | `limit_reached` |
|---|---|---|---:|---|
| Every source byte analyzed and no hard limitation | `full` | `complete` | `false` | `null` |
| Byte budget stopped analysis | `sampled` | `partial` | `true` | `byte_limit` |
| Record-count budget stopped analysis | `sampled` | `partial` | `true` | `record_limit` |
| Header byte cap stopped analysis | `sampled` | `partial` | `true` | `header_byte_limit` |
| Data-record byte cap stopped analysis | `sampled` | `partial` | `true` | `record_byte_limit` |
| Column cap stopped fact construction | `full` or `sampled` | `partial` | `true` | `column_limit` |
| Accounting base could not fit | `full` or `sampled` | `partial` | `true` | `memory_limit` |
| Shared cardinality budget was exhausted after a full scan | `full` | `partial` | `true` | `cardinality_memory_limit` |

`bytes_read` is capped by `configured_max_bytes` and by any narrower consumer
scope. For legacy comma analysis, `bytes_analyzed` includes only complete
physical records accepted for field analysis. For alternate analysis, it
includes only complete logical records. In both cases it excludes an
incomplete fragment retained up to a byte cap.

The CLI performs the bounded multi-record detection read before the reported
coverage interval. A detected comma then also passes through the legacy
analyzer's fixed 4 KiB prefix preflight. These reads are not added to
`bytes_read` or `bytes_analyzed`; those fields are selected-analyzer scanner
counts rather than whole-process I/O counters or RSS measurements.

`scope` describes source-byte coverage; `completeness` also accounts for hard
fact limitations. Consequently `scope: "full"` can coexist with
`completeness: "partial"`. `final_newline` is `true` or `false` whenever every
source byte was consumed, even if another limitation made facts partial. It is
`null` for a partial byte scan.

## Estimates and unknown values

Cardinality is tagged:

- `{"kind":"exact","value":N}` means the compatible tracker retained every
  observed distinct hash.
- `{"kind":"at_least","value":N}` means tracking was censored after at least
  `N` distinct stable hashes were established. The established local threshold
  produces `8193`; shared-budget pressure can produce a different defensible
  lower bound. It is not an exact count.
- `{"kind":"unknown","value":null}` is reserved for an analyzer that cannot
  establish a defensible value or lower bound.

Repetition is exact only when the sample has observed values and cardinality is
exact. Otherwise it is `{"kind":"unknown","value":null}`. The internal legacy
planner sentinel is not emitted as a factual value.

Minimum, maximum, and mean byte length are `null` when no values were observed.
`numeric_values` counts raw lexical fields accepted by the existing integer or
float parse checks; it is not a type declaration. Alternate logical fields
retain quotes, doubled quotes, and embedded newlines in these factual byte
metrics rather than exposing a normalized value.

All planner sizes, savings, and memory numbers are estimates. MiB and KiB names
use 1024-based units. Normally `selection_scope` is `planner_recommendation`
and selections identify `PlannerPolicyV1` results. When a hard limitation makes
structured planning unsafe, `selection_scope` is `safe_fallback`, the selected
archive mode is `raw_zstd`, and reason code
`ANALYSIS_LIMITED_RAW_ZSTD_FALLBACK` identifies the compatibility adapter's
override.

For a successfully analyzed semicolon, tab, or pipe dialect without a hard
limitation, `selection_scope` is `planner_recommendation`. Phase 7 enables the
existing DCSV01 structured payload for all four detected delimiters, so the
Phase 6 format-capability override is no longer applied to current analyze
reports. Column recommendations remain policy observations and do not prove
which encoding an archive writer executed: full-input dialect agreement,
codec validation, and candidate comparison can still select RawZstd.

## Stable codes

Archive reason codes:

- `ANALYSIS_LIMITED_RAW_ZSTD_FALLBACK`
- `STRUCTURED_COMPRESSION_NOT_ENABLED_FOR_DIALECT`
- `INSUFFICIENT_REPETITION_MAJORITY`
- `PROJECTED_DICTIONARY_SAVINGS_BELOW_THRESHOLD`
- `HIGH_REPETITION_DETECTED`

`STRUCTURED_COMPRESSION_NOT_ENABLED_FOR_DIALECT` remains reserved in the V1
code domain for Phase 6 reports even though current complete supported-dialect
analysis no longer emits that capability fallback.

Column reason codes:

- `CARDINALITY_THRESHOLD_EXCEEDED`
- `VERY_LOW_CARDINALITY_HIGH_REPETITION`
- `REPETITION_SUPPORTS_DICTIONARY`
- `NUMERIC_OR_DATE_HEURISTIC_MATCHED`
- `NO_STRONG_DICTIONARY_SIGNAL`

Diagnostic codes:

- `SAMPLE_BYTE_LIMIT_REACHED`
- `SAMPLE_RECORD_LIMIT_REACHED`
- `INCOMPLETE_HEADER_SAMPLE`
- `HEADER_BYTE_LIMIT_REACHED`
- `RECORD_BYTE_LIMIT_REACHED`
- `COLUMN_LIMIT_REACHED`
- `ANALYSIS_MEMORY_LIMIT_REACHED`
- `CARDINALITY_MEMORY_LIMIT_REACHED`
- `CARDINALITY_LIMIT_REACHED`
- `EMPTY_COLUMN_NAME`
- `DUPLICATE_COLUMN_NAME`

Every diagnostic contains `code`, `severity`, `message`, and a nullable
`column_index`. `CARDINALITY_LIMIT_REACHED` means a column's stable-hash
cardinality was censored by an analysis limit; the shared-budget diagnostic
additionally explains global pressure. Consumers should branch on `code`,
tolerate unknown codes within V1, and treat messages as human-readable text.
Header- and record-byte diagnostic messages say `physical` for the legacy
comma parser and `logical` for an alternate parser; the stable code is the
machine contract.

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

Running `datapack analyze INPUT` without `--json` keeps legacy comma text
unchanged. Alternate-delimiter text adds a `Detected dialect` line and escapes
control characters in displayed header labels; this presentation does not
alter facts or input bytes.

Compression now uses the same multi-delimiter analysis dispatcher. Before it
executes a structured candidate, canonical full-input detection must agree
with the analyzed delimiter and the existing DCSV01 codec must prove exact
reconstruction. Unsafe, ambiguous, unsupported, or unbeneficial candidates use
RawZstd. The frozen `csv_columnar_dictionary` archive mode remains the DCSV01
wire identifier for every supported delimiter; JSON V1 does not add a new
archive-mode token. Benchmark retains its legacy comma-only planning and
execution scope in this phase.
