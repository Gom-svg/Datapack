# RFC-002 — Compatible CLI Modularization

- Status: Phase 2 implementation complete; integrated gates passed
- Date: 2026-08-06
- Input revision: `0ae2640` (`rfc-001b-compatible-analysis-core`)
- Declared MSRV: Rust 1.85.0
- Predecessor: [RFC-001B](RFC-001B-compatible-analysis-core.md)
- Decision: proceed only as a private, behavior-preserving source move

## 1. Decision and authority

Phase 2 may split the current CLI implementation into private modules so that
argument definition, command orchestration, shared CLI mechanics, benchmark
methodology, and presentation are easier to review independently. It may not
change what the CLI accepts, what it writes, what it prints, which branch it
executes, or how it validates restored bytes.

The compatible dependency direction is:

```text
public cli facade
  -> command orchestrators
      -> private shared CLI leaves
      -> existing analysis / planning / generation / tuning / storage domains

benchmark orchestrator
  -> benchmark model / report / streaming / temp leaves
  -> private shared CLI leaves
  -> existing analysis / compression / storage domains
```

This RFC authorizes file moves, visibility tightening, import cleanup, and
additive compatibility tests. It does not authorize semantic cleanup. If a
source move exposes an apparent bug or inconsistency, the Phase 2 action is to
characterize and preserve it, not silently repair it.

The executable baseline has priority over this prose. A frozen fixture, legacy
golden, established integration test, or pre-change CLI observation that
conflicts with the proposed split stops the affected slice for investigation.

## 2. Pre-change audit

The audit inspected `src/main.rs`, the complete `src/cli/mod.rs`, the existing
private `src/cli/analysis_report.rs`, domain calls made by every subcommand,
benchmark and tuning methodology, error-to-exit-code mapping, atomic-output
helpers, frozen fixtures, analyze goldens, round-trip tests, and security tests.

At the input revision, `src/cli/mod.rs` owns all of the following at once:

- public Clap entry type and private command/value enums;
- defaults and dispatch;
- compression and decompression orchestration;
- tune parsing and summary rendering;
- benchmark model, execution, temporary files, hashing, and both renderers;
- v1 candidate composition;
- chunked-option construction;
- validation and operation error context;
- progress wrappers and profile diagnostics.

`analysis_report.rs` is already separated and presentation-only. Phase 2 keeps
it unchanged.

### 2.1 Pre-change help fingerprints

The following SHA-256 values were recorded from the exact help stdout bytes
before modularization:

| Help invocation | SHA-256 |
|---|---|
| `datapack --help` | `55ecaf5992964a651bd6db59278f82814c045bf3ae17e78250184a6a76004358` |
| `datapack analyze --help` | `a0d73b18855397c6b5f33692e3ec5c0ac05c2d903a86bbac8bfd4c5430b35a42` |
| `datapack compress --help` | `b414b8edec15329f5d355f6b8886e26857ee161646cb4a6c4d9e9b76917f5602` |
| `datapack decompress --help` | `aa3f2b679df52184a8a1474b92fefe8b12acde964be9ac5df9c1f5c0c81e3e02` |
| `datapack generate-test-data --help` | `77e1fad3d326b51196cefde0362e87a58b95eca555a32e726bfb0c667e73d16e` |
| `datapack tune --help` | `da8cf6b628a13fd4cd1b54fb911cbdee3ee5f05100e7968ffe09a2d96bd8caf4` |
| `datapack benchmark --help` | `dc52eec6e40d814c7d0deee18686217e471b7b2260da0ecc2b1e1d6fd02ce3` |

These hashes are local refactor evidence under the locked dependency graph and
declared toolchain. They are not a portable external protocol: a Clap upgrade,
different executable name, or authorized later UX phase could intentionally
change them. No such change is authorized in Phase 2.

## 3. Chosen private module layout

The target layout is deliberately asymmetric. Clap stays in the facade to
avoid re-export and visibility churn; benchmark receives a directory because
it has an internal model and several execution/rendering paths. Tiny commands
do not receive files merely for symmetry.

