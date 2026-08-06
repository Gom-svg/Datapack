# RFC-005A — Archive Validation

- Status: Implemented
- Date: 2026-08-06
- Baseline: `rfc-008-planner-encoder-contract`
- Related: [Validation JSON V1](../reference/VALIDATION_JSON_V1.md)

## 1. Decision

Phase 9 adds a read-only archive-validation command:

```text
datapack validate ARCHIVE
datapack validate ARCHIVE --against ORIGINAL
datapack validate ARCHIVE --json
datapack validate ARCHIVE --json --pretty
datapack validate ARCHIVE --max-output-mb 8192 --max-chunks 10000 --max-memory-mb 512
```

`--pretty` requires `--json`. The default output is a human-readable report.
JSON output uses the versioned `ValidationReportV1` DTO documented in
`docs/reference/VALIDATION_JSON_V1.md`.

Validation also accepts the existing decompression-style safety limits:

- `--max-output-mb` rejects a larger declared restored size before payload
  validation;
- `--max-chunks` lowers the internal v2 chunk-count ceiling; and
- `--max-memory-mb` bounds v1 header/metadata allocation, the v1 columnar
  working-memory estimate, and the v2 working-memory estimate; it defaults to
  512 MiB.

Limit values use MiB (1,048,576 bytes) and must be greater than zero. The
memory value is a conservative validation bound, not a measurement of process
RSS or zstd allocator overhead.

Validation is a separate operation from decompression. It reconstructs and
checks archive content without creating a restored file, a sibling `.partial`
file, or another temporary output. The archive and an optional `--against`
source are opened read-only.

The command exits successfully only when every check available for that
archive version passes and, when requested, the complete reconstructed
size/SHA-256 identity matches the `--against` source. A structurally invalid
or corrupt archive and an `--against` mismatch are validation failures.
Command-line parsing, invalid limit values, and operational failures such as
an unreadable path remain ordinary CLI errors.

## 2. Internal boundary

The CLI command is an adapter over a crate-private validation service. The
service owns archive inspection and produces a validation result; CLI code
owns text and JSON presentation. Clap types, terminal output, and serialized
report DTOs are not used by the storage readers.

The DTO is not a serialization of `DpackMetadata`, `ChunkedArchiveInfo`, or a
storage error. It converts those internal values to explicit V1 strings and
status values. This prevents changes to internal Rust types from silently
changing the machine-readable contract and does not create the public Rust
application API reserved for Phase 12.

The implementation reuses the existing v1 reader, DCSV01 decoder, zstd
backend, v2 header/table reader, and v2 chunk verification rules. It does not
introduce a second archive parser or a validation-only interpretation of the
wire format.

## 3. Version-specific guarantees

### 3.1 `.dpack` v1

For v1, validation checks:

- the `DPACK` magic and version;
- the outer file-type byte;
- the bounded metadata length and complete metadata bytes;
- bincode decoding with trailing metadata bytes rejected;
- agreement between outer and serialized metadata fields;
- the frozen extension policy;
- the payload mode;
- zstd payload decompression;
- DCSV01 payload structure and reconstruction when the payload is columnar;
- successful payload decoding; and
- the declared restored byte length.

V1 stores neither per-chunk SHA-256 values nor a global hash of the original
file. It also has no authenticated payload-end field independent of its zstd
payload. The per-chunk hash, global hash, and trailing-data checks are reported
as `not_available`, never as `passed`. A valid v1 report therefore means that
the container, metadata, payload, codec structure, and restored length passed
the checks the format can support. It is not the same guarantee as v2 hash
verification.

When `--against ORIGINAL` is supplied, DataPack additionally compares the
complete reconstructed size and SHA-256 with the supplied source's complete
size and SHA-256. A match adds the end-to-end identity check that v1 cannot
provide from its archive alone.

### 3.2 `.dpack` v2

For v2, validation checks:

- the `DPACK` magic, version, and RawZstd archive mode;
- the fixed header and declared output size;
- the internal chunk-count ceiling;
- the complete fixed-width chunk table;
- ordered chunk IDs and original-byte coverage;
- compression modes and checked ranges;
- absence of compressed-range overlap, unreferenced gaps, and trailing bytes;
- successful bounded decompression of every chunk;
- every restored chunk length;
- every stored per-chunk SHA-256 value;
- the complete restored length; and
- the stored global SHA-256 value.

The global digest is accumulated in validated chunk order, so it covers the
original byte sequence represented by the complete table. Validation never
uses the decompression command's unsafe `--no-verify` behavior.

