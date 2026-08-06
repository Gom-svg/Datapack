# DataPack

DataPack is a Rust CLI for lossless compression of CSV, TXT, logs, database exports, and other flat structured data.

The non-negotiable requirement is byte-for-byte restoration. SHA256 equality between the original and restored files is the source of truth; a mismatch is a failure.

DataPack is not presented as universally faster than Zstd or LZ4. Those are excellent general-purpose compressors. DataPack competes by selecting a suitable path for the data, providing strong compression on repetitive structured CSVs, scaling predictably on large difficult files, and making integrity and performance visible.

## Choose a Compression Path

| Path | Archive | Best fit | Important tradeoff |
| --- | --- | --- | --- |
| `CsvColumnarDictionary` | `.dpack` v1 | Repetitive structured CSVs, categorical columns, BI exports, and reports | Often the best DataPack ratio, but it remains a whole-file path and is not the large-file chunked mode |
| `RawZstd` | `.dpack` v1 | Planner fallback, non-CSV data, and compatibility with the established container | Bounded streaming is available, but v1 has no chunk table or stored SHA256 hashes |
| Chunked `RawZstd` | `.dpack` v2 | Huge or high-cardinality CSVs, numeric-heavy files, logs, and difficult datasets | Scalable and observable, with bounded memory and per-chunk/global SHA256; it does not apply columnar transforms |
| Native zstd MT (experimental) | `.dpack` v2 | Explicit comparison of zstd's internal worker pool on v2 chunks | Non-default; thread semantics and memory behavior differ, and no speed advantage is assumed |

Use normal `compress --mode fast` when you want the planner to select between the established v1 paths. Use `--chunked` explicitly for the v2 large-file path. DataPack v2 is RawZstd-only today.

## Commands

```powershell
datapack analyze input.csv --plan --sample-mb 64
datapack compress input.csv output.dpack --mode fast
datapack compress input.csv output.dpack --mode fast --chunked --profile
datapack decompress output.dpack restored.csv --profile
datapack benchmark input.csv --quick --profile
datapack tune input.csv --max-input-mb 1024 --runs 1 --output tune.csv
datapack generate-test-data realistic sample.csv --rows 1000000 --seed 20260710
```

When running from the source tree, prefix a command with `cargo run --`, for example `cargo run -- analyze samples/small_sample.csv`.

## Archive Formats and Integrity

### `.dpack` v1

DataPack continues to read existing v1 archives. The v1 container stores:

- `DPACK` magic and version `1`
- original file type
- bincode metadata length and metadata
- one zstd-compressed payload

The payload is either `RawZstd` original bytes or the zstd-compressed `CsvColumnarDictionary` reconstruction payload. Planner-selected v1 RawZstd compression and decompression stream with bounded memory without changing the wire format. Columnar v1 remains the established whole-file encoder and decoder.

The v1 format does not contain the global and per-chunk SHA256 fields added in v2. For end-to-end v1 identity proof, use a validating benchmark or compare the original and restored SHA256 values explicitly.

### `.dpack` v2

DataPack v2 is the scalable chunked RawZstd container. It stores:

- `DPACK` magic and version `2`
- original byte size and global SHA256
- target chunk size and chunk count
- one fixed-size table entry per chunk containing original and compressed offsets and sizes, zstd level, mode, and SHA256 of the original chunk
- compressed chunk payloads in chunk-ID order

Compression uses a bounded reader/worker/ordered-writer pipeline. Chunks can finish compression out of order, but the writer commits them in chunk-ID order, preserving the existing v2 layout and deterministic ordering. `--max-in-flight-chunks` limits buffered work and compressed results. For the default `chunked-raw-zstd` backend, its conservative default is `max(threads * 2, 4)`.

Normal v2 decompression verifies each restored chunk and the global SHA256 by default, writes restoration through a temporary file, and commits the requested output only after the operation succeeds. `--no-verify` is an explicit unsafe speed-testing option; output from that run is not final integrity validation. Existing outputs are refused unless `--force` is supplied, and `--keep-temp` preserves a failed operation's sibling `.partial` file for debugging.

