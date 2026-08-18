# RFC-007 — `.dpack` v3 structured-chunk design

- Status: Phase 17 design complete; implementation is not authorized
- Date: 2026-08-17
- Baseline: `rfc-016-performance-regression-suite`
- Scope: future container, planning, safety, compatibility, and acceptance design
- Related: [Architecture](../../ARCHITECTURE.md),
  [Delimited Data Engine](RFC-004-delimited-data-engine.md),
  [Planner/Encoder Contract](RFC-005-planner-encoder-contract.md), and
  [Performance Regression Suite](RFC-006-performance-regression-suite.md)

## 1. Decision and present-state boundary

DataPack v3 will be a new, explicitly versioned container for bounded,
record-aware structured compression. It will not reinterpret v1 metadata,
DCSV01, or the v2 chunk table. A future v3 encoder may choose a structured
strategy independently for each column block in each stripe, while retaining
a chunked RawZstd representation as the universal fallback.

This RFC is a design artifact only. At this revision:

- `CURRENT_VERSION` remains `1` and `CHUNKED_VERSION` remains `2`;
- there is no v3 Rust constant, reader, writer, validator, request option,
  command-line flag, Python surface, fixture, or archive;
- the `.dpack` v1 and v2 byte representations and dispatch rules are unchanged;
- no strategy identifier described here is accepted from an archive; and
- no compression, ratio, memory, or throughput improvement is claimed.

The first implementation change must receive separate authorization and its
own compatibility/security review. Code must not land merely because this
document assigns a proposed layout or identifier.

## 2. Goals

The v3 design has seven goals:

1. Preserve the complete original byte sequence, including lexical spelling,
   delimiters, quotes, escaped quotes, newlines, whitespace, empty fields, and
   final-newline state.
2. Apply structured transforms to large delimited inputs without requiring a
   whole-file in-memory document or global dictionary.
3. Make every emitted strategy explicit so the decoder never runs a planner,
   guesses a schema, or infers how bytes were encoded.
4. Bound parser state, dictionaries, queues, tables, decompression output, and
   every archive-controlled allocation.
5. Isolate corruption with stored-payload, stripe, control-plane, and global
   SHA-256 evidence while preserving transactional output.
6. Keep archive bytes hardware-neutral and decodable by a CPU-only reference
   path, regardless of which future execution backend created them.
7. Establish deterministic boundaries, ordering, and planning decisions for a
   fixed input, option set, DataPack build, and codec implementation.

## 3. Non-goals

V3 is not authorization for:

- changing or removing any v1/v2 reader, writer, fixture, mode, default, or
  validation guarantee;
- implementing v3 during this design phase;
- semantic normalization of numbers, dates, Unicode, line endings, or CSV;
- a query engine, database table format, schema registry, or random-access API;
- GPU-only archives or archive semantics that depend on a particular CPU;
- encryption, signatures, authentication, deduplication, erasure coding, or
  damaged-archive repair;
- cloud, SaaS, Desktop, telemetry, or an alternate Python compression core;
- assigning executable Delta or BitPacking semantics before exact lexical
  reconstruction is proven; or
- timing-based CI failure thresholds.

## 4. Compatibility and version dispatch

The five-byte `DPACK` magic remains the family identifier. The following
little-endian `u16` remains the major archive version. A future version-aware
dispatcher must treat the versions as separate formats:

| Version | Existing or proposed meaning | Compatibility rule |
| ---:| --- | --- |
| 1 | established bincode metadata plus one RawZstd or DCSV01 payload | frozen reader and writer remain unchanged |
| 2 | established fixed-header, fixed-table chunked RawZstd | frozen reader and writer remain unchanged |
| 3 | proposed manifest plus RawZstd or structured stripes | new reader and writer required before use |

An old DataPack build must continue to reject version 3 as unsupported. It must
not attempt a v2 parse. A future v3-capable build must keep the current v1 and
v2 dispatch paths rather than converting old archives in place. Reading an old
archive never upgrades it, and creating a v3 archive never changes the source.

The `.dpack` extension remains a container-family extension, not a version
claim. A future inspection or validation result must report the numeric archive
version and effective mode explicitly.

V3 does not reuse the v1 bincode graph or interpret the v2 archive-mode and
chunk-mode bytes. Its archive kinds, stripe kinds, transforms, codecs, flags,
and feature bits form independent registries. Unknown required features,
archive kinds, transforms, or codecs are errors; they never trigger a guessed
fallback during decoding.