```text
src/cli/
├── mod.rs
├── analysis_report.rs
├── archive.rs
├── chunked_options.rs
├── compress.rs
├── decompress.rs
├── progress.rs
├── profile.rs
├── tune.rs
├── validation.rs
└── benchmark/
    ├── mod.rs
    ├── model.rs
    ├── report.rs
    ├── streaming.rs
    ├── temp.rs
    └── tests.rs
```

Every new module is private. Cross-module items use no visibility broader than
`pub(super)`. The established `datapack::cli::Cli` and
`datapack::cli::run(Cli)` surface remains at the same path; no new Rust API is
published.

### 3.1 Facade

`cli/mod.rs` remains the public compatibility facade and retains:

- `Cli` itself;
- the private Clap `Command`, `CompressMode`, and `ChunkedBackendArg` types;
- every Clap option, doc string, default, possible-value mapping, and alias;
- private command option structures whose construction fixes dispatch
  semantics;
- `run` and its match order;
- the trivial `analyze` and `generate-test-data` adapters.

Keeping the derive declarations together preserves help ordering and avoids a
new public re-export. The facade delegates after parsing; it does not absorb
command algorithms during the split.

`analysis_report.rs` remains unchanged. It consumes the Phase 1 analysis result
and renders only the frozen legacy analyze report.

### 3.2 Command orchestrators

| Module | Responsibility |
|---|---|
| `compress.rs` | Preserve compression validation order, the pre-analysis chunked bypass, v1 planning/candidate branch order, atomic commit, and operation error context. |
| `decompress.rs` | Preserve version dispatch, verification and limit options, v1 streaming versus whole-file restoration, atomic commit, and operation error context. |
| `tune.rs` | Preserve comma-list parsing, `max` resolution, experimental warning, domain invocation, recommendation rendering, and all-runs-failed behavior. |
| `benchmark/mod.rs` | Preserve top-level planning and branch order, full versus estimate-only orchestration, in-memory benchmark flow, optional chunked comparison, and final report routing. |

The command modules do not import one another.

### 3.3 Shared leaves

| Module | Responsibility and boundary |
|---|---|
| `archive.rs` | CLI-only composition of planner modes with existing v1 storage encoders: planned encoding, detailed fallback, payload-to-mode mapping, and strict-smaller candidate comparison. It defines no wire bytes itself. |
| `chunked_options.rs` | Map already-parsed CLI flags to existing v2 storage options; retain chunk/thread/backend/default/max-in-flight and memory-limit validation. It does not select command branches. |
| `progress.rs` | Progress phase formatting, interval accounting, progressed readers/writers, and buffered full/prefix reads. It depends on standard I/O and CLI error results, never on a command. |
| `profile.rs` | Direct and chunked profile diagnostics plus shared duration/rate display helpers. Benchmark-specific profile rendering remains in `benchmark/report.rs`. It reports measurements but does not choose methodology or make performance claims. |
| `validation.rs` | Option-independent input/output distinction, parent and overwrite checks, numeric-limit conversions, normalized paths, and `OperationFailed` context. Command-specific v1 decompression limits remain in `decompress.rs`. |

Shared leaves never depend on `compress`, `decompress`, `tune`, or `benchmark`.
They do not become a second domain layer and do not duplicate storage logic.

### 3.4 Benchmark children

| Module | Responsibility and boundary |
|---|---|
| `benchmark/model.rs` | Private metrics, chunked metrics, scope/reason/validation policy, duration aggregation, and rate/ratio helpers. Parsed option structures remain in the facade. |
| `benchmark/report.rs` | Estimate-only and executed table/JSON renderers, including ordering, precision, escaping, aliases, and unavailable/null behavior. It receives model values and performs no compression or hashing. |
| `benchmark/streaming.rs` | The existing RawZstd-planned streaming benchmark and its prefix compression, standalone zstd, restore, hash, and copy helpers. |
| `benchmark/temp.rs` | `DATAPACK_TEMP_DIR` resolution, established names, and the cleanup/keep guard. |
| `benchmark/tests.rs` | Relocated private benchmark unit tests and additive structural regressions. |