```powershell
datapack compress input.csv output.dpack --mode fast --chunked
datapack compress input.csv output.dpack --mode fast --chunked --chunk-size-mb 128 --threads 8
datapack compress input.csv output.dpack --mode fast --chunked --threads 4 --max-in-flight-chunks 6 --adaptive-level
datapack decompress output.dpack restored.csv
datapack decompress output.dpack restored.csv --no-verify
datapack decompress untrusted.dpack restored.csv --max-output-mb 8192 --max-chunks 10000 --max-memory-mb 1024
```

Decompression limits are optional and use MiB (1,048,576 bytes). `--max-output-mb` checks the declared restored size before payload work, `--max-chunks` lowers the internal 1,000,000-entry v2 ceiling before table allocation, and `--max-memory-mb` checks an approximate lower-bound working-memory estimate (zstd workspace and allocator overhead are not exact). V2 structural validation, bounded zstd output, size checks, and the internal chunk ceiling always remain active. Compression also accepts `--max-memory-mb` for the explicit `max-in-flight-chunks * chunk-size` lower-bound check.

All v1 and v2 production compression/decompression paths create a sibling temporary file and rename it to the requested output only after the operation's validation succeeds. An interrupted or failed operation must not look like a successful archive or restoration. Use `--force` deliberately to replace a regular file; directories and other non-regular destinations are always rejected.

Same input plus the same options produces the same chunk boundaries and ordered payload layout. Archive bytes are deterministic whenever practical for the same DataPack and zstd implementation; future library versions are not assumed to emit byte-identical zstd frames.

### Experimental native zstd multithreading

V2 `compress`, `benchmark`, and `tune` expose `--backend zstd-mt-experimental`. It is opt-in and does not replace the default `chunked-raw-zstd` backend.

```powershell
datapack compress input.csv output_mt.dpack --mode fast --chunked --backend zstd-mt-experimental --chunk-size-mb 128 --threads 8 --profile
datapack benchmark input.csv --quick --chunked --backend zstd-mt-experimental --chunk-size-mb 128 --threads 8 --profile
datapack tune input.csv --backend zstd-mt-experimental --chunk-sizes-mb 128,256 --threads-list 4,8 --runs 1 --output tune_mt.csv
```

For this backend, DataPack uses one outer pipeline compression worker and interprets `--threads` as the number of native zstd workers inside that worker. The default maximum in-flight count therefore resolves from one outer worker and is 4 unless set explicitly. This avoids nesting an unconstrained DataPack worker pool around multiple native zstd pools.

Each chunk is still an independent standard zstd frame. The v2 header, table, compression-mode identifiers, payload order, per-chunk SHA256, and global SHA256 are unchanged, and the compressor backend is not required to decode the archive. Normal v2 decompression and verification work exactly as for the default backend.

Tradeoffs are workload- and hardware-dependent:

- Native zstd can coordinate compression workers internally and reuse its compression context between chunks.
- Only one chunk is compressed by the outer pipeline at a time; `--threads` no longer means independently compressed DataPack chunks.
- Native work queues and larger chunks can increase memory use.
- Small chunks or I/O-bound workloads may not benefit.
- The backend may be deterministic for a fixed tested build, but byte-identical frames are not promised across zstd versions or platforms.

Round-trip, SHA256, compatibility, CLI profile, and same-build deterministic-output tests pass in WSL. That is correctness evidence, not a throughput claim. Use benchmark or tune on representative local data before drawing performance conclusions.

## Planning and Columnar Compression

The preflight `CompressionPlan` samples the input and predicts `CsvColumnarDictionary` or `RawZstd`. It is deliberately conservative on unstable row widths, unsupported CSV structures, high cardinality, and poor projected dictionary savings.

```powershell
datapack analyze input.csv --plan --sample-mb 64
datapack compress input.csv output.dpack --mode fast
datapack compress input.csv output.dpack --mode best --sample-mb 128
datapack compress input.csv output.dpack --verify-best
```

`--mode fast` follows the planner for one planned compression path. `--mode best` compares candidates when the projection is close, and `--verify-best` builds both safe candidates and keeps the smaller archive when structured analysis is eligible. A hard analysis limit or malformed/unsupported structured input overrides both modes and enters the transactional streaming RawZstd path without building a columnar candidate. The columnar encoder still validates its own reconstruction before that payload can be stored; an unsafe or unsupported candidate also falls back to RawZstd.