## 5. Input model and archive-wide fallback

### 5.1 Structured eligibility

The initial structured design is limited to the established canonical
single-byte delimiters: comma, semicolon, tab, and pipe. A structured source
must have:

- one uniquely detected delimiter;
- DCSV01-compatible quote behavior;
- complete logical records;
- a stable positive field count;
- one record-terminator style, LF or CRLF; and
- all parser, record, field, cardinality, and memory limits satisfied.

The header, if policy identifies one, remains an ordinary first record in the
wire representation. V3 stores no inferred semantic column types and does not
need to decide whether that first record is a header to reconstruct the bytes.

Ambiguous, malformed, unsupported, unstable-width, mixed-record-newline, or
hard-limited input is not coerced into a structured interpretation. It uses the
v3 chunked RawZstd archive kind when the caller explicitly requests v3, or the
existing v1/v2 selection when no future v3 request was made.

### 5.2 Bounded two-pass contract

Structured v3 uses two bounded sequential source passes:

1. The facts/planning pass validates the complete lexical structure, computes
   source size and SHA-256, chooses record-aware stripe boundaries, and emits a
   bounded executable plan.
2. The encoding pass reopens or rewinds the source, executes that exact plan,
   and independently recomputes source size and SHA-256.

The second identity must equal the first before an archive is committed. A
changed source produces a typed `input_changed` failure; it does not create an
archive from two snapshots. The future implementation must document how it
handles non-seekable inputs before exposing them. This RFC does not promise
single-pass structured creation.

If complete facts do not authorize structured execution, the writer starts a
v3 RawZstd plan instead. If execution later proves that the structured plan
cannot be honored, the incomplete temporary archive is discarded and the
operation may restart as v3 RawZstd only from a newly verified complete source
pass. It must never silently change one planned column transform while keeping
the rest of the structured candidate.

## 6. Record-aware stripes

A stripe is the independently verified unit corresponding to one contiguous
range of original bytes. Structured stripe boundaries occur only after a
complete logical record terminator. They never split:

- a field;
- a quoted multiline field;
- a doubled-quote escape pair; or
- the two bytes of a CRLF record terminator.

The target stripe size is a target, not permission to split a record. One
logical record larger than the target forms one oversize stripe if it remains
within the configured maximum logical-record size. A record beyond that hard
limit makes the input ineligible for structured v3 and invokes the archive-wide
RawZstd decision.

Structured eligibility requires stable width, so every column block in a
stripe has exactly `record_count` values. A stripe may use either:

- `columnar`, with one explicit block for every column; or
- `raw`, with one block containing the exact contiguous stripe bytes.

A planner may select a raw stripe when columnar representation is valid but
not beneficial. It may not select raw to conceal a parser error. Raw archive
stripes use fixed byte targets and are not required to find record boundaries.

Initial structured dictionaries are stripe-local and column-local. No global
or cross-stripe dictionary is part of the initial design. This keeps memory
bounded and permits independent stripe verification and parallel work.

## 7. Exact lexical reconstruction

The shared delimited engine already models fields as raw byte ranges. V3
column values use those exact lexical field bytes, including opening and
closing quote bytes, doubled quotes, embedded newlines, leading zeros, signs,
spaces, invalid UTF-8, and quoted-empty spelling. Values are not unescaped,
trimmed, parsed, normalized, or transcoded before a lossless transform.

For a columnar stripe, reconstruction interleaves values by record and column,
inserting the one stored delimiter between fields. It inserts the stored LF or
CRLF between logical records and after every non-final stripe. The archive-wide
final-newline flag determines whether the final record has a terminator.

The reconstructed stripe must match all of the following before its bytes are
accepted:

- declared original offset and size;
- declared logical record and value counts;
- stored stripe SHA-256; and
- ordered coverage relative to adjacent stripes.

The complete reconstructed sequence must then match the declared original
size and global SHA-256. This proof, rather than the semantic meaning of a
field, is the source of truth.

## 8. Proposed v3 physical layout

The physical layout is a fixed prelude, a bounded manifest, and block payloads:

