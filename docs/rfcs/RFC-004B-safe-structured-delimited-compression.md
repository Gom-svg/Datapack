# RFC-004B — Safe Structured Compression for Delimited Text

- Status: Implemented
- Date: 2026-08-06
- Baseline: `rfc-006-delimited-analysis`
- Related: [RFC-004](RFC-004-delimited-data-engine.md) and
  [RFC-004A](RFC-004A-delimited-analysis.md)

## 1. Decision

Non-chunked v1 `datapack compress` may use the existing DCSV01 structured
payload for uniquely detected comma, semicolon, tab, and pipe input. This does
not introduce an archive variant. The frozen
`PayloadKind::CsvColumnarDictionary` value remains the umbrella identifier for
the existing DCSV01 payload, and DCSV01's existing delimiter byte identifies
the actual dialect.

The Phase 6 bounded analysis dispatcher supplies the delimiter. A complete
analysis passes its unchanged facts through `PlannerPolicyV1`; a hard-limited,
ambiguous, unsupported, or malformed analysis uses streaming RawZstd. Before a
structured candidate is written, full candidate bytes must be uniquely
detected as the same delimiter. The explicit-delimiter DCSV01 encoder then
decodes its candidate under the original-size output limit and returns it only
when the restored bytes equal the input exactly. Any detection, parse, or
reconstruction failure selects RawZstd.

## 2. Frozen representation

DCSV01 already has this prefix:

| Offset | Value |
|---:|---|
| `0..6` | ASCII `DCSV01` |
| `6` | delimiter byte: comma, semicolon, tab, or pipe |
| `7` | newline style: LF or CRLF |
| `8` | final-newline marker |
| `9..17` | row count (`u64`, little endian) |
| `17..21` | column count (`u32`, little endian) |

The existing decoder already accepts exactly the four supported delimiters.
The v1 outer metadata, bincode graph, zstd settings, payload-kind discriminant,
reader, and writer are unchanged. Path-derived metadata also stays unchanged:
`.csv` maps to `FileType::Csv`, while `.tsv` and `.psv` remain
`FileType::Unknown`. No semantic file-type inference is serialized.

V2 remains the existing chunked RawZstd representation. Its archive mode and
every chunk compression mode remain `1`; Phase 7 adds no structured v2 path.

## 3. Compatibility adapter

The public `formats::csv::columnar::encode(bytes)` behavior remains the legacy
adapter: it selects a delimiter with the historical detector and then calls
the common encoder. A new crate-private entry point accepts the canonical
delimiter explicitly. The CLI and crate-private storage adapter use only that
explicit path for Phase 7 routing.

This separation is required because the historical detector counts raw
delimiter bytes, including bytes inside quotes. Canonical analysis is
quote-aware. Re-detecting at the writer could therefore choose a different
dialect even if both interpretations happened to round-trip. Phase 7 instead
requires canonical full-input agreement and writes the supplied delimiter byte.

The public `storage::encode_adaptive_archive` and existing public columnar
storage functions retain their established behavior. No new public API is
introduced before the designated application-API phase.

## 4. Planner and execution behavior

`PlannerPolicyV1` is not changed. Complete facts for all four supported
dialects now retain the policy result instead of the Phase 6 capability
override. Hard limitations still override the effective plan to RawZstd.

For default/fast execution, a policy-selected structured plan executes only
after full-input dialect agreement and the codec's exact reconstruction proof.
`--mode best` keeps its existing threshold behavior. `--verify-best` still
constructs both candidates and keeps the smaller complete archive. Dictionary
limits and column-plan enforcement are not changed here; that is the separate
planner/encoder execution-contract phase.

Inputs for which structured analysis cannot safely complete are archived by
the existing transactional streaming RawZstd path. RawZstd therefore remains
available for ambiguity, inconsistent width, unterminated quotes, mixed
newline styles, hard limits, codec rejection, or an unbeneficial candidate.

Benchmark remains on its certified legacy comma analysis and execution scope
in this phase. Its run count, median calculations, sampling, timing, hashing,
and `--max-input-mb` behavior are unchanged. Multi-dialect benchmarking belongs
to the later Compare/benchmark work rather than this codec-enablement change.

## 5. Exactness and safety evidence

The focused matrix covers all four delimiter bytes and verifies:

- LF and CRLF;
- final newline present and absent;
- delimiters and other delimiter candidates inside quoted fields;
- doubled quotes and multiline quoted fields;
- empty and quoted-empty fields;
- lexical spaces and leading zeroes;
- UTF-8;
- the emitted DCSV01 delimiter byte;
- exact DCSV01 decode and archive restoration;
- default, best, and verify-best v1 routing on a beneficial deterministic
  corpus;
- same-build deterministic v1 output;
- transactional RawZstd fallback for unresolved or unsafe inputs; and
- unchanged v2 RawZstd mode and exact restoration.

The implementation makes no throughput, ratio, or memory-performance claim.
The existing direct v1 structured path materializes candidate bytes and the
columnar representation in memory; Phase 7 does not present that architecture
as a streaming or fixed-memory codec.

## 6. Compatibility invariants

Certification requires all frozen v1/v2 fixtures and the nine legacy analyze
goldens to remain unchanged. It also requires the round-trip, security,
PlannerPolicyV1, shared delimited-engine, and new structured-delimiter suites.
No fixture or golden is regenerated.
