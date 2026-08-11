# DataPack Security Policy

DataPack treats every input file, `.dpack` archive, path, and option crossing
the CLI, public Rust Application API, or Python SDK boundary as untrusted.
Losslessness is the primary invariant: a restored file is valid only when its
bytes are identical to the original bytes. For v2 archives, SHA-256 is the
authoritative integrity check.

This document describes the security boundary of the local DataPack CLI, Rust
Application API, Python SDK foundation, and frozen `.dpack` v1 and v2 formats.
It does not create a new archive format.

## 1. Threat Model

### Trust boundary

| Trusted | Untrusted |
| --- | --- |
| The DataPack binary, Rust Application service, Python extension, and their shipped project code | Input files and `.dpack` archives |
| The local machine, operating system, and Rust/Python runtimes | `.dpack` headers, metadata, chunk tables, offsets, sizes, hashes, and compressed payloads |
| The local operator's decision to invoke DataPack and grant operating-system access | Paths and option values supplied through the CLI, Rust requests, or Python calls |

The primary attacker-controlled object is an archive supplied by another user
or machine. An attacker may deliberately truncate it, corrupt a zstd frame,
forge sizes or offsets, create overlapping ranges, claim an extreme output size
or chunk count, alter hashes, append data, or combine fields to trigger integer
overflow and excessive allocation. Input filenames and output paths may also be
mistyped or deliberately chosen to collide. A library or Python caller can
likewise pass hostile paths, counts, byte limits, overwrite choices, or
verification controls. The Python package is a thin adapter over the Rust
Application API and does not bypass Rust validation.

The assets DataPack protects are:

- the byte identity of restored data;
- existing files at user-selected paths;
- the availability of the local process and reasonable use of memory and disk;
- clear failure status, so an incomplete artifact is not mistaken for success;
- compatibility with existing v1 and v2 archives.

DataPack assumes the local operating system correctly implements file access,
permissions, and rename operations. A privileged local attacker, a compromised
kernel, hostile storage firmware, and time-of-check/time-of-use races caused by
another process modifying paths during an operation are outside this boundary.
Run untrusted archives in a private working directory when local path races are
a concern.

### Format-specific trust decisions

- V1 and v2 magic bytes and versions must match their existing definitions.
- V1 metadata and its payload are untrusted. V1 does not store the v2 per-chunk
  or global SHA-256 fields, so end-to-end v1 identity requires an external hash
  comparison with a trusted original.
- V2 uses a fixed 64-byte header and fixed 80-byte chunk-table entries. Version
  2 has no optional hash-presence flag: its 32-byte global SHA-256 field and each
  entry's 32-byte chunk SHA-256 field are mandatory parts of the frozen layout.
- V2 currently recognizes only its existing RawZstd archive and chunk mode.
  Unknown modes and nonzero unsupported/reserved values are malformed, not
  requests to enable a feature.
- The v2 table's zstd-level field is informational writer metadata; the decoder
  follows the standard zstd frame. All existing `i32` values remain readable
  because v2 never defined a narrower compatibility range.
- A v2 empty-file archive is explicit: original size and chunk count are zero,
  the hash is SHA-256 of the empty byte sequence, and the archive ends after its
  header. A non-empty archive must contain non-empty chunks.
- V2 payload ranges must be ordered, non-overlapping, in bounds, and cover the
  declared original byte sequence contiguously. The final compressed range must
  end exactly at the archive length. Trailing bytes are unauthenticated and are
  therefore rejected. This also makes concatenated or appended data a clean
  parse error instead of silently ignoring it.

## 2. Supported Security Guarantees

### Integrity and losslessness

DataPack never treats a hash mismatch as a warning. Normal v2 decompression
performs all of the following before committing the requested output:

1. validates the header and complete chunk table against configured limits and
   the actual archive length;
2. reads and decompresses each declared zstd frame;
3. checks the exact decompressed byte count for each chunk;
4. computes SHA-256 over the restored bytes of each chunk and compares it with
   that chunk's table entry;
5. computes SHA-256 over the full restored byte sequence in original order and
   compares it with the v2 global hash; and
6. checks the total restored size.

The per-chunk SHA-256 covers **the original, uncompressed bytes of exactly that
chunk** (option A), in the range described by its validated original offset and
size. It does not cover the compressed zstd frame or the table entry. The global
SHA-256 covers **the complete original input byte sequence**, from byte zero to
the declared original size, with no separators or transformed representation.
It is the final authority for v2 round-trip identity.

The zstd decoder independently rejects malformed or truncated zstd frames.
That structural error detection remains active even when hash verification is
disabled. It is not a substitute for SHA-256: some altered compressed streams
can still be syntactically valid and produce different bytes, which the stored
hashes detect during normal verification.

`--no-verify` is an explicitly unsafe benchmarking control. It skips v2
per-chunk and global hash comparisons only; it does not disable archive parsing,
range checks, zstd decoding, per-chunk restored-size checks, or total-size
checks. Output produced with this option has not passed DataPack's authoritative
integrity check and must not be treated as validated.