```text
+-----------------------+  offset 0
| fixed v3 header       |  proposed 176 bytes
+-----------------------+
| stripe table          |  stripe_count * 96 bytes
+-----------------------+
| block table           |  block_count * 96 bytes
+-----------------------+
| block payload 0       |
+-----------------------+
| block payload 1       |
+-----------------------+
| ...                   |
+-----------------------+  exact declared archive_size; no trailing bytes
```

All integers are unsigned little-endian unless explicitly signed. Every
reserved byte must be zero when written and must be rejected if nonzero by the
initial reader. Offsets are absolute archive offsets. Counts, multiplication,
addition, and platform conversions require checked arithmetic.

### 8.1 Fixed header

The proposed initial header is 176 bytes:

| Offset | Size | Field |
| ---:| ---:| --- |
| 0 | 5 | ASCII `DPACK` |
| 5 | 2 | major version, exactly `3` |
| 7 | 1 | archive kind: raw chunked or structured delimited |
| 8 | 4 | header length, initially `176` |
| 12 | 4 | flags; initially zero |
| 16 | 8 | required-feature bitset; initially zero |
| 24 | 8 | exact complete archive size |
| 32 | 8 | exact original size |
| 40 | 32 | SHA-256 of complete original bytes |
| 72 | 32 | control SHA-256 |
| 104 | 8 | manifest offset, initially equal to header length |
| 112 | 8 | manifest size |
| 120 | 8 | stripe count |
| 128 | 8 | block count |
| 136 | 8 | target uncompressed stripe size |
| 144 | 8 | total logical record count; zero for raw archives |
| 152 | 4 | structured column count; zero for raw archives |
| 156 | 1 | delimiter byte; zero for raw archives |
| 157 | 1 | record-newline code: none, LF, or CRLF |
| 158 | 1 | final-newline flag |
| 159 | 17 | reserved zero bytes |

The control SHA-256 covers the complete `header_length` bytes with the control
digest field zeroed, followed by the exact manifest bytes. It detects changes
to routing metadata and tables but is not a signature or proof of authorship.

A future header extension requires an assigned required-feature bit and a
separate compatibility decision. An initial reader rejects nonzero unknown
feature bits rather than skipping semantics it does not understand.

### 8.2 Stripe entry

Each proposed 96-byte stripe entry contains:

| Offset | Size | Field |
| ---:| ---:| --- |
| 0 | 8 | dense stripe ID |
| 8 | 8 | original byte offset |
| 16 | 8 | original byte size |
| 24 | 8 | first logical record index; zero for raw archives |
| 32 | 8 | logical record count; zero for raw archives |
| 40 | 8 | first block-table index |
| 48 | 4 | block count for this stripe |
| 52 | 1 | stripe kind: raw or columnar |
| 53 | 1 | stripe flags; initially zero |
| 54 | 2 | reserved zero bytes |
| 56 | 32 | SHA-256 of exact original stripe bytes |
| 88 | 8 | reserved zero bytes |

Stripe IDs, original ranges, and logical-record ranges are dense, ordered, and
gap-free. A columnar stripe has exactly the archive column count in blocks. A
raw stripe has exactly one RawBytes block.

### 8.3 Block entry

Each proposed 96-byte block entry contains:

| Offset | Size | Field |
| ---:| ---:| --- |
| 0 | 8 | dense block ID |
| 8 | 8 | owning stripe ID |
| 16 | 4 | column index, or `u32::MAX` for RawBytes |
| 20 | 2 | transform identifier |
| 22 | 2 | payload codec identifier |
| 24 | 4 | block flags; initially zero |
| 28 | 4 | reserved zero bytes |
| 32 | 8 | value count; zero for RawBytes |
| 40 | 8 | decoded transform-stream size |
| 48 | 8 | absolute stored-payload offset |
| 56 | 8 | stored-payload size |
| 64 | 32 | SHA-256 of exact stored payload bytes |

Block IDs and payload ranges are dense, ordered, gap-free, non-overlapping,
and cover the complete payload region through the declared archive size. The
stored-payload hash is verified before codec decoding. The decoded-size limit
is enforced during decoding rather than checked only after allocation.

## 9. Initial transform and codec registry

Transform and payload compression are independent choices. A transform defines
the decoded logical stream. A codec wraps that stream for storage.

The proposed initial transform registry is:

| ID | Name | Decoded stream and invariant |
| ---:| --- | --- |
| 1 | `RawBytes` | exact contiguous stripe bytes; one block owns the stripe |
| 2 | `Plain` | `value_count` repetitions of `u64 byte_length` plus exact value bytes |
| 3 | `Dictionary` | deterministic dictionary plus exactly `value_count` codes |
| 4 | `Rle` | exact value/run pairs whose positive run lengths sum to `value_count` |

The proposed initial codec registry is:

| ID | Name | Rule |
| ---:| --- | --- |
| 0 | `None` | stored payload equals transform stream |
| 1 | `Zstd` | one standard zstd frame with exact bounded decoded size |

The first authorized v3 implementation should start with RawBytes, Plain,
Dictionary, and Zstd. RLE may be implemented only when its focused design and
lexical exactness tests are approved; assigning its proposed ID here does not
authorize an encoder to emit it.

### 9.1 Dictionary stream

A Dictionary stream contains:

1. a `u64` dictionary entry count;
2. entries in first-occurrence order, each as `u64 length` plus exact bytes;
3. a one-byte code width, limited to 1, 2, 4, or 8;
4. seven zero reserved bytes; and
5. exactly `value_count` little-endian codes of the declared width.

Every code is in range. The chosen width is the smallest width capable of
representing the entry count. Dictionary entry count, individual value size,
logical dictionary bytes, code bytes, and their sum are bounded before
retention or allocation.

### 9.2 RLE stream

The proposed RLE stream starts with a `u64` run count. Each run contains a
`u64` exact-value length, the exact value bytes, and a positive `u64` run
length. Run lengths must sum exactly to the block value count without overflow.
Adjacent equal run values are forbidden so one value sequence has one canonical
RLE representation.

### 9.3 Deferred transforms

Delta and BitPacking are research candidates, not part of the initial registry.
Parsing a value as an integer is insufficient because `1`, `01`, `+1`, `1 `,
and a quoted `"1"` are distinct source bytes. Any future numeric transform must
specify a bounded lexical side channel that reproduces each exact value. It
requires a new assigned transform ID, security review, corpus, frozen fixture,
and measured benefit before emission.

## 10. Planning and execution contract

V3 requires a new `PlannerPolicyV3`; it must not change
`PlannerPolicyV1`. The conceptual path is:

```text
bounded complete lexical facts
        |
        v
PlannerPolicyV3 proposal
        |
        v
validated executable stripe/column plan
        |
        v
v3 writer executes exact transform IDs
```

The plan records every stripe boundary, stripe kind, column transform,
dictionary limit, codec, and zstd setting. The encoder validates plan shape and
resource bounds before writing. A missing, duplicated, reordered, unsupported,
or impossible entry is an error; the encoder does not rerun policy or invent a
replacement transform.

Plan decisions use deterministic integer facts. They do not use wall-clock
timings, CPU model, GPU availability, thread completion order, filesystem
speed, or ambient system load. Execution backend and thread count may change
how work is scheduled but not stripe boundaries, transform selection,
dictionary ordering, table ordering, or payload ordering.

The initial policy compares complete candidate sizes where practical and uses
RawBytes when a structured block or stripe is not smaller after its manifest
cost. Exact selection formulas and thresholds must be frozen in the future
implementation RFC and characterized before defaults are exposed.

## 11. Reader and validator contract

A future v3 reader performs these stages in order:

1. read the fixed prelude without archive-sized allocation;
2. validate magic, version, header length, kinds, flags, feature bits, declared
   archive size, and configured output limit;
3. bound stripe/block counts and manifest memory before reading tables;
4. verify exact manifest position/size and the control SHA-256;
5. validate dense IDs, ownership, counts, original coverage, record coverage,
   block shapes, payload ranges, and exact end-of-archive;
6. for each stripe in original order, verify stored block payload hashes,
   decode each codec to its exact transform-stream limit, execute the explicit
   transforms, and reconstruct the stripe;
7. verify stripe size and SHA-256 before making its bytes eligible for ordered
   output; and
8. verify final restored size and global SHA-256 before committing output.

Parallel decode may process independent stripes, but output remains ordered and
bounded by an explicit maximum in-flight count. A later stripe cannot commit
ahead of a failed or incomplete earlier stripe.

Validation uses the same parser and transform decoders as decompression. It
does not maintain a validation-only interpretation. Validation always verifies
all hashes; an explicit future speed-testing option must never alter validation
or be described as integrity evidence.