Benchmark children may consume `benchmark::model` and the shared CLI leaves.
The model and shared leaves do not depend back on benchmark orchestration or
rendering. Report code does not invoke streaming, storage, planning, or hash
code.

## 4. Exact command-line surface to preserve

Subcommand order remains `analyze`, `compress`, `decompress`,
`generate-test-data`, `tune`, `benchmark`, followed by Clap's `help` command.
Root `-h`/`--help` and `-V`/`--version` remain available. Positional argument
order and all option display order remain unchanged.

### 4.1 `analyze [OPTIONS] <INPUT>`

- `--plan` is false by default and controls presentation only; planning always
  occurs.
- `--sample-mb <SAMPLE_MB>` defaults to 64 and retains the established runtime
  range of 1 through 2,048 MiB.

### 4.2 `compress [OPTIONS] <INPUT> <OUTPUT>`

Options remain, in order:

- `--mode <MODE>`, default `fast`, values `fast` and `best`;
- `--sample-mb`, default 64;
- `--verify-best`, default false;
- `--max-dictionary-values`, default 65,535;
- `--max-dictionary-mb`, default 64;
- `--chunked`, default false;
- optional `--chunk-size-mb`;
- optional `--threads`;
- optional `--max-in-flight-chunks`;
- optional `--backend`, values `chunked-raw-zstd` and
  `zstd-mt-experimental`;
- `--adaptive-level`, default false;
- `--profile`, default false;
- `--force`, default false;
- `--keep-temp`, default false;
- optional `--max-memory-mb`.

Chunked mode is enabled by any of `--chunked`, `--chunk-size-mb`, `--threads`,
`--max-in-flight-chunks`, `--backend`, `--adaptive-level`, or
`--max-memory-mb`. `--profile` alone does not enable it.

### 4.3 `decompress [OPTIONS] <INPUT> <OUTPUT>`

Options remain, in order:

- `--no-verify`, default false;
- `--profile`, default false;
- optional `--max-output-mb`;
- optional `--max-chunks`;
- optional `--max-memory-mb`;
- `--force`, default false;
- `--keep-temp`, default false.

### 4.4 `generate-test-data [OPTIONS] <PROFILE> <OUTPUT>`

- Accepted runtime profile strings remain `repetitive`, `realistic`,
  `high-cardinality`, and `random`.
- `--rows` remains a string-valued Clap option with default `10000` and
  hyphenated values allowed. The adapter continues to map an integer parse
  failure to zero before the established 1 through 5,000,000 range check.
- `--seed` remains an optional `u64`; omitted seeds retain their fixed
  profile-specific defaults.

### 4.5 `tune [OPTIONS] <INPUT>`

Options remain, in order:

- optional `--output`;
- `--chunk-sizes-mb`, default `32,64,128,256`;
- `--threads-list`, default `1,2,4,8,max`;
- `--runs`, default 1;
- optional `--max-input-mb`;
- `--skip-roundtrip`, `--no-hash`, `--adaptive-level`, `--keep-temp`, and
  `--profile`, all false by default;
- `--backend`, default `chunked-raw-zstd`, with both established backend
  values;
- optional `--max-in-flight-chunks`;
- `--force`, default false.

Comma-list values continue to trim surrounding whitespace and reject empty
elements. `max` remains case-insensitive and resolves to the existing logical
CPU default. Tune continues to reject `--runs 0`; this intentionally differs
from benchmark's run handling.

### 4.6 `benchmark [OPTIONS] <INPUT>`

Options remain, in order:

- `--json`, `--keep-temp`, and `--quick`, false by default;
- `--runs`, default 3;
- `--profile` and `--chunked`, false by default;
- optional `--chunk-size-mb`, `--threads`, and
  `--max-in-flight-chunks`;
- optional `--backend`, with both established backend values;
- `--adaptive-level`, `--no-zstd-baseline`, `--no-roundtrip`, `--no-hash`,
  and `--estimate-only`, false by default;
- optional `--max-input-mb`.

`--skip-full-roundtrip` remains the one explicit compatibility alias and maps
to `--no-roundtrip`. Any benchmark chunked option enables the optional v2
comparison. `--quick` forces one run; without it, `--runs 0` continues to clamp
to one rather than failing.

