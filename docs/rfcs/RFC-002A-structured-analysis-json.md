# RFC-002A — Structured Analysis JSON V1

- Status: Phase 3 complete; all mandatory integrated quality gates passed
- Date: 2026-08-06
- Baseline: `rfc-002-cli-modularization`
- Related: [RFC-001B](RFC-001B-compatible-analysis-core.md) and
  [RFC-002](RFC-002-cli-modularization.md)

> **Follow-up audit (2026-08-10):** Later certified phases fulfilled the
> deferrals recorded below: [RFC-003](RFC-003-hard-bounded-analysis.md) added
> hard bounds, [RFC-004A](RFC-004A-delimited-analysis.md) added truthful
> multi-delimiter analysis, [RFC-005](RFC-005-planner-encoder-contract.md)
> enforced planner/encoder execution, and
> [RFC-005D](RFC-005D-public-rust-application-api.md) re-exported the V1 report
> through the public Application API. The historical Phase 3 body remains the
> checkpoint contract; these follow-ups did not place the report in v1/v2 wire
> metadata.

## 1. Decision

Phase 3 adds a versioned, privacy-conscious JSON representation of the
compatible analysis result:

```text
AnalysisEngine
  -> DatasetAnalysis
  -> AnalysisReportV1 conversion
  -> compact or pretty JSON renderer
```

`AnalysisReportV1` is a private data-transfer boundary. It does not become a
public Rust API, does not participate in `.dpack` serialization, and is not a
type alias for the internal factual or planner models. The existing legacy
text renderer remains a separate consumer of the same `DatasetAnalysis`.

The command forms are:

```text
datapack analyze INPUT --json
datapack analyze INPUT --json --pretty
```

Without `--json`, the command retains its byte-for-byte legacy presentation.
`--pretty` requires `--json`. The existing `--plan` flag remains a text-mode
presentation control; the JSON report always includes the planner section, so
combining `--plan` with `--json` does not change JSON content.

## 2. Invariants

> Current-behavior note: RFC-003 later replaced characterized read overshoot
> with a hard-bounded reader and extended JSON V1 with truthful hard-limit
> outcomes. The invariants below record the certified Phase 3 contract; see
> `docs/reference/ANALYSIS_JSON_V1.md` for the current V1 contract.

1. `schema_version` is the integer `1`.
2. JSON names and enum values use explicit V1 conversions. They do not use
   Rust `Debug` output or the legacy text labels.
3. No raw field value, sampled row, input path, filename, or header name is
   serialized.
4. Column identity is its zero-based index. Empty and duplicate-name facts are
   reported without disclosing the names.
5. Cardinality comes from `ColumnFacts.cardinality`, never from the inflated
   `PlannerPolicyV1` compatibility sentinel.
6. Partial coverage is labeled as sampled, partial, and limited, with the real
   byte or record stop reason.
7. `bytes_read` and `bytes_analyzed` remain distinct. V1 does not conceal the
   characterized legacy read overshoot.
8. Unknown values are JSON `null` or a tagged `unknown` estimate; compatibility
   sentinel values are not presented as measurements.
9. Planner selections are advisory `PlannerPolicyV1` results. They are not
   described as actual encoder execution.
10. Planner timing is omitted because it is nondeterministic and is not needed
    to explain the decision.
11. Stable reason and diagnostic codes accompany descriptive messages.
12. Successful JSON mode writes one JSON document and a trailing newline to
    stdout, with no presentation text mixed into it.
13. Analysis failures retain the established exit codes and text error path;
    JSON V1 does not define an error envelope.

## 3. DTO boundary

The private DTO family is conceptually:

```text
AnalysisReportV1
  DatasetReportV1
    ParserReportV1
    ColumnReportV1[]
      ColumnNameStatusReportV1
      ValueLengthReportV1
      CardinalityReportV1
      RepetitionReportV1
      ColumnPlannerReportV1
  SamplingReportV1
  PlannerReportV1
    PolicyReportV1
    ReasonReportV1
  DiagnosticV1[]
```