`--against ORIGINAL` performs an additional complete-source comparison. It
does not replace either the per-chunk or global v2 checks.

## 4. Report semantics

Both renderers distinguish five check states:

| Status | Meaning |
|---|---|
| `passed` | The check was applicable and completed successfully. |
| `failed` | The applicable check completed with a validation failure. |
| `not_available` | The archive version does not store or define this guarantee. |
| `not_applicable` | The archive version has no such structure or validation stage. |
| `not_completed` | An earlier failure prevented the check from running. |

An absent format guarantee is not a warning and is not promoted to success.
In particular, v1 per-chunk hash, global hash, and trailing-data statuses are
`not_available`.

Machine-readable failures use stable diagnostic codes. Human-readable
messages provide context, but consumers must branch on `code`, not match
message text. Reports contain no archive path, source path, raw field value,
sample row, reconstructed content, or user-data hash.

The report contains no elapsed-time or throughput field. Repeated validation
of unchanged bytes therefore has deterministic report content.

## 5. `--against` behavior

The optional source is read without modification. Validation compares the
complete reconstructed size and SHA-256 with the complete source size and
SHA-256; it does not infer equivalence from a sampled prefix, path, timestamp,
extension, or metadata file type.

The `against.status` value is:

- `not_requested` when the flag is absent;
- `matched` when complete identity comparison succeeds;
- `mismatched` when the complete comparison fails; or
- `not_completed` when an earlier archive failure prevents comparison.

An `--against` mismatch makes the root `valid` value false and the command
unsuccessful even if every format-native archive check passed.

## 6. Safety and non-mutation

Validation performs no archive rewrite, repair, normalization, or metadata
upgrade. It never writes a restored output. A single open archive handle is
used from version dispatch through metadata and payload validation, preventing
different path snapshots from being mixed into one report. The implementation
uses checked offsets and sizes, existing metadata and chunk-count ceilings, the
default 512 MiB validation bound, fallible allocation, exact decompression
limits, and the established DCSV01 output bound. The configured memory limit
is applied before v1 metadata allocation. Raw v1 payload validation then
streams with a fixed hash buffer. V1 columnar validation first checks a
conservative estimate containing two archive-sized buffers, the bounded DCSV01
expansion, and restored bytes. V2 applies its table-plus-largest-chunk
working-memory estimate before payload validation and then holds at most one
compressed and one restored chunk.

Raw v1 restoration is consumed by a counting SHA-256 sink rather than
materialized as an output file. V2 processes verified chunks without
committing restored bytes. The existing v1 columnar path remains an in-memory
codec and is bounded using the declared original size and the established
DCSV01 expansion allowance. Validation does not claim fixed RSS or constant
memory for that path.

No DataPack-owned unsafe code is introduced. Invalid user-controlled offsets,
lengths, modes, hashes, and compressed bytes return errors rather than
panicking.

## 7. Compatibility

Phase 9 does not change:

- the v1 serialized metadata graph;
- v1 header or payload bytes;
- the DCSV01 representation;
- the v2 fixed header, chunk table, or payload layout;
- v1 or v2 compression defaults;
- decompression behavior under existing command names;
- frozen compatibility fixtures;
- legacy analyze output or goldens;
- planner policy or encoder execution; or
- the Cargo dependency graph.

The command only reads archives using the frozen readers and exposes the
result through a new presentation boundary.

## 8. Verification scope

Focused validation regressions cover valid frozen v1 RawZstd, v1 DCSV01, and
v2 archives; compact and pretty JSON equivalence; `--against` match and
mismatch; bad magic and version; truncated headers, v1 metadata, and payloads;
invalid v2 modes and offsets; restored-length, per-chunk-hash, and global-hash
corruption; v2 trailing bytes; configured output, metadata-memory, columnar
memory, and chunk-count limits; stable exit behavior; and absence of restored
or temporary output artifacts. Existing storage and security suites continue
to cover the deeper range and declared-size matrix used by the same readers.

These tests establish correctness and compatibility behavior. They make no
performance or memory-throughput claim.

## 9. Limitations

- V1 has no archive-native content hash. Without `--against`, successful v1
  validation cannot prove identity with an external original.
- V1 is not described as having chunks, chunk recovery, or per-chunk
  verification.
- Validation reports whether the archive is valid under the implemented v1 or
  v2 rules; it does not repair damaged archives.
- `--against` requires access to the complete original file and is a full-file
  operation.
- Validation does not create a restored artifact for later use. Use
  `datapack decompress` when restored output is required.
