# RFC-005C — DataPack Advisor

- Status: Implemented
- Date: 2026-08-06
- Baseline: `rfc-010-datapack-compare`
- Related: [Advisor JSON V1](../reference/ADVISOR_JSON_V1.md)

## 1. Decision

Phase 11 adds a deterministic advisory command over the existing compatible
analysis core and `PlannerPolicyV1`:

```text
datapack advisor INPUT
datapack advisor INPUT --sample-mb 32
datapack advisor INPUT --json
datapack advisor INPUT --json --pretty
```

`--sample-mb` defaults to 64 MiB and retains the analysis command's accepted
range and bounded-reader semantics. `--pretty` requires `--json`. The default
renderer is human-readable; JSON uses the versioned `AdvisorReportV1` contract
documented in `docs/reference/ADVISOR_JSON_V1.md`.

Advisor is a policy projection, not another analyzer. It calls the shared
dialect-aware `analyze_cli_path` entry point and consumes its existing
`DatasetAnalysis` facts and `PlannerPolicyV1` result. `AdvisorPolicyV1` then
maps those typed observations and stable reason codes to a small ordered set
of explanatory recommendations. It does not parse input independently,
duplicate delimiter detection, or modify `PlannerPolicyV1`.

Advisor does not run Compare implicitly or ingest an earlier Comparison
report. It performs no compression, decompression, archive validation, timing,
hashing, or temporary-file work. When analysis is partial or structured
analysis is unavailable, the report can recommend an explicit Full comparison
as a separate next action. Prior Comparison-report ingestion is deferred; it
is not part of the completed Phase 11 contract.

## 2. Internal boundary

The advisory service, policy, and DTO remain crate-private. The CLI module is
only an adapter responsible for argument handling and text/JSON rendering.
This preserves the Phase 12 boundary for the public Rust application API and
keeps Clap and terminal behavior out of advisory policy.

`AdvisorReportV1` is a dedicated DTO. It does not serialize
`DatasetAnalysis`, `DatasetFacts`, `CompressionPlan`, or the analysis JSON DTO
directly. The report instead converts relevant internal values to explicit V1
tokens. Stable analysis and planner reason-code conversion is shared with the
analysis report so the two machine-readable views cannot silently assign
different codes to the same fact.

No dependency or archive metadata change is required.

## 3. Deterministic policy

For a supported structured analysis, `AdvisorPolicyV1` emits recommendations
in a fixed order:

1. exactly one compression-strategy recommendation;
2. a high-cardinality observation when its lower-bound requirement is met;
3. an analysis-limitation recommendation when analysis is partial; and
4. a Full-comparison recommendation when analysis is partial.

Evidence entries and affected column indexes retain deterministic source and
column order. Every recommendation has typed evidence whose source is one of
`analysis_status`, `analysis_diagnostic`, `planner_policy`, `column_policy`,
`safety_policy`, or `compatibility_policy`. Human-readable messages are
presentation; the `(source, code, column_index)` tuple is the stable
explanation boundary.

### 3.1 Compression strategy

Exactly one of these codes is present in every successful report:

- `STRUCTURED_COMPRESSION_RECOMMENDED` when the existing plan selects
  `CsvColumnarDictionary`; or
- `RAW_ZSTD_RECOMMENDED` when the existing plan selects `RawZstd` or when
  structured analysis is unavailable.

For an available analysis, `planner.selection_scope` distinguishes an actual
`planner_recommendation` from a `safe_fallback` or `format_fallback`. Their
selected-mode evidence respectively uses `planner_policy`, `safety_policy`, or
`compatibility_policy`. Only the first is described as a `PlannerPolicyV1`
recommendation. A safety- or compatibility-selected RawZstd result is
described as a fallback, so the presentation does not attribute an override
to the planner. Advisor does not independently recalculate repetition,
projected size, or a break-even threshold.

A structured recommendation applies only to the analyzed scope. It is not a
promise that every unexamined source byte is structured or that the encoder
must emit a columnar archive. The existing full-input eligibility check,
execution-plan enforcement, dictionary limits, and encoder safety checks
retain authority and may select the byte-exact RawZstd fallback.

### 3.2 High cardinality

`HIGH_CARDINALITY_OBSERVED` is emitted only for a column whose factual
cardinality is an `at_least` estimate of 8,193 or greater. It identifies
affected columns by zero-based index. `CARDINALITY_LIMIT_REACHED` is typed as
an `analysis_diagnostic`; a corresponding
`CARDINALITY_THRESHOLD_EXCEEDED` planner branch is typed as `column_policy`.
It does not expose a column name or promote a censored lower bound to an exact
value.

This threshold is important because shared cardinality-memory pressure can
censor tracking at a smaller defensible lower bound. Such a smaller
`at_least` value remains truthful analysis data but is not labeled
high-cardinality by Advisor V1.

### 3.3 Partial analysis