No damaged archive commits a final restored file. Partial recovery, if ever
offered, must be a distinct operation with an explicit non-equivalence result;
it is not normal decompression.

## 12. Resource and malicious-input requirements

Before implementation, named hard ceilings and caller-configurable lower
limits must exist for:

- restored output bytes;
- header and manifest bytes;
- stripe and block counts;
- column count and logical records per stripe;
- target and maximum stripe bytes;
- maximum logical-record and field bytes;
- values per block;
- dictionary entries and logical dictionary bytes;
- decoded transform-stream bytes;
- compressed payload bytes;
- worker count and maximum in-flight stripes; and
- total accounted working memory.

All archive-controlled additions, multiplications, offset conversions, table
sizes, value lengths, code counts, and run sums use checked arithmetic.
Collections use fallible reservation after the corresponding limit succeeds.
Zstd decoding stops at the declared exact decoded size and rejects both short
and expanded output.

The reader rejects:

- table ranges outside the declared archive;
- overlaps, gaps, duplicated ranges, and trailing bytes;
- non-dense or out-of-order IDs;
- stripe/block ownership disagreement;
- impossible record/value counts;
- invalid delimiter/newline combinations;
- unknown transforms, codecs, flags, or required features;
- nonzero reserved bytes;
- dictionary codes outside the dictionary;
- non-canonical dictionary width or RLE runs;
- payload, stripe, control, or global hash mismatch; and
- any allocation or platform conversion that exceeds a configured bound.

SHA-256 supplies integrity detection, not authenticity. This design makes no
claim against an attacker who can replace an archive and recompute every
digest.

## 13. Transactionality and failure coordination

V3 compression and decompression use the established temporary-sibling output
model. The requested output is committed only after tables, payloads, lengths,
and hashes are finalized successfully. Existing outputs remain protected unless
explicit overwrite authority is supplied. Input/output identity is rejected
before temporary output creation.

Reader, worker, reorder-buffer, and writer errors stop admission and propagate
to the caller. Queues are bounded. A worker panic becomes an operation failure
rather than a hang or a successful partial archive. Temporary retention remains
an explicit debugging choice and must never make a partial artifact appear
valid.

## 14. Determinism and portability boundary

For the same source bytes, options, DataPack build, and codec implementation,
v3 promises deterministic:

- structured eligibility and dialect;
- stripe boundaries and IDs;
- plan and transform IDs;
- dictionary entry/code order;
- manifest ordering and payload ordering;
- hashes, offsets, and sizes; and
- final archive bytes when the selected codec itself is byte-deterministic in
  that scope.

Thread scheduling and worker completion order do not affect archive order.
Byte-identical zstd frames across zstd versions, platforms, compiler settings,
or distinct backend implementations are not promised. Compatibility means a
conforming reader executes the recorded strategy and restores identical source
bytes.

The reference format is CPU-decodable. A future GPU or SIMD encoder may emit
only the same registered transforms and standard codec payloads. Accelerator
presence cannot become a required feature or create a separate `.dpack-gpu`
format.

## 15. Future Application API, CLI, and reports

No placeholder API or CLI option is added in Phase 17. A future implementation
may extend the typed compression request with an explicit v3 format variant
only after the reader, writer, validation, fixtures, and safety gates exist.
The default must not silently change to v3 from performance observations alone.

CLI and Python remain adapters over the Rust Application API. Python must not
parse a v3 manifest, select transforms, or implement fallback independently.

Existing Analysis/Validation/Application JSON V1 contracts are not silently
reshaped for v3. If v3 facts cannot be expressed truthfully in an existing
versioned report, a new report schema is required. At minimum a future report
must distinguish archive version, archive kind, stripe kinds, transforms,
codecs, resource limits, and completed integrity checks without exposing source
field values or user-data hashes in ordinary public reports.

## 16. Verification and compatibility matrix

Before any v3 writer is considered releasable, certification must cover:

### Exact reconstruction

- comma, semicolon, tab, and pipe;
- LF and CRLF;
- final newline present and absent;
- quoted delimiters and alternate-delimiter decoys;
- doubled quotes and quoted multiline fields;
- empty, quoted-empty, and blank lexical values;
- spaces, signs, leading zeroes, UTF-8, and invalid UTF-8;
- stripe boundaries on both sides of quoted/newline edge cases;
- Plain, Dictionary, RawBytes, and every later authorized transform; and
- structured-to-raw planning decisions.