The equivalent Rust Application and Python request choice is `verify = false`;
it has the same reduced integrity meaning and does not create a separate
verification policy.

SHA-256 fields detect corruption relative to the metadata in an archive. They
do not prove who created the archive: an attacker able to rewrite an archive
can also recompute unkeyed hashes. Use a trusted external signature when
authenticity or provenance matters.

### Malformed archive handling

Archive fields are checked before use. In particular, parsers validate exact
magic and supported versions/modes, fixed header and table sizes, the internal
chunk-count ceiling, checked table-size arithmetic, platform conversions,
archive bounds, contiguous original ranges, ordered/non-overlapping compressed
ranges, exact original-size coverage, and empty-file consistency. Untrusted
metadata must pass its configured and internal ceilings before a chunk table or
payload buffer is allocated.

Malformed, truncated, corrupted, unsupported, or oversized input returns a
specific error. User-controlled input is not permitted to reach `panic!`,
`unwrap`, `expect`, `todo!`, `unimplemented!`, or unchecked indexing. Internal
invariants that remain infallible must be documented and preceded by validation.

The core library, CLI binary, and Python binding each declare
`#![forbid(unsafe_code)]`. Those product crates therefore cannot introduce an
`unsafe` block or function. This restriction does not extend into third-party
dependencies, which remain part of the supply-chain trust boundary and are
reviewed through the committed lockfiles and dependency-policy checks.

### Resource limits

Decompression supports optional defense-in-depth limits. CLI options ending in
`-mb` use MiB, where one MiB is 1,048,576 bytes:

- `--max-output-mb <n>` rejects an archive whose declared original size exceeds
  the limit, before payload decompression.
- `--max-chunks <n>` rejects an archive whose declared chunk count exceeds the
  limit, before allocating the chunk table. The internal hard ceiling of
  1,000,000 chunks applies even when this option is omitted; the option can only
  lower that ceiling.
- `--max-memory-mb <n>` rejects a configuration whose approximate lower-bound
  working-memory estimate exceeds the limit before large payload buffers are
  created. Allocator overhead and zstd context/workspace memory are not exact.

The Rust Application API and Python SDK use bytes for byte-named fields such as
`max_output_bytes`, `max_memory_bytes`, and v2 `chunk_size_bytes`; they do not
reinterpret those values as MiB. Fields that retain an `_mb` name, including
`sample_mb` and `max_input_mb`, retain MiB semantics. `max_chunks` is a count in
all interfaces and applies to the v2 chunk table, not to v1.

Compression also validates chunk size, worker count, and maximum in-flight
chunks. A configuration whose in-flight chunk bytes exceed an active memory
limit is rejected with the relevant option names. Explicit limits are optional
so an operator can select budgets appropriate to the input and host; omitting
them does not disable structural validation or the internal chunk-count
ceiling, and it does not promise that every large input will fit available
resources.

Limits reduce resource-exhaustion risk but cannot guarantee that a run will fit
available RAM or disk. Zstd workspaces, filesystem buffering, temporary output,
and other processes also consume resources. The declared original size is used
for preflight decisions, but sufficient free disk space is still the operator's
responsibility. The 1,000,000-chunk ceiling is an implementation safety bound,
not permission to use that many chunks on a resource-constrained machine.

### Output safety

Compression and decompression validate path collisions and the output parent
directory. Existing outputs are protected unless `--force` is explicitly
provided by the CLI or `overwrite` is explicitly enabled by a Rust/Python
caller. DataPack writes a temporary sibling and commits it only after the
operation and required verification succeed. Thus:

- on success, the final path contains the complete committed output;
- on failure, a pre-existing final file remains valid, or a new final path does
  not exist; and
- a partial temporary file is not presented as a successful final artifact.

Temporary files are removed after ordinary failures by default. For CLI
Compress and Decompress, `--keep-temp` maps to the Rust/Python `keep_partial`
request field; both preserve an operation-owned partial output for diagnosis.
Benchmark and Tune use their own `--keep-temp` artifact lifecycles, and the
Rust Benchmark request names that choice `keep_artifacts`. Every retained file
may contain plaintext, sampled input, or incomplete/untrusted output and must
never be mistaken for a committed production result. Filesystem and commit
errors name the affected operation and path and report whether the final output
was committed or cleaned up.

### Compatibility and format freeze

Existing v1 and v2 archives remain readable without modification. Security
validation may reject bytes that were never a valid archive, including
out-of-range metadata and unauthenticated trailing v2 data, but it must not
reinterpret valid fields or silently alter the v2 binary layout. Any security
property that needs new on-disk fields, modes, or hash coverage requires a
separate v3 proposal and compatibility review.

## 3. Non-Goals

DataPack intentionally does not provide:

- encryption or confidentiality;
- authentication, HMAC, signatures, provenance, or tamper attribution;
- DRM, anti-debugging, obfuscation, or resistance to reverse engineering;
- a sandbox for hostile code or protection from a compromised local machine;
- cloud services, telemetry, or licensing controls; or
- protection against an authorized user deliberately choosing very large
  resource limits or exhausting the destination filesystem.

DataPack does not currently implement Desktop, GPU acceleration, an adaptive
CPU/GPU scheduler, cloud upload, SaaS, telemetry, or a v3 encoder/decoder.
Those absent product capabilities do not weaken the documented v1/v2 checks.

## 4. Recommended Safe Usage

- Obtain DataPack from a trusted build and keep its dependencies updated.
- Treat archives from other people as hostile. Set `--max-output-mb`,
  `--max-chunks`, and `--max-memory-mb` to values appropriate for the machine
  before decompressing them.
- Decompress into a private directory on a filesystem with sufficient free
  space. Avoid shared or attacker-writable directories when path races matter.
- Do not select the input/archive path as the output path. Review the resolved
  output path carefully before using `--force`.
- Leave v2 verification enabled. Use `--no-verify` only for disposable
  performance measurements, then repeat without it before trusting output.
- For v1 archives, compare SHA-256 of the restored file with a trusted hash of
  the original. A successful zstd decode and size check alone are not an
  end-to-end identity proof.
- When provenance matters, verify a trusted detached signature over the entire
  archive before invoking DataPack. The embedded unkeyed hashes are not an
  authenticity mechanism.
- Delete preserved CLI `--keep-temp`, Application/Python `keep_partial`, and
  Benchmark `keep_artifacts` outputs after diagnosis. They can contain
  sensitive plaintext, sampled input, or incomplete archive data.
- Keep backups of valuable existing outputs even when using atomic commit
  semantics; storage and operating-system failures remain possible.

### Fuzzing and deterministic robustness checks

Fuzzing is separate from `cargo test`, requires nightly Rust and `cargo-fuzz`,
and is never run by the normal test suite:

```bash
rustup toolchain install nightly
cargo install cargo-fuzz
cargo +nightly fuzz run csv_roundtrip -- -max_total_time=300
cargo +nightly fuzz run v2_archive_parser -- -max_total_time=300
cargo +nightly fuzz run v2_decompress_mutated_archive -- -max_total_time=300
cargo +nightly fuzz run chunk_table_validation -- -max_total_time=300
```

The parser targets must accept valid inputs or return clean errors without
panicking or making metadata-sized allocations before validation. The mutated
decompression target also checks that a rejected archive leaves no final output.
Deterministic header, table, payload, truncation, hash, trailing-byte, and output
commit mutation tests remain part of the regular test suite.

### Local quality checks

Run `scripts/check.sh` on Bash or `scripts/check.ps1` on PowerShell. They execute
format checking, `cargo check`, tests, Clippy with warnings denied, and a
release build. Supply-chain checks can be run separately:

```bash
cargo tree
cargo tree -d
cargo install --locked cargo-audit
cargo audit
cargo install --locked cargo-deny
cargo deny check
```

### Accepted compatibility advisory

Frozen v1 metadata uses bincode 1.3.3. Replacing its serialization merely to
silence a maintenance advisory would change a protected compatibility surface.
[`deny.toml`](deny.toml) therefore contains one explicit exception for
`RUSTSEC-2025-0141`, which reports that bincode is unmaintained. This is a
**KNOWN ACCEPTED WARNING**, not a claim that the advisory is absent or fixed.

`cargo audit` remains visible and may report this advisory. The cargo-deny
exception is narrow and does not suppress unrelated new advisories. See the
[CI and repository policy](docs/reference/CI_AND_REPOSITORY_POLICY.md) for the
current check matrix.

### Deferred design and productization

The following remain unimplemented and require separate compatibility,
security, and resource reviews before they can become product behavior:

- chunked `CsvColumnarDictionary`;
- per-chunk or global zstd dictionaries;
- CSV-aware chunk boundaries;
- improved numeric encoding;
- full v3 archive encoding or decoding;
- bounded-latency cooperative cancellation; and
- Desktop, GPU, hybrid scheduling, cloud, or service integrations.

Encryption, authentication/HMAC, DRM, obfuscation, cloud services, telemetry,
and licensing remain explicit non-goals unless a later project scope authorizes
and specifies them.

## 5. Reporting Security Issues

This repository does not currently publish a private security-reporting address
or security-advisory URL. Do not infer or invent one from package metadata. If
no trusted private contact is already available, withhold exploit details and
confidential data until the repository owner publishes a reporting channel.

Please include the affected DataPack version and platform, whether the archive
is v1 or v2, the command used, the observed error or resource behavior, and a
minimal reproducer when it is safe to share one. Do not attach sensitive source
data publicly or open a public issue containing an undisclosed exploit or
confidential archive contents.