### 4.7 Parsing, errors, and exit routing

Clap parse errors remain Clap-owned and go to stderr with Clap's exit status.
After parsing, `main` continues to print a domain failure exactly as
`error: {err}` and uses `DatapackError::exit_code()`:

- invalid profile and unreadable analysis input: 1;
- output-not-writable and invalid CSV: 2;
- row range: 3;
- all other domain variants: 1.

Validation order and error wrapping are observable behavior. Compression and
decompression must continue to validate input/output distinction and overwrite
policy before entering their inner operation, preserve a prior output on
failure, and wrap inner failures with the same operation, paths, reason, and
output-status wording.

## 5. Compression and decompression invariants

### 5.1 Non-chunked v1 compression

> Current-behavior note: RFC-003 later added hard-bounded analysis and an
> earlier streaming RawZstd recovery branch for hard-limited or malformed
> structured input. The sequence below records the certified Phase 2 behavior
> for analysis-eligible input.

Non-chunked compression continues to write v1 and to follow this order:

1. validate paths and output policy;
2. run the shared Phase 1 analysis using the requested sample size;
3. clone the returned plan;
4. apply the existing dictionary-limit compatibility mutation and emit
   `Column '{column}' exceeded dictionary limit; switching to Plain.` for each
   affected column;
5. decide whether candidate comparison is required;
6. select the established streaming or whole-file implementation;
7. write to a sibling partial and commit atomically.

Candidate comparison remains:

```text
verify_best
OR (mode == best AND plan.estimated_savings_percent < 15.0)
```

If the plan is RawZstd and comparison is not required, compression remains the
256 KiB-buffered v1 streaming path. Otherwise the CLI reads the complete input.
`--verify-best` builds RawZstd and the safe columnar candidate, keeps columnar
only when it is strictly smaller, and keeps RawZstd on a tie or unavailable
columnar candidate. Its diagnostic remains:

```text
verify-best: selected <MODE>, saved <N> bytes over alternative.
```

The conditional `--mode best` comparison uses the same strict-smaller rule but
does not gain the `verify-best` selection line. A planned columnar encode still
falls back to RawZstd when the columnar codec returns no safe candidate. The
codec's byte-exact reconstruction check remains authoritative.

Dictionary-limit mutations remain declarative planner behavior; this phase
does not make them new encoder allocation limits or change the codec's own
column-mode choice.

### 5.2 Chunked v2 compression

The chunked branch remains before analysis. It accepts binary/non-CSV input and
always writes v2 RawZstd; `--mode`, `--sample-mb`, dictionary limits, and
`--verify-best` do not govern that output.

Defaults remain:

- 64 MiB target chunks;
- existing logical-CPU count, clamped by storage to its supported range;
- `chunked-raw-zstd` backend;
- max-in-flight equal to `max(2 * outer_workers, 4)`;
- one outer worker, and therefore default max-in-flight 4, for
  `zstd-mt-experimental`, whose requested threads belong to native zstd;
- fixed level unless `--adaptive-level` is supplied.

The optional compression memory check remains the established
`chunk_size_bytes * max_in_flight_chunks` comparison. It must not be renamed or
presented as a measured peak-memory guarantee.

### 5.3 Decompression

Decompression continues to inspect the archive version before choosing a
decoder:

- v2 uses the existing chunked file decoder, verifies per-chunk and global
  SHA-256 by default, and applies optional output/chunk/memory limits;
- `--no-verify` disables v2 hash verification only; it does not suppress zstd,
  range, size, table, or write failures;
- v1 RawZstd, Plain, and Dictionary payload kinds use the established streaming
  restore to an atomic sibling partial;
- v1 CSV columnar uses whole-archive decode and whole restored bytes before the
  atomic write;
- v1 has no stored SHA-256, so `--no-verify` does not create a new v1 behavior;
- v1 output and approximate memory checks retain their current formulas,
  ordering, and messages.

`--force` and `--keep-temp` retain their exact output-protection and failed
partial-file semantics for both versions.

## 6. Benchmark methodology invariants