Dictionary planning limits are controlled with `--max-dictionary-values` and `--max-dictionary-mb`. The defaults are 65,535 values and 64 MiB of logical dictionary entries per column. Columns exceeding a sampled limit switch to `Plain` in the executable plan. The full-input DCSV01 writer enforces the same limits, including the stored header value; if later values make a planned Dictionary column exceed either limit, the structured candidate is rejected and compression falls back byte-exactly to `RawZstd`.

## Experimental Hardware Tuning

`datapack tune` runs a grid of v2 chunked RawZstd configurations and writes a CSV report. It supports the default `chunked-raw-zstd` backend and the explicit `zstd-mt-experimental` backend. It is an experimental measurement tool: recommendations apply to the measured dataset, storage path, zstd build, and hardware. It does not change DataPack defaults.

```powershell
datapack tune input.csv `
  --backend chunked-raw-zstd `
  --chunk-sizes-mb 32,64,128,256 `
  --threads-list 1,2,4,8,max `
  --runs 1 `
  --output tune.csv
```

Useful tuning controls include:

- `--output <csv_path>`: choose the CSV report path; an existing explicit report is protected unless `--force` is also supplied. If omitted, DataPack creates a non-colliding `<input-stem>-tune-<unix-timestamp>.csv` beside the input and adds a numeric suffix if necessary.
- `--backend <chunked-raw-zstd|zstd-mt-experimental>`: select the tuning backend; the default remains `chunked-raw-zstd`.
- `--chunk-sizes-mb <list>`: provide a comma-separated positive-MiB grid such as `64,128,256`.
- `--threads-list <list>`: provide positive counts and/or `max`, such as `4,8,max`; rows mean DataPack chunk workers for the default backend and native zstd workers for the MT backend.
- `--runs <n>`: repeat each grid point and retain per-run measurements.
- `--max-input-mb <n>`: tune a byte-prefix sample, set `benchmark_scope=sampled`, and use `partially_validated` only if that prefix passes a SHA256 round trip.
- `--skip-roundtrip`: skip decompression; the result is non-validating.
- `--no-hash`: skip tuning-side identity hashing where permitted; the result is non-validating. Required v2 archive hashes are still written.
- `--adaptive-level`: use the deterministic per-chunk level heuristic.
- `--max-in-flight-chunks <n>`: bound queued and reorder-buffered chunks.
- `--keep-temp`: preserve generated archives and restored files for inspection; otherwise they are cleaned up.
- `--profile`: print pipeline diagnostics to stderr.
- `--force`: permit replacement of an existing report path. Without it, tuning refuses to overwrite the report.

For `zstd-mt-experimental` tuning rows, the reported `threads` value is the native zstd worker count. The implicit maximum in-flight count is 4 because the backend uses one outer DataPack worker; an explicit `--max-in-flight-chunks` value is still reported when supplied.

The CSV records each run and configuration. Its core columns are:

- identity and input: `timestamp`, `datapack_version`, `input_path`, `input_size_bytes`, `measured_input_size_bytes`, `input_sampled`, and `max_input_mb`
- configuration: `backend`, `archive_version`, `mode`, `chunk_size_mb`, `chunk_size_bytes`, `threads`, `max_in_flight_chunks`, `adaptive_level`, and `zstd_level_strategy`
- run identity: `runs`, `run_index`, plus an aggregate/median marker when emitted
- results: `output_size_bytes`, `compression_ratio`, `compression_time_ms`, `compression_mb_per_sec`, `decompression_time_ms`, `decompression_mb_per_sec`, and `total_time_ms`
- integrity and context: `sha256_match`, `validation_status`, `benchmark_scope`, `peak_memory_estimate_mb`, `temp_path_used`, `notes`, and `error`

At completion, tuning reports three recommendations:

- best compression throughput
- best compression ratio
- a balanced score based on normalized throughput and ratio, with a memory penalty when an estimate is available

A sampled run can guide a shortlist, but always validate the selected configuration against a full representative file before treating it as a production setting.

## Benchmarking Large Files

Normal benchmark behavior includes compression, decompression, and SHA256 comparison. The benchmark reports the full source size separately from the measured input size and uses consistent table and JSON fields.

```powershell
datapack benchmark input.csv
datapack benchmark input.csv --json
datapack benchmark input.csv --quick --profile --chunked --chunk-size-mb 128 --threads 8
datapack benchmark input.csv --quick --profile --chunked --backend zstd-mt-experimental --chunk-size-mb 128 --threads 8
datapack benchmark input.csv --quick --no-zstd-baseline
datapack benchmark input.csv --quick --no-roundtrip --no-hash
datapack benchmark input.csv --estimate-only
datapack benchmark input.csv --quick --max-input-mb 256
```

Benchmark scope values are:

- `full`: the complete source was measured with the normal comparison flow.
- `partial`: the complete source was measured, but one or more comparison stages were skipped.
- `sampled`: `--max-input-mb` limited measurement to a prefix of the source.
- `estimate_only`: bounded planning ran without compression, decompression, or hashing.

Validation status is separate from scope:

- `validated`: a full measured-input round trip passed SHA256 identity validation.
- `partially_validated`: identity validation passed for a sampled prefix rather than the complete source.
- `not_validated`: no conclusive SHA256 round trip was performed, including `--no-roundtrip`, `--no-hash`, and `--estimate-only`.

Skipping the standalone zstd baseline makes the comparison partial but does not, by itself, make DataPack identity validation partial. Partial reasons are deduplicated in table and JSON output.

Core report fields include `source_size_bytes`, `measured_input_size_bytes`, `input_sampled`, `validation_status`, `partial_reasons`, stage-performed booleans, selected and estimated modes, plan correctness, backend sizes and ratios, compression/decompression times and throughput, and `total_elapsed_time_ms`. When several backends run, each backend's throughput is identified separately.

Large-file controls:

- `--quick` uses one timing run.
- `--no-zstd-baseline` skips the standalone zstd comparison.
- `--no-roundtrip` skips decompression and identity validation. `--skip-full-roundtrip` remains an alias.
- `--no-hash` skips benchmark-side SHA256 identity validation.
- `--estimate-only` runs bounded planning only.
- `--max-input-mb <n>` measures the first `n` MiB, caps planning to that same prefix (including estimate-only mode), and never describes that result as full-file validation.
- `--max-in-flight-chunks <n>` controls memory pressure for the optional chunked comparison.
- `--backend <chunked-raw-zstd|zstd-mt-experimental>` selects the v2 backend to benchmark; the experimental choice changes `--threads` to native zstd workers and remains non-default.

For inputs larger than 1 GiB, a requested full benchmark prints this non-blocking warning because the complete flow can be lengthy:

> Large input detected. Full benchmark may run zstd baseline, DataPack compression, decompression, and SHA256 validation. Use --quick, --max-input-mb, --no-roundtrip, or --no-zstd-baseline for faster partial tests.

Benchmark artifacts are created beside the input by default. Set `DATAPACK_TEMP_DIR` to choose another directory. Artifacts are removed on success and ordinary errors unless `--keep-temp` is supplied. Diagnostics go to stderr so stdout tables and JSON remain parseable.

## Profiling and Progress

`compress --profile`, `decompress --profile`, `benchmark --profile`, and `tune --profile` emit diagnostics to stderr. Stable summary fields include the operation, archive version, selected mode/backend, input and output sizes, ratio where relevant, chunk count and size, worker and in-flight limits, verification state, elapsed time, and throughput. Pipeline and per-chunk timing fields are reported where available rather than inferred from overlapping stages.

V2 progress distinguishes:

- chunks read
- chunks compressed
- chunks written
- total chunks and in-flight work
- MiB read and committed
- percent, elapsed time, ETA, and throughput
- per-chunk zstd level when profiling is enabled

Compressed progress is not presented as written progress: the ordered writer may still be waiting for an earlier chunk.

## Recommended Large-File Workflow

### 1. Fast safe compression

```powershell
datapack compress input.csv output.dpack --mode fast --chunked --chunk-size-mb 128 --threads 8 --profile
```

### 2. Full validation

```powershell
datapack decompress output.dpack restored.csv --profile
Get-FileHash input.csv -Algorithm SHA256
Get-FileHash restored.csv -Algorithm SHA256
```

The two hashes must match exactly.

### 3. Partial huge-file benchmark

```powershell
datapack benchmark input.csv --quick --profile --no-roundtrip --no-hash --no-zstd-baseline
```

This is intentionally non-validating and must not be used as the final integrity check.

### 4. Sample tuning

```powershell
datapack tune input.csv --max-input-mb 1024 --chunk-sizes-mb 64,128,256 --threads-list 4,8 --runs 1 --output tune.csv
```

### 5. Full tuning

```powershell
datapack tune input.csv --chunk-sizes-mb 64,128,256 --threads-list 4,8,max --runs 1 --output full_tune.csv --skip-roundtrip
```

This full-input tuning example measures compression but deliberately skips round-trip validation. Validate the winning configuration separately.

## Starting Configuration Guidance

Do not treat these as universal defaults. Measure on representative data:

- SSD/NVMe: begin with 64 or 128 MiB chunks and 4 to 8 workers, then tune 64/128/256 MiB against 4, 8, and logical-CPU-count workers.
- Memory-limited systems: use 32 or 64 MiB chunks, fewer workers, and a small explicit `--max-in-flight-chunks` value.
- HDD or network storage: additional workers may not help once storage is saturated; compare low concurrency as well.
- Synced folders such as OneDrive: compression, antivirus scanning, indexing, and sync upload can overlap and distort throughput. Benchmark from a local non-synced SSD/NVMe directory when possible, then copy completed artifacts as a separate step.

These starting points describe the default independent-chunk backend. Treat native zstd MT as a separate experiment because one outer worker and `--threads` native workers have different scaling and memory behavior.

Peak memory depends on chunk size, active workers, queued input, compressed results waiting for ordered output, and zstd workspaces. Increasing all three of chunk size, worker count, and in-flight count can multiply memory use.

## Safety Notes

- Never use the same path for input and output.
- V1 and v2 compression write a sibling temporary archive and commit only after successful completion.
- V1 and v2 decompression commit a sibling temporary restoration only after all format-available size and integrity checks; v2 normally includes per-chunk and global SHA256 verification.
- Existing outputs require `--force`; failed operations preserve the previous file or leave no final output. `--keep-temp` is debugging-only.
- Normal v2 decompression verifies hashes. Use `--no-verify` only for explicitly unsafe speed testing.
- Sampled and partial benchmarks do not establish full-file integrity.
- Tuning recommendations do not alter global defaults.
- Any future format change must use an explicit version; existing v1 and v2 archives remain readable.

## Test Data Generation

```powershell
datapack generate-test-data repetitive samples/repetitive_1m.csv --rows 1000000 --seed 42
datapack generate-test-data realistic samples/realistic_500k.csv --rows 500000 --seed 2026
datapack generate-test-data high-cardinality samples/high_cardinality_100k.csv --rows 100000 --seed 99
datapack generate-test-data random samples/random_100k.csv --rows 100000 --seed 99
```

Generated CSVs use CRLF line endings, RFC 4180-style quoting, valid UTF-8, no BOM, and deterministic output for the same profile, row count, and seed.

## Build, Test, and Fuzz

```powershell
cargo fmt -- --check
cargo test
cargo build --release
```

If native Windows execution is blocked by Application Control, use WSL with a target directory outside the Windows tree:

```powershell
wsl --cd /mnt/c/Users/gompr/Documents/datapack -- bash -lc "cargo fmt -- --check && CARGO_TARGET_DIR=/tmp/datapack-target cargo test && CARGO_TARGET_DIR=/tmp/datapack-target cargo build --release"
```

The `fuzz/` directory contains a separate `cargo-fuzz` CSV round-trip target. It is intentionally excluded from normal `cargo test`.

```powershell
cargo install cargo-fuzz
cargo fuzz run csv_roundtrip
```

## Roadmap

- Phase 5B: bounded asynchronous v2 RawZstd pipeline, tuning, profiling, benchmark clarity, and reliability engineering.
- Phase 5C plan only: chunked `CsvColumnarDictionary` with CSV-aware boundaries, schema/dictionary coordination, per-chunk fallback, and full integrity metadata.
- Phase 5D plan only: profile-guided parser and CPU optimization, considering `memchr`, `bstr`, `csv-core`, and `simdutf8` only where measurements justify them.
- Phase 5E plan only: an optional explicit LZ4 ultra-fast mode with lower expected ratio, SHA256 validation, and honest comparison against zstd and current DataPack modes.
- Native zstd multithreading is available as the explicit non-default `zstd-mt-experimental` v2 compress/benchmark/tune backend; it has no unqualified performance claim.

See `ARCHITECTURE.md` for format invariants, pipeline design, tuning methodology, and the detailed future plans.
