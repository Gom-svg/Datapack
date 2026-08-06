# RFC-004 — Delimited Data Engine

- Status: Phase 5 complete; all mandatory integrated quality gates passed
- Date: 2026-08-06
- Baseline: `rfc-004-bounded-analysis`
- Related: [RFC-001B](RFC-001B-compatible-analysis-core.md),
  [RFC-003](RFC-003-hard-bounded-analysis.md)

## 1. Decision

Phase 5 introduces one crate-private, byte-oriented engine for delimited
lexical structure. The engine recognizes comma, semicolon, tab, and pipe,
tracks logical records across quoted newlines, preserves raw byte ranges, and
enforces explicit byte, logical-record, record-count, field-count, and total
field limits.

This phase shares lexical mechanics without pretending that the repository's
three established consumers have identical policy:

```text
crate-private delimited module
  -> bounded logical-record scanner
       -> DCSV01 logical-record compatibility adapter
       -> canonical quote-aware detector (not selected by a CLI command in Phase 5)
  -> standalone PlannerPolicyV1 physical-record compatibility splitter

unchanged public metadata analyzer
  -> historical lossy/line-oriented implementation
```

The engine is `pub(crate)`. It is not a new public Rust API and none of its
types participate in serialized metadata.

## 2. Existing authorities

### 2.1 PlannerPolicyV1 analysis

`analysis::engine` reads bounded physical lines, requires UTF-8 and a raw comma
in the prefix, treats the first physical record as a header, skips blank data
lines, and uses comma-only quote toggling within each line. A quoted multiline
logical record is rejected. These semantics remain the compatibility
authority for legacy analyze text, JSON V1's existing comma cases, non-chunked
planning, and benchmark planning.

Only field-boundary mechanics may move behind an exact compatibility adapter.
The Phase 4 reader, limits, coverage, header policy, errors, facts,
`PlannerFeaturesV1`, and `PlannerPolicyV1` remain unchanged.

### 2.2 Public metadata analysis

`analysis::analyze_bytes` and `formats::csv` are public. Their current behavior
uses lossy UTF-8, raw delimiter counts over physical lines, heuristic headers,
line splitting, and missing-cell padding. Their types are embedded in the
frozen v1 metadata graph.

Phase 5 does not redirect or reshape this API. It does not add fields or enum
variants to `CsvAnalysis`, `NewlineStyle`, `FileType`, `PayloadKind`, or any
other serialized type.

### 2.3 DCSV01 columnar codec

The v1 columnar payload already stores one delimiter byte and accepts comma,
semicolon, tab, and pipe when decoding. Its private parser is byte-oriented,
keeps raw lexical field bytes, supports LF/CRLF, escaped quotes, and quoted
multiline fields, rejects bare CR/mixed record newlines/unstable width, and
preserves the final-newline state.

Its quote grammar is intentionally compatibility-permissive: a quote opens
only at field start, bytes after a closing quote are accepted, and a quote in
an unquoted field is ordinary data. The compatibility adapter must reproduce
that grammar exactly. The historical raw-count delimiter detector, public
`CsvSafetyScanner` result variants, DCSV01 writer/decoder, and wire layout stay
unchanged.

## 3. Internal lexical model

The logical-scanner dialect contains only the lexical choices needed by that
scanner:

- delimiter: comma, semicolon, tab, or pipe;
- quote policy: disabled or DCSV01-compatible logical quoting;
- record-newline policy: observe LF/CRLF or require a consistent style.

PlannerPolicyV1-compatible physical quote toggling remains a separate
compatibility splitter because it operates on already bounded physical
records. It is deliberately not a `DelimitedDialect` variant.

The quote byte remains ASCII `"`; no alternate quote character is justified by
current inputs. Header mode is consumer policy, not lexical dialect state.
Newline style is an internal observed fact and does not reuse or extend the
serialized public `csv::NewlineStyle` enum.

Fields are byte ranges into caller-owned input. The scanner never unescapes,
trims, transcodes, or normalizes them. Quotation marks, doubled quotes,
embedded newlines, spaces, leading zeroes, empty values, and invalid UTF-8 are
therefore preserved as source bytes.

## 4. Explicit limits

Every scan receives limits for:

- maximum input bytes;
- maximum logical-record bytes;
- maximum records;
- maximum fields in one record;
- maximum fields across all retained records.

Limit arithmetic and offsets are checked. Collection growth uses fallible
reservation. A limit outcome is distinct from malformed syntax and never
emits an incomplete logical record as complete.

The DCSV01 compatibility adapter may derive limits from its already resident
full input so that Phase 5 does not silently reject inputs previously accepted
by the codec. That adapter is finite but is not a source-size-independent
memory guarantee. The existing whole-file v1 encoder, dictionary materializer,
and validation copy remain observed debt; this RFC makes no RSS or end-to-end
compression-memory claim.

## 5. Logical record and quote behavior

The canonical/DCSV01 logical scanner:

- recognizes a quote only at the beginning of a field;
- treats `""` inside a quoted field as an escaped pair without modifying it;
- ignores candidate delimiters and LF/CRLF while quoted;
- emits LF and CRLF logical-record terminators;
- reports bare CR, unterminated quotes, and configured limit outcomes;
- distinguishes a final newline from an unterminated final record;
- preserves empty leading, middle, and trailing fields;
- does not consume a header or require stable width.

Stable-width checking and header interpretation remain adapters/policy. The
DCSV01 adapter continues to reject mixed record newline styles. The canonical
scanner can observe mixed styles so a future analysis adapter can report the
fact rather than silently normalize it.

## 6. Detection

The canonical detector evaluates candidates in the stable order comma,
semicolon, tab, pipe. Each candidate has independent quote, field-start,
logical-record, width, and limit state. Evidence comes only from that
candidate's separators outside its quoted fields. It returns one of:

- detected dialect;
- ambiguous ordered candidates;
- undetected;
- malformed or limited scan.

It first prefers a consistent record shape, then coherent quoted fields,
modal-record support, separator count, and modal width. All evidence is
deterministic and integer-valued; it emits no confidence percentage. Equal
evidence is ambiguous rather than being broken by candidate order. A
delimiter appearing only inside a quoted field contributes no evidence.

A malformed or limited candidate is not allowed to poison an independently
valid dialect when it has no delimiter evidence or its observed widths can no
longer match. Conversely, a failed candidate whose observed shape could still
match remains an explicit malformed/limited result instead of being silently
reinterpreted. Allocation and counter failures are always fatal. Width
histograms are indexed by bounded observed field count rather than searched
linearly.

Phase 5 does not route any legacy command, public metadata helper, or DCSV01
encoder selection through this detector. Phase 6 is the designated analysis
integration point. The existing public/raw detector therefore retains its
historical later-candidate tie behavior during this phase.

## 7. Compatibility adapters

### 7.1 Planner physical splitter

The planner adapter receives one already bounded UTF-8 physical record and
returns byte-identical field slices. It preserves comma-only separation,
toggle-anywhere quote behavior, doubled-quote handling, unterminated-quote
error text, field counting, and retention limits. It does not change physical
framing or permit multiline records.

### 7.2 DCSV01 logical parser

The codec adapter retains scanner-owned raw ranges and exposes their
caller-owned byte slices to the existing column encoder. It preserves the
legacy delimiter selection, quote grammar, newline/error rules, stable-width
decision, and final newline. The frozen decoder and serialized archive graph
are not modified.

Before replacing private codec parsing, tests must prove that the frozen
columnar source still generates the exact decompressed DCSV01 payload stored in
the frozen archive, in addition to ordinary decode/restore tests.

The frozen compatibility regression compares that payload byte-for-byte.
Focused comma, semicolon, tab, and pipe tests separately prove exact
restoration; they do not independently baseline prior encoded payload bytes.

## 8. Invariants

Phase 5 must keep all of the following true:

1. `.dpack` v1/v2 representations and readers/writers are unchanged.
2. Frozen fixture and legacy analyze golden bytes are unchanged.
3. Normal PlannerPolicyV1 facts, plans, reasons, and deterministic v1 output
   are unchanged.
4. Public `analysis::analyze_bytes` and `formats::csv` signatures/results are
   unchanged.
5. The frozen v1 columnar corpus reproduces its stored DCSV01 payload
   byte-for-byte, and focused DCSV01 corpora restore their original input bytes
   exactly.
6. Every parser result preserves exact user bytes; there is no semantic
   normalization.
7. Canonical ambiguity and limits are explicit and deterministic.
8. No new dependency, public API, CLI option, or format capability is exposed.
9. RawZstd remains available for inputs that cannot safely use a structured
   path.

## 9. Verification matrix

The private-engine matrix covers:

- comma, semicolon, tab, and pipe;
- quoted delimiters and decoy candidate delimiters;
- escaped quotes;
- LF and CRLF logical records;
- multiline quoted fields;
- empty fields and blank records;
- UTF-8 and invalid UTF-8 byte preservation;
- final newline and no final newline;
- headerless records without parser-level header consumption;
- stable and inconsistent widths;
- ambiguous and absent delimiter evidence;
- candidate-local quote positions and malformed-candidate precedence;
- candidate-local limit failures that must not mask a valid dialect;
- bare CR, mixed newlines, unterminated quotes, and compatibility-permissive
  quote cases;
- exact and exceeded byte, record, field, and logical-record limits;
- scanner state transitions across small input chunks.

Protected regressions remain the nine legacy analyze goldens, Analysis JSON
V1 tests, public analysis API characterization, PlannerPolicyV1 divergence
tests, DCSV01 unit tests, frozen v1/v2 fixtures, round-trip, security, and
deterministic-output tests.

## 10. Out of scope

Phase 5 does not enable alternate delimiters in `datapack analyze`, change
Analysis JSON V1 dialect output, enable new structured compression routes,
modify planner/encoder execution, or alter any CLI name/default. Those actions
belong to later authorized phases.

## 11. Implemented and certified state

The landed implementation consists of:

- `formats::delimited`, a crate-private byte scanner, canonical detector, and
  PlannerPolicyV1 physical-record compatibility splitter;
- an exact planner splitter adapter in `analysis::engine`;
- a DCSV01 private-parser adapter retaining raw byte ranges;
- compatibility characterization for the unchanged public delimiter detector;
- a frozen-corpus regression comparing newly encoded DCSV01 payload bytes with
  the payload stored in the frozen v1 archive.

Certification used Rust and Cargo 1.85.0 with the Linux-side shared target
directory. Actual integrated results were:

- `cargo fmt --check`: pass;
- `cargo check`: pass;
- `cargo test`: 261 passed, 0 failed, 0 ignored;
- `cargo clippy --all-targets --all-features -- -D warnings`: pass;
- frozen compatibility fixtures: 4/4;
- legacy analyze goldens: 9/9;
- round-trip: 31/31;
- security hardening: 69/69;
- PlannerPolicyV1 characterization: 32/32;
- Analysis JSON V1: 9/9;
- bounded analysis: 6/6;
- public legacy analysis API: 5/5;
- delimited engine: 20/20;
- DCSV01 adapter/codec: 22/22.

The final protected-surface review found no changes to Cargo manifests, the
serialized metadata graph, v1/v2 storage readers or writers, frozen fixture or
golden bytes, CLI names/options/defaults, compression defaults,
PlannerPolicyV1, or benchmark methodology. No dependency was added.
