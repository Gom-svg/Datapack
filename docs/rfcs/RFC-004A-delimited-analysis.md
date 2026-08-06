# RFC-004A — Multi-Delimiter Analysis

- Status: Implemented and technically certified
- Date: 2026-08-06
- Baseline: `rfc-005-delimited-engine`
- Related: [RFC-002A](RFC-002A-structured-analysis-json.md),
  [RFC-003](RFC-003-hard-bounded-analysis.md), and
  [RFC-004](RFC-004-delimited-data-engine.md)

## 1. Decision

Phase 6 extends the `datapack analyze` adapter to recognize four delimited
formats by content and adds negative eligibility guards that keep alternate
formats out of the existing structured encoder until Phase 7:

| Delimiter | Analysis JSON V1 `format` |
|---|---|
| comma (`,`) | `csv` |
| semicolon (`;`) | `semicolon_delimited` |
| tab (`\t`) | `tsv` |
| pipe (`|`) | `psv` |

The routing is deliberately separate from compression and benchmark planning:

```text
CLI analyze
  -> bounded multi-record detection prefix
       -> comma: legacy physical-line AnalysisEngine
       -> semicolon/tab/pipe: bounded logical-record analyzer
       -> ambiguous: deterministic CLI error
       -> undetected: preserve specific legacy comma outcomes, otherwise
            report unsupported/unstructured input
       -> completed malformed input: legacy error authority
       -> detection cutoff without safe evidence: explicit bounded error

compress and benchmark
  -> unchanged legacy AnalysisEngine facts and PlannerPolicyV1
  -> canonical comma-only capability guard
  -> execution-time candidate-byte guard
  -> RawZstd unless comma eligibility is established
```

This phase does not make the new detector an encoder selector. Phase 7 remains
the first phase authorized to add safe structured-compression routing for
additional delimiters.

## 2. Compatibility authorities

The implementation leaves these authorities in place:

- `analysis::analyze_path` and `analysis::analyze_path_with_scope` retain the
  comma-only physical-record behavior used by compression and benchmark;
- `PlannerPolicyV1` formulas, thresholds, reasons, and direct legacy inputs
  remain unchanged;
- the public `analysis::analyze_bytes` and `formats::csv` behavior remains
  separate and unchanged;
- `FileType`, `CsvAnalysis`, `NewlineStyle`, `PayloadKind`, and every serialized
  metadata type remain unchanged;
- v1/v2 readers, writers, fixtures, and wire representations are outside this
  phase;
- CLI options and requested defaults are unchanged; the benchmark run/median
  methodology is unchanged. Phase 6 adds only a conservative RawZstd
  capability fallback when canonical comma eligibility is not established.

File extensions do not select or break ties between dialects. In particular,
the frozen metadata API does not gain TSV, PSV, or semicolon variants even
when the analyze command recognizes those contents.

## 3. Detection and dispatch

The CLI dispatcher accumulates at most 32 complete physical records and at
most 1 MiB, further capped by the configured sample allowance and consumer
scope. It evaluates the canonical detector after each completed physical
boundary and retains the outcome for the longest successfully evaluated
prefix. This extra evidence resolves header-only delimiter decoys without
retaining the full file or an unbounded sample.

The RFC-004 detector evaluates candidates in the stable order comma,
semicolon, tab, then pipe. It uses candidate-local quote state and counts only
separators outside quoted fields. It does not use lossy UTF-8 conversion or a
filename extension.

Dispatch behavior is:

- a uniquely detected comma delegates to the unchanged legacy analyzer;
- a uniquely detected semicolon, tab, or pipe selects the logical-record
  analyzer;
- equal best candidates produce an `InvalidCsv` error whose candidate labels
  retain canonical order;
- an undetected delimiter consults the legacy analyzer, preserving specific
  certified comma outcomes while translating its generic non-CSV preflight
  error into an explicit unsupported/unstructured error;
