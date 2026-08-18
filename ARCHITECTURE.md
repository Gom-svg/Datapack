# DataPack Architecture

## Compatibility and Reliability Invariants

DataPack has two primary engineering invariants:

1. Restoration must be byte-for-byte identical to the original. SHA256 equality is the final identity check; a mismatch is a failure.
2. Existing `.dpack` v1 and v2 archives remain readable. A wire-format change requires an explicit new version and compatibility documentation; it must never be introduced silently.

Supporting invariants are:

- normal v2 decompression verifies per-chunk and global SHA256 by default
- the v2 payload and table order are deterministic for the same input and options whenever practical
- large-file paths have bounded memory rather than buffering the complete input
- profiling and partial benchmarks must not imply validation that did not occur
- `CsvColumnarDictionary` and its proven parser retain their frozen v1 representation

## Current Consumer and Service Layers

The public service boundary separates operation facts from presentation:

```text
CLI arguments / Python calls
            |
            v
CLI adapter / PyO3 adapter
            |
            v
typed Application requests
            |
            v
Rust Application services
            |
            v
analysis / planning / codecs / storage / validation / comparison / benchmark
            |
            v
versioned Application results
            |
            v
CLI rendering / Python dictionaries / future consumer presentation
```

`datapack::application` exposes synchronous, path-based Analyze, Compress,
Decompress, Validate, Compare, and Benchmark services. Requests own operation
options and results expose versioned facts without leaking Clap types, terminal
rendering, stdout, or stderr into the service layer. The CLI adapter owns Clap,
human and JSON rendering, terminal progress labels, and diagnostic streams.
See the [Rust Application API V1](docs/reference/APPLICATION_API_V1.md).

Advisor, Tune, and test-data generation remain outside the public Application
API. Advisor is a CLI policy projection over shared analysis and planner facts;
Tune and generation remain dedicated CLI workflows. The
[Python SDK foundation](python/README.md) is deliberately thinner still: it
exposes Analyze, Compress, Decompress, Validate, and Compare, returning the
corresponding Rust reports as Python dictionaries. It does not reimplement
planning, parsing, codec, validation, or comparison logic in Python.

## `.dpack` v1 Architecture

The established v1 container remains:

- `DPACK` magic
- version `1`
- original file type
- metadata length
- bincode metadata
- one zstd-compressed payload

V1 has two active archive modes.

### V1 RawZstd

RawZstd stores the original byte stream in one zstd frame. Planner-selected v1 RawZstd compression writes the existing header and metadata, then streams the frame directly to the archive. Decompression reads only the header and metadata before streaming restored bytes to the output.

This implementation changed the I/O path, not the wire format. Old v1 archives remain readable and newly streamed RawZstd archives are valid v1 files.

### V1 CsvColumnarDictionary

The columnar path parses structured input using comma, tab, pipe, or semicolon
as its single-byte delimiter, selects dictionary or plain representation per
column, stores reconstruction metadata, and zstd-compresses that reconstruction
payload. CSV, TSV, PSV, and semicolon-delimited inputs can therefore use v1
structured compression when analysis and planner safety requirements pass.
`CsvColumnarDictionary` and the internal `DCSV01` identifier are historical
names; the frozen payload's existing delimiter byte distinguishes all four
supported dialects. Dictionary IDs use the smallest safe integer
representation. The encoder reconstructs and compares the bytes before
accepting a columnar candidate.

This path is intentionally still a whole-file operation. It is strongest on
repetitive structured data but is not the bounded large-file architecture. Its
parser, payload representation, and v1 behavior remain compatibility surfaces.

V1 does not contain the v2 global and per-chunk SHA256 fields. A validating benchmark or an external original/restored SHA256 comparison provides end-to-end identity proof without changing the v1 format.

## `.dpack` v2 Chunked RawZstd Architecture

V2 remains RawZstd-only and retains its existing fixed layout.

### Global header: 64 bytes