Phase 2 moves benchmark code; it does not normalize or redesign the benchmark.
In particular, it preserves the following path-dependent methodology.

### 6.1 Planning, scope, and validation

> Current-behavior note: RFC-003 later moved `--max-input-mb` validation before
> planning and caps planning to the same measured prefix, including
> estimate-only mode. The bullets below record the certified Phase 2 order.

- Every benchmark first runs shared analysis with a fixed 64 MiB planning
  sample.
- `--max-input-mb` is validated after planning and limits measurement only
  after the plan exists. A valid limit does not change estimate-only measured
  bytes; estimate-only reports planner coverage. Zero remains an error even in
  estimate-only mode.
- Estimate-only returns before compression, decompression, hashing, and
  optional chunked execution.
- The estimated RawZstd mode selects the streaming benchmark before the
  in-memory input read. Estimated columnar mode selects the whole-prefix
  in-memory path.
- Hashing is enabled exactly when neither `--no-hash` nor `--no-roundtrip` is
  set. `--no-roundtrip` therefore also disables hashing even if `--no-hash` was
  not supplied.
- A full-source successful hash comparison reports `validated`; a prefix
  comparison reports `partially_validated`; a run without hashing reports
  `not_validated`. A requested hash that cannot complete or mismatches is a
  command error.
- `benchmark_scope` is `sampled` whenever the measured prefix is shorter than
  the source. Otherwise it is `full` only when no partial reason exists, and
  `partial` otherwise.
- Partial reasons retain this order and de-duplication: standalone zstd
  skipped; round-trip skipped/output identity not validated; SHA-256 skipped;
  configured input prefix only.
- The warning for a source larger than 1 GiB remains limited to the complete
  flow with no estimate-only mode, no input limit, and zstd/roundtrip/hash all
  enabled.

### 6.2 Runs and timing boundaries

- `--quick` uses one run. Otherwise `max(runs, 1)` is used.
- There is no warm-up run.
- DataPack and an optional chunked comparison run the selected count for
  compression and, unless disabled, decompression.
- Duration aggregation sorts durations and chooses `values[len / 2]`. For an
  even count this is the upper middle, not an arithmetic mean.
- The standalone zstd baseline runs once rather than `runs_used` times.
- Input and restored hashes run once outside the reported compression and
  decompression durations.

The timing boundary differs by the planned path and must remain explicit:

- RawZstd-planned DataPack compression times prefix file read, v1 container
  construction, zstd, and archive-file write together. Its restore timing also
  includes restored-file writing.
- Columnar-planned DataPack compression times in-memory encode/compress after
  the prefix read. Archive writing is measured separately for profile output
  and excluded from `compression_time_ms`. Decompression times in-memory archive
  decode and restore without writing a restored output file.
- Standalone zstd is streamed file-to-file on the RawZstd branch but is an
  in-memory compression on the columnar branch.
- Chunked timings surround the existing end-to-end v2 file encode/decode APIs.

These are compatibility facts, not evidence that values from different paths
are directly comparable. Phase 2 must not add throughput, latency, memory, or
scalability claims based on them.

### 6.3 Temporary artifacts

Benchmark temporary paths continue to use `DATAPACK_TEMP_DIR` when set,
otherwise the input parent, otherwise the system temporary directory. Names
retain the input stem, process ID, and established suffix for v1 archive,
restored output, zstd baseline, v2 archive, v2 restore, and sampled prefix.

The guard removes all reserved artifacts on ordinary exit and error unless
`--keep-temp` is set. Preserved paths remain printed only in the established
non-JSON output cases. Modularization must not broaden cleanup paths or delete
unrelated files.

### 6.4 Tuning is a separate methodology

Tune remains an experimental v2 file-API grid, not an alias for benchmark. It:

- sorts and de-duplicates chunk and thread values;
- rejects zero runs instead of clamping;
- hashes the measured source once and each restored output when validation is
  enabled;
- records one CSV row for every configuration run and continues after a
  per-run failure;
- aggregates successful configurations with arithmetic medians for even
  counts, unlike benchmark duration aggregation;
- preserves the balanced score
  `0.55 * throughput + 0.45 * ratio - 0.08 * approximate_memory` and its
  existing tie-breakers;