- a completed malformed prefix delegates to the legacy analyzer, which remains
  authoritative for certified comma syntax errors;
- allocation, counter, and unexpected detector-limit failures are propagated;
- if an artificial cutoff follows a successful complete prefix, that prefix
  may select the parser, while the selected analyzer still reports its own
  partial coverage;
- if the first bounded header fragment provides unique unquoted delimiter
  evidence, it may select only the parser that will emit the header-limit
  result; ambiguous or syntactically incomplete evidence produces an explicit
  bounded detection error rather than an invented delimiter.

Ambiguity is a normal text error even when `--json` was requested. It exits
with code 2, writes no JSON document to stdout, and reports a reason beginning
`ambiguous delimiter; candidates:`. JSON V1 does not add an error envelope.

Quoted multiline records are evaluated once a closing physical boundary makes
the prefix syntactically complete. If the first quoted logical header itself
crosses the detection cap, there is no safe parser fact to serialize; the CLI
returns the explicit bounded detection error and no JSON document. Undetected
complete input preserves any specific legacy comma outcome; otherwise it emits
the explicit unsupported/unstructured text error. JSON V1 does not add an
`unknown` parser object or error envelope.

## 4. Alternate logical-record analysis

The alternate analyzer is crate-private and holds one bounded logical record
at a time. Its RFC-004-compatible grammar:

- recognizes a quote only at field start;
- treats doubled quotes as an escaped pair without rewriting them;
- ignores delimiters and LF/CRLF while inside a quoted field;
- accepts LF and CRLF logical-record endings;
- accepts empty fields, quoted multiline data fields, and a missing final
  newline;
- rejects an unterminated quote or bare carriage return at actual EOF.

The first logical record is always the header; there is no header inference.
Blank logical data records are skipped. A nonblank row whose width differs
from the header is rejected.

Field facts retain lexical source representation. Quotes, doubled quotes,
embedded newlines, spaces, and leading zeroes are neither unescaped nor
normalized. Outer matching quotes are stripped only when constructing the
internal header name, matching the established analyzer convention. Every
record must be valid UTF-8 and contain no NUL byte before its fields reach the
existing factual accumulator.

The analyzer reuses the Phase 4 sample, header, record, column, cardinality,
and accounted-memory limits. A sample or record cutoff never emits an
incomplete logical record as complete. `bytes_read` includes the bounded
incomplete fragment, while `bytes_analyzed` stops after the last complete
logical record.

## 5. Facts, PlannerPolicyV1, and format fallback

`DatasetFacts` carries an internal, nonserialized parser fact. It distinguishes
the legacy comma physical-line parser from a canonical logical-record parser
and its delimiter. JSON conversion maps this fact explicitly rather than
serializing scanner enums.

Alternate facts pass through the unchanged `PlannerPolicyV1` feature and
column-profile formulas. A separate plan disposition then prevents the
analysis report from recommending an encoder route that Phase 6 has not
authorized:

| Situation | `selection_scope` | Selected archive mode |
|---|---|---|
| complete legacy comma analysis | `planner_recommendation` | PlannerPolicyV1 result |
| hard-limited analysis | `safe_fallback` | `raw_zstd` |
| alternate-delimiter analysis without a hard limit | `format_fallback` | `raw_zstd` |

Format fallback uses reason code
`STRUCTURED_COMPRESSION_NOT_ENABLED_FOR_DIALECT`. It is a capability boundary,
not an analysis limitation. A complete alternate analysis therefore remains
`sampling.completeness: "complete"` and `sampling.limited: false` even though
its planner section selects RawZstd.

## 6. Analysis JSON V1

Phase 6 keeps the existing `dataset.parser` object and extends only its
documented string values:

| `format` | `delimiter` | `record_model` | `header_mode` |
|---|---|---|---|
| `csv` | `,` | `physical_line` | `first_record` |
| `semicolon_delimited` | `;` | `logical_record` | `first_record` |
| `tsv` | tab character (JSON-encoded as `\t`) | `logical_record` | `first_record` |
| `psv` | `|` | `logical_record` | `first_record` |