- `DPACK` magic
- version `2`
- archive mode, currently RawZstd
- original size in bytes
- global SHA256 of the original bytes
- chunk count
- target chunk size

### Chunk table entry: 80 bytes per chunk

- chunk ID
- original offset
- original size
- compressed offset
- compressed size
- compression mode, currently RawZstd
- zstd level
- SHA256 of the original uncompressed chunk bytes
- reserved padding

Compressed payloads follow the table in chunk-ID order. Decompression restores table entries in order and validates restored size, the per-chunk hash, and the global hash by default.

The bounded asynchronous pipeline changes how the existing v2 bytes are
produced, not their interpretation. No v2 field, offset, mode identifier, or
ordering rule is changed by the pipeline.

Two compressors can produce those same v2 payloads: the default `chunked-raw-zstd` backend and the explicit `zstd-mt-experimental` backend. Both emit one standard zstd frame per chunk. The backend is an execution choice reported by profiling, benchmarking, and tuning, not a new v2 compression-mode identifier required for decompression.

## V2 Bounded Compression Pipeline

The large-file compression path is divided into a continuously fed reader, a persistent compression pool, and an ordered writer.

```text
input file -> reader -> bounded work queue -> compression worker(s)
                                                   |
output file <- ordered writer <- bounded results/reorder buffer
```

### Reader stage

The reader owns sequential buffered input I/O. It:

- reads byte chunks in original order
- assigns stable chunk IDs and original offsets
- updates the global SHA256 in source order
- sends work into a bounded queue

The reader cannot run arbitrarily far ahead. `--max-in-flight-chunks` bounds the combined work admitted to the pipeline. If the queue is full, reading waits and memory remains bounded.

### Worker stage

The default `chunked-raw-zstd` backend creates a persistent pool of `--threads` DataPack workers. Each worker:

- computes the original chunk SHA256
- selects the deterministic adaptive zstd level when requested, or uses the fixed level
- compresses the complete chunk as an independent zstd frame
- records compression timing and size information for profiling
- sends either a `ChunkResult` or an error to the result path

Persistent workers avoid recreating a set of operating-system threads for every batch. Chunks are independent, so a slow chunk does not prevent later chunks from being compressed.

The opt-in `zstd-mt-experimental` backend uses a deliberately different topology: one outer DataPack compression worker owns one reusable native zstd compression context, and `--threads` configures the native zstd workers inside that context. It does not create `--threads` outer workers, which avoids multiplying native worker pools and oversubscribing the machine. The reader, bounded admission permits, ordered writer, hashes, and error propagation remain the same.

### Ordered writer stage

Results can arrive out of order. The writer keeps a bounded reorder map keyed by chunk ID and commits only the next expected result. It:

- writes compressed payloads strictly in chunk-ID order
- records final compressed offsets and sizes
- updates the in-memory table entry for each committed chunk
- reports written progress separately from read and compressed progress
- seeks back to finalize the global header and table after all payloads succeed

This preserves deterministic layout without forcing workers to finish in order. A completed chunk is not reported as written until its bytes have actually been committed.

### Memory model

Peak memory is approximately the sum of:

- queued input chunk buffers
- one active input and compressed buffer per worker
- compressed results waiting for an earlier chunk in the reorder buffer
- zstd workspaces
- the small fixed-size chunk table

Chunk size, thread count, backend, and maximum in-flight chunks interact. For `chunked-raw-zstd`, the default in-flight limit is `max(threads * 2, 4)`. For `zstd-mt-experimental`, it resolves from one outer worker and is 4; `threads` instead controls native zstd concurrency. Tuning, benchmarking, and profiling report the resolved values so memory-sensitive runs can set the in-flight bound explicitly.

The complete input and the complete set of compressed payloads are never held in memory.

### Pipeline shutdown and output safety

Reader, worker, and writer errors propagate through the pipeline and stop
admission of new work. Internal stop signaling and channel closure let the
remaining stages exit without waiting for an unbounded queue. This is pipeline
failure coordination, not a public cancellation contract: callers cannot
request cancellation, and no bounded cancellation latency is promised.