- never changes DataPack defaults automatically.

The memory value in tuning remains explicitly approximate.

## 7. Presentation contracts

Presentation moves may change imports and ownership only. Output bytes, field
order, precision, stdout/stderr routing, and availability markers remain
unchanged.

### 7.1 Analyze, compress, decompress, generation, and tune

- Analyze continues to use the unchanged `analysis_report.rs`; all existing
  goldens, including 14-character display-name truncation and planning-time
  normalization rules, remain authoritative.
- Successful compress and decompress commands do not add a summary on stdout.
- Progress remains on stderr. `ProgressReporter::finish` emits a final line even
  for operations shorter than the five-second interval. Dynamic elapsed,
  throughput, and ETA values are not moved to stdout.
- Dictionary warnings, `verify-best` selection, columnar fallback diagnostics,
  and profile diagnostics remain on stderr.
- Direct and chunked profile field names, order, numeric precision, and
  `unavailable`/`not_stored_in_v1` values remain unchanged.
- Generate-test-data is normally silent. Inputs over 500,000 requested rows
  continue to report dynamic stderr progress at the existing row/time cadence
  and at completion.
- Tune always prints its experimental hardware/dataset warning on stderr before
  invoking the domain tuner. Stdout retains report path, source and measured
  sizes, success/failure totals, and the Best throughput, Best compression
  ratio, and Balanced blocks. Recommendation precision remains throughput
  three decimals, ratio and score four decimals, and approximate memory one
  decimal.
- The tune CSV header remains the established ordered 32-field schema.

### 7.2 Executed benchmark table and JSON

The ordinary table retains its metric/value/description headings, metric order,
descriptions, precision, and `unavailable` strings. Non-JSON output retains the
trailing `mode` line and the conditional temporary-artifact lines. A columnar
candidate diagnostic remains on stderr.

Executed benchmark JSON remains a single object on stdout. Its 44 keys retain
this order:

```text
source_size_bytes
measured_input_size_bytes
original_size_bytes
benchmark_scope
validation_status
partial_reasons
input_sampled
zstd_baseline_performed
roundtrip_performed
hash_performed
datapack_size_bytes
zstd_only_size_bytes
chunked_backend
chunked_chunk_size_mb
chunked_threads
chunked_max_in_flight_chunks
chunked_raw_zstd_size_bytes
compression_ratio
selected_mode
estimated_mode
plan_was_correct
peak_memory_estimate_mb
planning_time_ms
runs_used
total_elapsed_time_ms
columnar_candidate_error
zstd_only_ratio
chunked_raw_zstd_ratio
compression_time_ms
chunked_compression_time_ms
zstd_only_compression_time_ms
compression_mb_per_sec
zstd_only_compression_mb_per_sec
chunked_compression_mb_per_sec
decompression_time_ms
chunked_decompression_time_ms
decompression_mb_per_sec
chunked_decompression_mb_per_sec
roundtrip_sha256_match
no_roundtrip
skip_full_roundtrip
no_hash
chunked_roundtrip_sha256_match
datapack_beats_zstd
```

`original_size_bytes` remains a compatibility alias for measured input size.
`skip_full_roundtrip` remains a compatibility alias for `no_roundtrip`.
Unavailable JSON values remain `null`, while the table uses `unavailable`.
Manual JSON escaping retains quotes, backslashes, standard control escapes, and
lowercase `\u` escapes for other characters through U+001F.

### 7.3 Estimate-only table and JSON

Estimate-only remains a distinct report rather than a partially populated
executed `BenchmarkMetrics`. Its JSON retains these 37 ordered keys:

```text
source_size_bytes
measured_input_size_bytes
original_size_bytes
benchmark_scope
validation_status
partial_reasons
input_sampled
zstd_baseline_performed
roundtrip_performed
hash_performed
selected_mode
estimated_mode
plan_was_correct
planning_sample_bytes
planning_time_ms
datapack_size_bytes
zstd_only_size_bytes
chunked_raw_zstd_size_bytes
chunked_backend
chunked_chunk_size_mb
chunked_threads
chunked_max_in_flight_chunks
compression_ratio
zstd_only_ratio
chunked_raw_zstd_ratio
compression_time_ms
decompression_time_ms
compression_mb_per_sec
decompression_mb_per_sec
zstd_only_compression_time_ms
zstd_only_compression_mb_per_sec
chunked_compression_time_ms
chunked_decompression_time_ms
chunked_compression_mb_per_sec
chunked_decompression_mb_per_sec
roundtrip_sha256_match
total_elapsed_time_ms
```

This schema intentionally does not acquire executed-only `runs_used`, memory,
candidate-error, skip-flag, chunked-hash, or zstd-comparison result fields.
Planning-only unavailable values remain `null` in JSON and `unavailable` in the
table. The fixed partial reason remains
`estimate-only: compression, decompression, and hashing skipped`.

## 8. Wire, API, and behavioral non-goals

Phase 2 does not change:

- `.dpack` v1 or v2 magic, version, header, metadata, bincode layout, chunk
  table, offsets, zstd payloads, hashes, codec identifiers, or validation;
- DCSV01 parsing, encoding, cost selection, fallback, or byte-exact restoration
  check;
- any frozen fixture, fixture manifest, fixture hash, or fixture source;
- any legacy analyze golden;
- planner facts, thresholds, reasons, sample semantics, dictionary-limit gap,
  or selected archive mode;
- compression/decompression branch or validation order;
- benchmark and tuning timing boundaries, aggregation, scope, validation,
  temporary files, or report schemas;
- command names, positional arguments, flags, aliases, defaults, possible
  values, help prose, order, stdout/stderr routing, or exit codes;
- `datapack::cli::Cli`, `datapack::cli::run`, or any other public Rust API;
- dependencies or features in `Cargo.toml`/`Cargo.lock`.

Explicit non-goals include:

- JSON output for `analyze`;
- a shared public report model or new serializable public DTO;
- new flags, aliases, subcommands, environment variables, or config files;
- removal or renaming of compatibility aliases;
- parser, delimiter, header, multiline-record, or UTF-8 behavior changes;
- making the compression plan executable at column level;
- changing defaults based on tuning results;
- benchmark redesign, warm-ups, statistical normalization, or cross-path
  comparability claims;
- performance, memory, throughput, latency, or scalability claims;
- async I/O, a progress framework, logging framework, or dependency upgrade;
- public CLI plugin points or a public command API.

No new public API may be introduced merely because a moved function now needs
to cross a file boundary. The remedy is private module structure and
`pub(super)`, not `pub`.

## 9. Implementation sequence

The compatible sequence is:

1. extract shared leaves (`archive`, `chunked_options`, `progress`, `profile`,
   `validation`) without changing call order;
2. move tune parsing/orchestration/rendering to `tune.rs`;
3. move compression and decompression as coherent commands;
4. move benchmark intact into `benchmark/mod.rs` before subdividing it;
5. split benchmark model, report, streaming, temp, and tests;
6. remove obsolete imports and duplicated private helpers;
7. run the complete Phase 2 gate ledger.

Compilation should occur after coherent slices. A temporary compile failure
inside an uncommitted source move is not evidence that a public shim should be
added. No slice is complete until its focused behavior checks pass.

## 10. Additive test plan

Existing tests remain the primary compatibility authority:

| Test | Preserved coverage |
|---|---|
| `tests/compatibility_fixtures.rs` | Frozen v1/v2 archive sizes/hashes, corruption handling, byte-exact restoration, and no fixture regeneration. |
| `tests/analyze_cli.rs` | Exact legacy analyze stdout/stderr and exit behavior. |
| `tests/analysis_core_consistency.rs` | Shared analysis consumers, deterministic v1 output, chunked bypass, and presentation separation. |
| `tests/legacy_analysis_api.rs` | Existing public analysis API and metadata behavior. |
| `tests/round_trip.rs` | v1/v2 compression, decompression, benchmark scopes/validation/JSON basics, profiles, backends, and tune output. |
| `tests/security_hardening.rs` | Malformed archives, limits, verification, option failures, path safety, force, partial cleanup, and error context. |
| `planning::tests` | Frozen policy decisions, sampling characterization, and exact restoration of candidates. |