The CLI routes detected comma input through the legacy parser, so its existing
parser object and factual semantics remain unchanged. Successful alternate
reports do not include raw header names, field values, sample rows, paths,
timestamps, or confidence scores.

The parser object describes the parser used for the reported facts. For a
partial sample it does not claim that unexamined bytes satisfy the dialect.
Ambiguous and unsupported inputs produce no report object because JSON V1
retains its established text-error model.

## 7. Text presentation

Legacy comma text continues through the existing renderer without an added
dialect line. Alternate reports add exactly one line after original size:

```text
Detected dialect:  TSV (tab)
```

The other labels are `semicolon-delimited (;)` and `PSV (|)`. Alternate header
labels escape tab, LF, and CR as `\t`, `\n`, and `\r`; other control characters
display as `?`. Escaping and the existing 14-character display truncation do
not change facts or input bytes.

## 8. Compression boundary and Phase 7 deferral

`compress` and `benchmark` do not use alternate facts for planning; both retain
the legacy comma compatibility analyzer and PlannerPolicyV1. Before either may
execute a structured candidate, however, the shared canonical detector must
establish comma eligibility within the bounded planning scope. The common
archive adapter repeats the comma-only check over already-loaded candidate
bytes with fixed logical-record and column caps. Failure at either gate selects
RawZstd, including under `--mode best` and `--verify-best`.

These are negative capability checks, not Phase 7 support. They do not route
tab, pipe, or semicolon facts to DCSV01, change frozen payload metadata, or
claim that the existing delimiter byte is safe for new formats. Auditing that
payload and deliberately enabling byte-exact alternate structured compression
remain Phase 7 work.

## 9. Required verification before certification

Phase 6 certification must cover:

- stable JSON format, delimiter, record-model, and header-mode values for all
  four delimiters;
- unchanged comma JSON semantics and all nine legacy text goldens;
- quoted delimiter decoys, header-only decoy resolution, and multiline logical
  data records;
- LF, CRLF, UTF-8, empty fields, and missing final newline;
- deterministic ambiguity and legacy fallback for undetected input;
- JSON privacy for every alternate delimiter;
- alternate header, logical-record, column, cardinality, memory, and sample
  limits;
- control-character-safe alternate text;
- RawZstd-only compression and benchmark execution for alternate delimiters,
  including inputs that also satisfy the legacy comma preflight;
- frozen compatibility fixtures, round trips, security hardening,
  PlannerPolicyV1 characterization, formatting, check, tests, and clippy.

No certification result is asserted by this document until those commands
have been run from the committed Phase 6 state.

## 10. Certification results

Certification used Rust 1.85.0 and the Linux-side modernization target
directory. Actual results from the final Phase 6 working tree were:

- `cargo fmt --check`: pass;
- `cargo check`: pass;
- `cargo test`: 276 passed, 0 failed, 0 ignored;
- `cargo clippy --all-targets --all-features -- -D warnings`: pass;
- frozen compatibility fixtures: 4 passed;
- legacy analyze goldens: 9 passed;
- round trip: 31 passed;
- security hardening: 69 passed;
- `planning::tests`: 32 passed;
- Phase 6 multi-delimiter CLI analysis: 12 passed;
- shared delimited engine: 22 passed;
- DCSV01 adapter/codec characterization: 22 passed.

The protected-surface review found no Cargo dependency, serialized metadata,
v1/v2 storage, fixture-byte, golden-file, command-name, or flag/default change.

## 11. Out of scope

This phase does not enable alternate-delimiter structured compression, change
PlannerPolicyV1, add serialized format variants, add a public Rust API, add a
CLI delimiter override, infer headers, normalize values, add a JSON error
envelope, or make performance claims.