V2 compression writes to a temporary sibling path. Only after all chunks, hashes, header data, and table entries have been successfully finalized and flushed is the temporary archive renamed to the requested output. Ordinary failures remove the incomplete temporary output. A failed run must not leave a partial file that appears to be a successful archive.

Input and output path equality is rejected before destructive output creation.

### Determinism boundary

For the same input, options, DataPack build, and zstd implementation, the following are stable:

- chunk boundaries and IDs
- selected backend and thread interpretation
- adaptive-level decisions
- per-chunk hashes
- table ordering and offsets
- payload ordering

Worker completion order and progress-log interleaving do not affect archive order. Same-build deterministic-output tests cover both backends, but byte-identical zstd frames across different zstd library versions or platforms are not promised. Compatibility means those standard frames remain decodable and restore the same original bytes.

## V2 Decompression

The current decompressor reads the v2 table, validates its ordering and bounds, then restores independent frames in table order through a temporary output. For each chunk it checks the restored size and, unless `--no-verify` was explicitly supplied, the chunk SHA256. It also checks final restored size and global SHA256, then commits the requested restored path only after success.

`--no-verify` bypasses hash comparison only for explicit speed testing. It does not change archive parsing or restored-size validation, and its output is not considered integrity-validated.

## Automated Tuning Architecture

The visible experimental `datapack tune` command measures a grid of chunk sizes and thread counts using either `chunked-raw-zstd` or `zstd-mt-experimental`. `max` in the thread list resolves to the current logical CPU count. For the default backend, report `threads` means independent DataPack chunk workers; for native MT, it means native zstd workers behind one outer DataPack worker and the implicit maximum in-flight count is 4. Each configuration can run multiple times, with per-run rows retained so medians and variability remain inspectable.

Tuning can measure either:

- the complete source
- a byte-prefix selected by `--max-input-mb`, explicitly marked as sampled

By default a tuning run compresses, decompresses, and validates SHA256. `--skip-roundtrip` and `--no-hash` are explicit non-validating controls. V2 creation still computes the hashes required by the archive format even when tuning-side identity validation is skipped.

The CSV schema includes:

- timestamp and DataPack version when available
- source path, full source bytes, measured bytes, and sampling state
- backend, archive version, mode, chunk size, worker count, and maximum in-flight chunks
- adaptive-level state and zstd-level strategy
- configured run count and run index or aggregate marker
- archive size, ratio, compression/decompression time, throughput, and total time
- SHA256 match, validation status, and benchmark scope
- estimated peak memory, temporary path, notes, and error text

Temporary artifacts are cleaned by default and retained only with `--keep-temp`. An explicit report path is not overwritten unless `--force` is supplied. Without `--output`, tuning creates a non-colliding `<input-stem>-tune-<unix-timestamp>.csv` beside the input and adds a numeric suffix if needed.

### Recommendations

Tuning prints three recommendations without modifying global defaults:

1. highest compression throughput
2. highest compression ratio
3. balanced configuration

The balanced score normalizes throughput and ratio across successful candidates and includes a memory penalty when an estimate is available. It is a shortlist heuristic, not a universal hardware profile. A sampled recommendation must be confirmed on a complete representative file with SHA256 validation.

## Benchmark Semantics

Benchmark scope and integrity validation are separate dimensions.

Scopes are:

- `full`: complete source and normal comparison flow
- `partial`: complete source with one or more comparison stages skipped
- `sampled`: a prefix was measured because `--max-input-mb` was used
- `estimate_only`: bounded planning without compression, decompression, or hashing

Validation statuses are:

- `validated`: full measured-input round trip passed SHA256 equality
- `partially_validated`: a sampled prefix passed SHA256 equality, but the complete source was not tested
- `not_validated`: no conclusive SHA256 round trip occurred

Skipping only the standalone zstd baseline changes comparison scope but not DataPack identity validation. Partial reasons are accumulated once and deduplicated before table or JSON serialization.