Analysis is partial when its configured sample scope stops before completion
or when a hard analysis limit prevents complete facts. Advisor emits
`ANALYSIS_LIMITED` with the applicable stable analysis diagnostic evidence.
Examples include byte and record sample limits, header or record byte limits,
column limits, and analysis or cardinality-memory limits.

The same partial result also emits `FULL_COMPARISON_RECOMMENDED`. This is a
recommendation to measure and round-trip the complete input explicitly with
`datapack compare INPUT --mode full`; it is not a promise that Full comparison
will remove the analyzer's record, column, cardinality, or memory safety
limits. RawZstd remains the fallback when those limits prevent safe structured
execution.

### 3.4 Structured analysis unavailable

Ambiguous, malformed, and unsupported or unstructured inputs can make the
shared structured analyzer return `InvalidCsv` without a `DatasetAnalysis`.
Advisor treats this specific outcome as a successful, truthful advisory
result:

- analysis status is `unavailable`;
- `RAW_ZSTD_RECOMMENDED` is emitted;
- `FULL_COMPARISON_RECOMMENDED` is emitted; and
- both recommendations cite `STRUCTURED_ANALYSIS_UNAVAILABLE` with source
  `analysis_status`.

The raw parser error text is not included in the report. Other errors,
including I/O failures, invalid bounds, allocation failures, and internal
format invariants, are not converted into recommendations. They remain command
failures so Advisor cannot conceal a safety or operational problem.

## 4. Explainability and non-claims

Advisor recommendations are deterministic mappings from facts, existing
policy decisions, and typed stable evidence. Advisor does not use an AI or
LLM, and it emits no confidence percentage, composite score, overall winner,
or probabilistic label.

It also makes no performance claim. `FULL_COMPARISON_RECOMMENDED` asks the user
to collect measurements; it does not predict their winner. The structured and
RawZstd messages describe the current compatibility planner or safe fallback,
not a guaranteed ratio or throughput.

Advisor does not activate `DeltaCandidate`, RLE, bit packing, or another
latent strategy. Any column-level planner observations remain subject to the
existing Phase 8 execution contract.

## 5. Privacy and determinism

Neither text nor JSON output contains:

- input or temporary paths;
- file names;
- header or column names;
- raw field values or sample rows;
- source, archive, or restored bytes;
- hashes;
- planning times or other timings; or
- raw parser errors.

Column-specific evidence uses only a zero-based index. Fixed recommendation
messages contain no interpolated user content. Planning time is deliberately
excluded, so unchanged bytes and options produce deterministic report content.

Advisor opens the input read-only and creates no archive, restored output,
`.partial` file, comparison workspace, or other temporary artifact.

## 6. Exit and output behavior

- Exit `0` means a complete advisory report was emitted. This includes
  partial analysis and the typed `InvalidCsv` structured-unavailable outcome.
- Exit `1` covers operational or semantic failures such as unreadable input,
  invalid analysis limits, allocation failure, or internal invariant failure.
- Exit `2` is reserved for Clap rejection, including `--pretty` without
  `--json`.

On failure, stdout is empty and the normal CLI error is written to stderr.
Advisor JSON V1 has no error envelope. A successful text or JSON report is
written to stdout and does not use stderr for recommendations or cautions.

## 7. Compatibility

Phase 11 does not change:

- the v1 serialized metadata graph, header, or payload bytes;
- the DCSV01 payload representation;
- the v2 fixed header, chunk table, or payload layout;
- frozen v1/v2 compatibility fixtures;
- legacy analyze text or goldens;
- `PlannerPolicyV1` decisions;
- structured-compression eligibility or encoder fallback behavior;
- compression defaults;
- Compare or Benchmark methodology and output;
- public Rust APIs; or
- the Cargo dependency graph.

Advisor is read-only and does not write advice into an archive.

## 8. Verification scope

Focused integration tests cover compact and pretty JSON; strict schema and
typed-evidence fields; deterministic recommendation ordering; nonempty
recommendation evidence also present in `analysis.evidence`; exactly one
compression recommendation; structured and RawZstd planner branches; shared
pipe-delimited analysis; a partial record-count sample; the 8,193
high-cardinality lower-bound case; structured-analysis-unavailable fallback;
privacy; the text renderer; and empty stdout for missing-input and
invalid-sample-limit failures. A focused policy unit test
confirms that an `at_least` estimate below 8,193 and an exact value at the
threshold are not promoted to the high-cardinality advisory. The separate
CLI-surface test covers the command flags, default sample size, and the
`--pretty` requirement.

The tests do not assert a compression ratio, throughput, winner, confidence,
or execution mode beyond the actual planner facts supplied to
`AdvisorPolicyV1`.

## 9. Limitations

- Advisor V1 consumes analysis and planner facts only. Ingesting a prior
  Comparison report is explicitly deferred.
- A sampled recommendation describes the analyzed scope and can differ from a
  complete-input encoder outcome.
- A Full comparison is an explicit follow-up operation and can be expensive.
- Full comparison retains all bounded-analysis and structured-encoder safety
  limits.
- High-cardinality reporting is deliberately conservative and does not infer
  exact cardinality from a censored estimate.