Serialization derives belong only to these DTOs. The following remain free of
JSON schema concerns:

- `DatasetFacts` and its nested factual types;
- `PlannerFeaturesV1` and `PlannerPolicyV1`;
- `CompressionPlan`, `ColumnPlan`, and `ColumnProfile`;
- `DpackMetadata` and every frozen v1/v2 serialized type.

The module is private and may be re-exported only as `pub(crate)` for the CLI
renderer. A public application/report API is deferred to Phase 12.

## 4. Truthful fact mappings

### 4.1 Dataset and parser

The successful compatible engine currently establishes only that it used the
legacy comma-delimited, physical-line parser with the first physical record as
the header. JSON V1 reports that parser contract. It does not claim dialect
detection, RFC-complete logical records, newline-style detection, or header
inference.

The report includes source byte size and column count. It deliberately omits
the input basename and all column names.

### 4.2 Sampling

Complete analysis maps to:

```text
scope = full
completeness = complete
limited = false
limit_reached = null
```

A byte- or record-limited analysis maps to:

```text
scope = sampled
completeness = partial
limited = true
limit_reached = byte_limit | record_limit
```

The report preserves source bytes, bytes read, bytes analyzed, sampled data
records, configured maximum bytes, configured maximum records, and nullable
final-newline state. The first physical record is the legacy header and is not
included in `records_analyzed`.

These fields describe current behavior; they are not a claim that the Phase 3
reader is hard-bounded. Hard bounds are Phase 4 work.

### 4.3 Columns

Column facts use physical UTF-8 field slices from the compatible parser:

- `observed_values`, `empty_values`, and `numeric_values` are counts;
- minimum, maximum, mean, and total value lengths are byte-based;
- minimum, maximum, and mean are `null` when no values were observed;
- `numeric_values` is a parse-success count, not an inferred type;
- empty and duplicate header-name status is disclosed without the name.

Cardinality is a tagged estimate:

```json
{"kind":"exact","value":120}
```

or:

```json
{"kind":"at_least","value":8193}
```

The V1 contract also reserves:

```json
{"kind":"unknown","value":null}
```

for a future bounded analyzer that cannot establish even a lower bound. The
compatible Phase 3 engine produces `exact` or `at_least` for every successful
column analysis.

Observed repetition is exact only when at least one value was observed and
cardinality is exact. It is otherwise represented as `unknown`; the policy's
legacy zero sentinel is never emitted as a factual repetition measurement.

### 4.4 Planner

The planner section names `PlannerPolicyV1`, reports version `1`, enumerates
the archive modes and column strategies the policy can choose, and gives the
archive mode selected by that policy. Each column reports its selected policy
recommendation. Candidate strategy names appear once in the planner section
rather than being repeated for every column.

Selection is explicitly scoped to the planner. The current encoder can make
independent column-mode choices; JSON V1 does not claim that the recommended
column strategy was executed. Phase 8 owns the planner/encoder execution
contract.

Sizes, savings, and memory values retain `estimated_` names. The existing
binary 1024-based memory calculation is labeled MiB. No actual archive ratio,
runtime memory, confidence percentage, or performance prediction is added.

## 5. Stable codes

Planner archive reason codes correspond exactly to the current V1 decision
branches:

- `INSUFFICIENT_REPETITION_MAJORITY`
- `PROJECTED_DICTIONARY_SAVINGS_BELOW_THRESHOLD`
- `HIGH_REPETITION_DETECTED`

Column policy reason codes correspond exactly to the current recommendation
branches:

- `CARDINALITY_THRESHOLD_EXCEEDED`
- `VERY_LOW_CARDINALITY_HIGH_REPETITION`
- `REPETITION_SUPPORTS_DICTIONARY`
- `NUMERIC_OR_DATE_HEURISTIC_MATCHED`
- `NO_STRONG_DICTIONARY_SIGNAL`

Diagnostics are emitted only from facts already present in the model:

- `SAMPLE_BYTE_LIMIT_REACHED`
- `SAMPLE_RECORD_LIMIT_REACHED`
- `CARDINALITY_LIMIT_REACHED`
- `EMPTY_COLUMN_NAME`
- `DUPLICATE_COLUMN_NAME`

Diagnostic order is deterministic: the sampling diagnostic first, followed by
column diagnostics in index order. Messages are descriptive; consumers should
branch on codes.

## 6. Compatibility and security boundaries

Phase 3 does not change:

- `.dpack` v1 or v2 metadata, headers, payloads, writers, or readers;
- frozen compatibility fixtures;
- compression defaults, planning formulas, or encoder behavior;
- benchmark methodology;
- the public `analysis::analyze_bytes` compatibility API;
- default analyze text or its nine frozen goldens.

The report contains aggregate counts, estimates, stable codes, and zero-based
column indexes. It excludes source values and identifiers that would reproduce
user content. JSON escaping is delegated to `serde_json`; hand-built string
escaping is not used.

Promoting the already-locked `serde_json` crate from development-only use to a
runtime dependency is required by this phase. Its locked version and the
resolved dependency graph remain unchanged.

## 7. Verification plan

Focused tests must establish:

- schema version, stable enum spellings, and required top-level sections;
- compact and pretty representations parse to identical JSON values;
- `--pretty` without `--json` is rejected by Clap;
- full versus record-limited sampling is reported truthfully;
- censored cardinality is `at_least 8193`, not a fake exact value;
- raw row values, paths, filenames, and header names do not appear;
- planner modes, strategies, and reason codes match the same analysis consumed
  by legacy text and compression;
- the nine legacy analyze goldens remain unchanged;
- all frozen fixture, round-trip, security, and planner characterization gates
  remain green.

## 8. Landed implementation

The Phase 3 implementation consists of:

- `analysis/report.rs`: the private V1 DTO family, factual/policy conversion,
  explicit stable enum strings, diagnostics, reason codes, and conversion
  invariant checks;
- `cli/analysis_json.rs`: compact/pretty serde serialization streamed to
  stdout after analysis and report validation;
- `cli/mod.rs`: the additive `--json` and `--pretty` Analyze options and the
  presentation-mode dispatch;
- `tests/analysis_json_cli.rs`: schema, semantics, privacy, determinism,
  sampling, error, and real-policy branch coverage;
- `tests/cli_surface.rs`: the authorized Analyze surface addition and the
  `--pretty`/`--json` relationship;
- `docs/reference/ANALYSIS_JSON_V1.md`: the consumer contract.

The DTO borrows planner reason text instead of cloning it, lists candidate
column strategies once at planner scope, and streams JSON instead of retaining
a second complete output buffer. The report still contains one small DTO entry
per analyzed column. Phase 4 owns the already-characterized maximum-column and
reader bounds; this phase does not claim those limits already exist.

`serde_json` moved from `[dev-dependencies]` to `[dependencies]` at the same
locked version. `Cargo.lock` did not change, so the resolved dependency graph
is unchanged.

## 9. Certification

Certification ran under WSL/Linux with Rust and Cargo 1.85.0 and
`CARGO_TARGET_DIR=/home/gompr/.cache/datapack-modernization`.

| Gate | Result |
|---|---:|
| `cargo fmt --check` | PASS |
| `cargo check --locked` | PASS |
| `cargo test` | 221 passed, 0 failed, 0 ignored |
| `cargo clippy --all-targets --all-features -- -D warnings` | PASS |
| `analysis_json_cli` | 9 passed |
| `analyze_cli` | 9 passed; legacy goldens unchanged |
| `compatibility_fixtures` | 4 passed |
| `analysis_core_consistency` | 3 passed |
| `legacy_analysis_api` | 4 passed |
| `round_trip` | 31 passed |
| `security_hardening` | 69 passed |
| `planning::tests` | 26 passed |

The final diff review found no change to `Cargo.lock`, serialized metadata,
v1/v2 storage code, format/codec code, `PlannerPolicyV1`, frozen fixtures, or
legacy analyze goldens. No benchmark was run and this RFC makes no performance
claim.