Reports expose full source bytes separately from measured input bytes and identify whether the zstd baseline, round trip, and hash comparison ran. Table and JSON outputs carry the same core semantics; JSON uses real nulls for unavailable values and remains parseable.

When a v2 comparison is requested, `--backend chunked-raw-zstd` selects the default independent-chunk worker pool and `--backend zstd-mt-experimental` selects the native zstd experiment. Reports include `chunked_backend` so throughput is not detached from its thread model. Selecting the experimental backend does not skip normal round-trip or SHA256 validation.

Inputs larger than 1 GiB receive a non-blocking warning before the full zstd baseline, DataPack compression, decompression, and hashing flow. The user retains control over whether to run the full measurement.

Large benchmark artifacts default to the input directory rather than a potentially small RAM-backed system temporary directory. `DATAPACK_TEMP_DIR` overrides the location, and cleanup guards remove artifacts on normal return and propagated errors unless `--keep-temp` is active.

## Profiling and Progress Model

The CLI adapter sends direct compression, direct decompression, benchmark, and
tuning diagnostics to stderr so stdout reports remain machine-readable. The
Application services themselves do not write to either stream.

Coarse stage metrics are preferred over fragile micro-timings. Compression profiling can report:

- operation, archive version, mode/backend, sizes, and ratio
- chunk count and size, worker count, and maximum in-flight chunks
- adaptive-level state and zstd-level distribution
- read, hash/compress, ordered-write, table-finalization, and total elapsed time where measurable
- average, fastest, and slowest chunk compression timing
- throughput and average compressed chunk size

Decompression profiling can report:

- operation, archive version, mode/backend, sizes, chunk count, and verification state
- read/decompress/write/verification timing where measurable
- average, fastest, and slowest chunk restoration timing
- total elapsed time and restored-input throughput

Overlapping pipeline timings are not added together and presented as wall-clock
time. The reusable Application progress contract reports typed operation,
phase, and lifecycle state plus completed and optional total byte/item counts.
It does not contain terminal labels or rendered messages. The CLI presentation
layer can derive percent, elapsed time, ETA, throughput, and optional
in-flight/level diagnostics from those facts. Progress observation is
synchronous and observational; it is not a cancellation hook.

The public Rust Application API and Python foundation expose no cancellation
token. Uniform bounded-latency cooperative cancellation remains deferred until
bulk parsing, codec, validation, benchmark, cleanup, and transactional commit
paths can all honor one truthful contract. This public boundary is distinct
from the internal v2 pipeline shutdown used to propagate a stage failure.

## Experimental Native Zstd Multithreading Backend

The Rust `zstd` 0.13 dependency enables its `zstdmt` feature, and DataPack exposes that capability as the explicit `zstd-mt-experimental` backend for v2 `compress`, `benchmark`, and `tune`. The default remains `chunked-raw-zstd`.

The implementation keeps v2 chunk semantics:

- one outer DataPack compression worker consumes chunks in order from the bounded reader queue
- `--threads` configures the reusable native zstd context's worker count
- each call emits one independent standard zstd frame
- adaptive zstd level selection remains per chunk when requested
- the ordered writer, 64-byte header, 80-byte table entries, mode identifiers, offsets, and payload order are unchanged
- original chunk SHA256 and global SHA256 are computed and verified normally
- the archive does not need to record which compressor created a standard frame

This topology deliberately avoids nesting multiple DataPack workers around multiple native worker pools. It also creates a real tradeoff: native zstd coordinates parallel work within one chunk, while the default backend compresses several chunks independently. Larger chunks may expose more native parallelism but can raise memory cost; small chunks, I/O-bound workloads, or particular data may show no benefit. No performance improvement is claimed without measurement.

The existing v2 decompressor requires no backend selection. Compatibility, full round-trip SHA256, corrupted-hash behavior, CLI profiling, and deterministic output for a fixed WSL build are tested. Deterministic table and payload ordering remains an invariant, but compressed bytes are not promised to remain identical across zstd library versions, build configurations, or platforms.