The focused Phase 2 addition is `tests/cli_surface.rs`. It records command
names, flags, explicit defaults, possible values, and the visible
`--skip-full-roundtrip` alias through Clap's public command metadata. Pre/post
help fingerprints are also compared in the controlled refactor environment
without declaring them a portable protocol. The test intentionally avoids
dynamic timing and prose snapshots.

The six relocated benchmark unit tests continue to cover canonical scope,
unique reason ordering, validation mismatch errors, JSON escaping, and
temporary cleanup/keep behavior. The audit also identified two useful future
characterization gaps: the complete legacy benchmark JSON key/nullability
schema and direct CLI execution of `--verify-best`. Phase 2 does not add those
tests because it does not modify either behavior; representative benchmark JSON
and strict-smaller candidate/restore behavior remain covered by the existing
round-trip and planner tests. The gaps are recorded rather than being presented
as completed coverage.

Tests must not assert dynamic timing or throughput values. Normalization remains
limited to fields already normalized by established goldens.

## 11. Phase 2 gate ledger

Only results observed against the final integrated Phase 2 tree with the
declared MSRV are marked passed.

All Cargo build/test commands use:

```text
CARGO_TARGET_DIR="$HOME/.cache/datapack-modernization"
```

| Scope | Command / gate | Result | Required evidence |
|---|---|---|---|
| Source scope | Review diff for only private CLI modularization, RFC, and additive tests | PASS | No storage/metadata/codec/dependency change; no fixture or golden modification. |
| Formatting | `cargo fmt --check` | PASS | Exit zero on final integrated tree. |
| Compilation | `cargo check` | PASS | Rust 1.85.0, exit zero. |
| CLI surface | `cargo test --test cli_surface` plus pre/post help fingerprints | PASS | 1 passed; every recorded help hash remained byte-identical. |
| Benchmark unit | `cargo test cli::benchmark::tests` | PASS | 6 passed, 0 failed in the integrated suite. |
| Frozen wire | `cargo test --test compatibility_fixtures` | PASS | 4 passed; no fixture regenerated. |
| Legacy analyze | `cargo test --test analyze_cli` | PASS | 9 passed; all goldens unchanged. |
| Shared analysis | `cargo test --test analysis_core_consistency` | PASS | 3 passed. |
| Legacy API | `cargo test --test legacy_analysis_api` | PASS | 4 passed in the integrated suite. |
| Planner | `cargo test planning::tests` | PASS | 26 passed. |
| Round trip | `cargo test --test round_trip` | PASS | 31 passed. |
| Security | `cargo test --test security_hardening` | PASS | 69 passed. |
| Full suite | `cargo test` | PASS | 210 passed, 0 failed, 0 ignored. |
| Lints | `cargo clippy --all-targets --all-features -- -D warnings` | PASS | Zero warnings. |

A compatibility failure, corruption risk, unexplained regression, mandatory
gate failure, public-surface expansion, or wire-format ambiguity is a Phase 2
blocker. Fixtures and goldens must never be regenerated to make a move pass.

## 12. WSL/NTFS methodology context

The authorized environment is WSL/Linux. The repository is stored on the
Windows-mounted NTFS path `/mnt/c/Users/gompr/Documents/datapack`, while Cargo
artifacts are placed in the native WSL path
`/home/gompr/.cache/datapack-modernization`.

This separation is methodology context only. NTFS/DrvFS traversal, native WSL
storage, host caching, antivirus, scheduler state, and first-versus-warm build
effects can all influence elapsed time. Therefore:

- Phase 2 uses functional status, exact bytes, schemas, and validation outcomes
  as gates;
- it does not compare build, test, compression, decompression, benchmark, or
  tuning speed before and after the source move;
- it does not infer a product performance improvement or regression from this
  environment;
- help hashes are compatibility fingerprints, not performance measurements.

Any future performance claim requires a separately authorized, controlled
methodology with stated hardware, storage, cache state, corpus, repetitions,
statistics, and uncertainty. RFC-002 supplies none and makes no such claim.