### Corruption and resource safety

- every fixed-header and table field truncated at representative boundaries;
- huge counts, sizes, and offset arithmetic overflow;
- gaps, overlaps, duplicate IDs, invalid ownership, and trailing data;
- payload, stripe, control, and global hash corruption;
- zstd short output, over-expansion, malformed frames, and trailing frame data;
- invalid dictionary widths, codes, lengths, and RLE sums;
- exact-limit acceptance and one-over rejection for every named bound;
- failure coordination with saturated queues and out-of-order workers; and
- transactional preservation of existing outputs.

### Compatibility

- all frozen v1/v2 fixture sizes and SHA-256 values unchanged;
- old v1/v2 archives readable through their existing code paths;
- v1/v2 deterministic characterization unchanged;
- an old binary rejects v3 cleanly;
- frozen v3 RawBytes, Plain, Dictionary, and multistripe fixtures from an
  independently recorded implementation baseline;
- cross-platform decode on the Rust minimum toolchain; and
- no archive fixture regeneration to make a failing reader pass.

### Fuzzing and model comparison

- header/manifest structured fuzz targets;
- transform-stream fuzz targets;
- corruption mutation from every frozen v3 fixture;
- scanner chunk-boundary fuzzing; and
- a small independent reference reconstruction model used only in tests to
  compare outputs, not a second production decoder.

## 17. Performance evidence policy

V3 adoption requires the Phase 16 evidence split:

```text
correctness regression != observational performance
```

Corruption, reconstruction, compatibility, determinism, and resource-limit
failures may fail CI. Wall-clock compression and decompression do not receive
uncontrolled pass/fail thresholds.

Representative observations must compare, where applicable:

- v1 structured DCSV01;
- v2 chunked RawZstd;
- v3 RawBytes and v3 structured strategies;
- standalone zstd; and
- repetitive, realistic, high-cardinality, random/low-redundancy, multiline,
  and wide structured datasets.

Dataset bytes, seeds, build, dependency lockfile, validation level, cache
policy, filesystem class, operating environment, run count, threads, stripe or
chunk size, in-flight bound, and memory policy must be recorded. WSL `/mnt/c`,
WSL Linux-native storage, native Linux, and native Windows remain distinct
evidence populations. A better ratio on one corpus or a faster run on one
machine is insufficient to change the default.

## 18. Staged future implementation plan

This sequence is a dependency order, not authorization to begin:

1. Freeze the byte-layout specification, registries, limits, and conformance
   vectors after a dedicated security review.
2. Implement bounded manifest primitives and read-only parser tests without
   changing normal version dispatch.
3. Add v3 RawBytes writer/reader/validator behind an explicit unstable test
   surface; freeze raw single- and multistripe fixtures.
4. Add record-aware Plain stripes and exact lexical reconstruction; freeze
   delimiter/newline/quote fixtures.
5. Add bounded Dictionary blocks and deterministic selection; freeze mixed
   Plain/Dictionary/RawBytes fixtures.
6. Add transactional parallel execution, Application API results, validation
   reporting, and explicit CLI opt-in.
7. Complete fuzzing, resource certification, cross-platform fixture decode,
   Python delegation tests, and representative observational measurements.
8. Consider default changes or later transforms only in separate decisions.

Every stage retains green v1/v2 compatibility gates. A stage that requires
changing v1/v2 semantics, unbounded allocation, an uncontrolled timing gate, or
a hardware-specific archive stops for redesign.

## 19. Deferred decisions

The following are deliberately outside the initial v3 release rather than
ambiguities in its core reconstruction model:

- Delta, BitPacking, and other semantic transforms;
- global or cross-stripe dictionaries;
- partial-recovery UX;
- seek/range extraction APIs;
- non-seekable structured creation;
- encryption, signatures, and authenticated provenance;
- alternate payload codecs; and
- default-format migration.

Each requires a separately registered feature or transform and cannot change
the meaning of an already emitted v3 identifier.

## 20. Phase 17 certification statement

Phase 17 changes documentation only. It does not implement v3, reserve a Rust
constant, alter Cargo dependencies, change the public API or CLI, or modify any
fixture. The design makes byte-exact reconstruction, bounded resources,
transactional output, explicit strategies, and frozen v1/v2 compatibility
prerequisites rather than assumptions.