The backend stays experimental and non-default until representative full-file measurements show a repeatable benefit with acceptable memory behavior. Benchmark and tune reports name the selected backend and preserve normal validation by default. Hardware-grid comparisons should keep chunk, validation, and storage conditions identical across backends.

## Storage and Hardware Effects

Tuning results include the complete I/O environment. NVMe, SATA SSD, HDD, network filesystems, antivirus scanning, indexing, and sync clients can shift the bottleneck between reader, workers, and writer.

Synced paths such as OneDrive are especially unsuitable for controlled throughput comparisons because the sync engine may read or upload an archive while DataPack is still producing it. Recommended benchmarking uses a local non-synced path, sufficient free disk space for input/archive/restored artifacts, and a separate copy step after measurement.

Under WSL, a repository or dataset below `/mnt/c` crosses the WSL-to-NTFS
boundary and can behave differently from a Linux-native filesystem or a native
Windows run. Results from `/mnt/c` must identify that environment and must not
be compared directly with results from another filesystem, operating mode, or
cache state unless those conditions are deliberately controlled.

No chunk or thread default is changed solely from one machine or dataset. Candidate defaults are adopted only after repeated tuning, acceptable memory behavior, deterministic output checks, and full SHA256 validation on representative structured and high-cardinality data.

## Future `.dpack` v3 Design Boundary

`.dpack` v3 is design-only. There is no v3 reader, writer, or executable archive
semantics, and neither v1 nor v2 is reinterpreted to prototype it. V2 remains
chunked RawZstd; new structured chunk semantics require an explicit future
version. The complete Phase 17 proposal is
[RFC-007 — `.dpack` v3 structured-chunk design](docs/rfcs/RFC-007-dpack-v3-structured-chunk-design.md).

The design explores a truly chunked structured archive in which a column or
chunk can select an explicitly encoded strategy such as Plain, Dictionary,
RLE, Delta, BitPacking, or RawZstd. That list is a design space, not a promise
that every strategy will ship. RawZstd remains the safe fallback when
structured parsing is unsafe, unsupported, or not beneficial.

Any future v3 implementation must preserve the following boundaries:

- reconstruction remains byte-for-byte exact, including delimiter choice,
  quote spelling, escaped quotes, LF versus CRLF, whitespace, empty fields,
  UTF-8 bytes, and final-newline presence;
- record-aware chunk boundaries never split quoted fields, escaped-quote
  pairs, or CRLF terminators;
- parsing, dictionaries, metadata, offsets, chunk counts, and decompression
  operate under explicit resource and integer-overflow limits;
- independent chunk integrity and a global identity check provide corruption
  isolation without weakening transactional output;
- schema, dictionary, strategy, fallback, and payload ordering rules are
  deterministic for the promised compatibility scope;
- archives are hardware-neutral and universally decodable by the CPU path;
  optional accelerators cannot create a separate archive format; and
- malformed or malicious input returns an error rather than panicking or
  forcing unbounded allocation.

Implementation requires its own compatibility review, frozen fixtures,
corruption and resource-limit tests, fuzzing, and representative performance
evidence. Comparisons must keep dataset, validation level, filesystem, build
profile, and cache conditions controlled and must cover v1 structured, v2
RawZstd, the proposed v3 mode, and standalone zstd. No ratio or throughput
claim follows from the design alone.

## Current Product Boundaries

- DataPack Desktop is not implemented.
- GPU acceleration and adaptive CPU/GPU scheduling are not implemented. A GPU
  is a possible execution backend, not an archive format; there is no
  `.dpack-gpu` format.
- Full v3 encoding and decoding are not implemented.
- SIMD/parser replacement and an LZ4 archive mode remain unimplemented and
  require independent evidence plus compatibility review.
- There is no SSD cache subsystem, ML-based planning, cloud service, or
  mandatory telemetry architecture.
- No future work may silently change the v1 or v2 wire format.
