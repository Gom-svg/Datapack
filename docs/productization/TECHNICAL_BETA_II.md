# DataPack Technical Beta II

Productization phase: P6

Status: **CERTIFIED — CLOSED**.

Local certification passed. Hosted certification passed in run
[`34404009535`](https://github.com/Gom-svg/Datapack/actions/runs/34404009535)
with result **SUCCESS**: all 18 required hosted jobs passed. P6 Technical Beta II
is formally closed. P7 — Release Artifacts / Release Engineering is next and
has not started.

Development version: `0.1.0`

Starting checkpoint: `ca1f0f7777a81481dfbecb3e1b8e3112bf6045c5`

Certified technical checkpoint: `621574a2607e3857fe932dbe9db8ec6eddc5a9fd`

## 1. Purpose and evidence boundary

P6 is an operational correctness and product-behavior certification phase. It
does not redesign compression. It exercises the installed Python wheel, the
release CLI, and the existing Rust engine across deterministic structured-data,
resource, corruption, cancellation, retry, and byte-exact restoration cases.

The following remain frozen:

- `.dpack` v1 and v2 wire formats;
- historical archive compatibility and protected fixture hashes;
- byte-exact restoration and transactional destination behavior;
- P3 progress, P4 cancellation, and P5 error contracts;
- Rust 1.85 / Cargo 1.85.0 certification policy;
- CPython 3.9 ABI3 wheel policy; and
- the DataPack product, CLI, Python import, distribution, and crate identities.

P6 adds no codec, parser, planner, archive, Rust API, Python ABI, runtime
dependency, workflow topology, telemetry, network operation, or publication
step. `.dpack` v3, adaptive structured compression, GPU, Desktop, and public
release engineering remain outside P6.

Correctness assertions are certification gates. Wall-clock measurements are
always **OBSERVATIONAL** and never participate in pass/fail policy.

## 2. Technical Beta II harness

`scripts/technical_beta_ii.py` executes through an installed
`datapack-engine` wheel. It generates all deterministic inputs inside an
operating-system temporary directory, records only aggregate facts, hashes,
diagnostic codes, and environment metadata, and removes generated inputs,
archives, and restored files when the run exits normally.

The harness uses a specified SplitMix64 generator rather than Python's random
module. The CI inputs are frozen by row count, seed, byte size, SHA-256, and
planner-selected v1 mode.

| Workload | Seed | CI rows | Manual-beta rows | Frozen CI bytes | Frozen CI SHA-256 | Expected v1 mode |
| --- | ---: | ---: | ---: | ---: | --- | --- |
| Repetitive | `0xDADA6001` | 512 | 80,000 | 32,940 | `ca7c293cc0cf022f55291adb0e429ee17ebf650e1e8693e1c1cd5b178434345a` | `csv_columnar_dictionary` |
| Realistic structured | `0xDADA6002` | 768 | 60,000 | 82,622 | `8dafd26144a13df7a28c284976dfd6b99b9644451068325e0729b585821ea91c` | `csv_columnar_dictionary` |
| High cardinality | `0xDADA6003` | 768 | 40,000 | 136,558 | `9e7a18da61145cd068f8c8257be4745746670205f8c6662a8547fc19e0945063` | `raw_zstd` |
| Random/incompressible-like | `0xDADA6004` | 1,024 | 50,000 | 299,036 | `07e4cd3f06c3b2afe6b07c1329f6959cf270b999f79fae03851919a9047d1cc4` | `raw_zstd` |
| Delimited database export | `0xDADA6005` | 2,048 | 120,000 | 278,887 | `b479305411154a35d76e92ecb5234d6371bfb593e9f8f07e2d6fedf826e7bf8a` | `csv_columnar_dictionary` |

The database-export workload is pipe-delimited text. It does not claim support
for SQL `INSERT`, DDL, or arbitrary SQL dump syntax.

Each normal workload performs:

```text
analyze
  -> compress
  -> validate --against source
  -> physical decompress
  -> size equality
  -> SHA-256 equality
  -> Python filecmp
  -> independent streaming byte comparison
```

The ETL workload is additionally forced through v2 chunked RawZstd with small
chunks and a bounded admission configuration so CI exercises multiple chunks,
the chunk table, and integrity verification.

## 3. Coverage

### 3.1 Format and fidelity matrix

The harness covers:

- comma CSV, TSV, PSV, and semicolon-delimited text;
- LF and CRLF;
- final newline present and absent;
- quoted fields, escaped double quotes, and delimiters inside quotes;
- leading zeros and scientific notation without numeric reserialization;
- leading/trailing whitespace;
- UTF-8 content;
- empty and trailing-empty fields;
- long text; and
- quoted multiline values only for alternate-delimiter paths whose current
  parser contract supports them.

The frozen physical-line comma planner is not broadened by P6. A comma CSV with
a quoted multiline record remains an expected structured-analysis rejection in
this phase and is still preserved byte-exactly through the safe raw path.

### 3.2 Edge and bounded-analysis cases

| Case | P6 expectation |
| --- | --- |
| Empty input | Analyze rejects; compression safely preserves zero bytes through RawZstd. |
| Header only / one row | Analyze and byte-exact roundtrip succeed. |
| Single column | Analyze rejects under the current contract; safe raw roundtrip succeeds. |
| Headerless input | Current first-record-as-header limitation is recorded; bytes are preserved. |
| Ambiguous delimiter | Analyze rejects; safe raw roundtrip succeeds. |
| Inconsistent width | Analyze rejects; safe raw roundtrip succeeds. |
| Unterminated quote / bare CR | Analyze rejects; safe raw roundtrip succeeds. |
| All-empty columns | Empty-value aggregation and exact roundtrip are checked. |
| 256 columns | Reasonable wide input is analyzed and restored exactly. |
| 4,097 columns | Manual-beta profile records the bounded column-limit result. |
| Record larger than 8 MiB | Manual-beta profile records the bounded record-byte-limit result. |

An expected structured-analysis rejection is not an engine failure and is not
reported as one. Safe byte preservation through RawZstd remains a deliberate
product behavior.

### 3.3 Operational resilience

P6 asserts:

- early, mid-operation, and late compression cancellation;
- no false terminal-success progress event after cancellation;
- preservation of an existing destination sentinel;
- cleanup when partial retention is not requested;
- retained compression partial output when `keep_partial` is requested;
- retry compression and validation after cancellation;
- decompression cancellation with the same destination and terminal-event
  guarantees;
- retained decompression partial output with `keep_partial` (the final local
  manual run retained 65,536 bytes);
- retry decompression with SHA-256 and independent byte equality;
- safe default overwrite rejection; and
- successful explicit overwrite.

### 3.4 Resource, validation, corruption, and Compare

The harness exercises:

- v2 compression memory admission rejection;
- validation `max_output_bytes`, `max_chunks`, and `max_memory_bytes`;
- matching decompression limits while preserving an existing destination;
- v1 structured validation under an insufficient memory limit;
- `--against` source mismatch as a structured `valid=false` result;
- truncated, tampered, trailing-data, and malformed archives;
- typed missing-archive operational failure;
- honestly partial sampled analysis;
- Compare Quick and Compare Full, with both DataPack and standalone-zstd
  restored SHA-256 checks;
- Compare cancellation; and
- Compare missing-input failure.

Compare timings and winners are factual observations from the requested run.
P6 does not turn Compare into a marketing score or a universal performance
claim.

### 3.5 Release CLI

The optional `--cli` path covers:

- `datapack --version` and help;
- `analyze --json`;
- `compress`;
- `validate --against ... --json`;
- `decompress` and independent byte equality;
- safe overwrite rejection;
- negative validation;
- missing-input runtime failure;
- `compare --mode quick --runs 1 --json`;
- stable `error[code]: context` rendering; and
- JSON/data stdout cleanliness with diagnostics on stderr.

The final local run used
`$HOME/.cache/datapack-modernization/release/datapack`, built with the certified
Rust/Cargo 1.85.0 toolchain and shared `CARGO_TARGET_DIR`.
Expected overwrite and missing-input failures use the established P5 runtime
exit semantics; P6 does not change exit-code policy to satisfy harness
assumptions.

## 4. Installed-wheel certification

`scripts/certify_python_wheel.py` copies both the existing installed-
distribution test and the P6 harness into its temporary isolation workspace.
After installing one ABI3 wheel with `--no-index --no-deps`, it runs:

1. the five installed public-SDK tests; then
2. `technical_beta_ii.py --profile ci --forbidden-source-root <checkout>`.

The isolated interpreter runs with `-I`; the repository is absent from the
working directory and `sys.path`; Rust, Cargo, maturin, `PYTHONPATH`, and
`PYTHONHOME` are absent from the runtime environment. The P6 harness is a
certification input, not a wheel member, so the exact eight-member runtime
wheel allowlist remains unchanged.

The existing hosted topology needs no P6-specific job. The P6 CI profile runs
inside all 12 wheel-test jobs:

- Linux and Windows ABI3 wheels;
- CPython 3.9, 3.10, 3.11, 3.12, 3.13, and 3.14; and
- one wheel built per platform and reused across its six interpreters.

The beta-only width/record probes, release CLI path, and external 16 GiB test
remain manual evidence. Hosted logs contain the P6 JSON; the workflow uploads
only the already-authorized temporary wheel artifacts and publishes nothing.

## 5. Final local certification evidence

The final executable P6 tree was certified in WSL on 2026-09-08. On 2026-09-09,
continuation recovered the retained logs, confirmed the interrupted package
command had completed with exit status 0, and finalized documentation without
rerunning the completed gates. No required local gate remains unknown or failed.

| Gate | Local result | Evidence |
| --- | --- | --- |
| Rust/Cargo baseline | **PASS** | `rustc 1.85.0`, `cargo 1.85.0`; shared modernization build cache |
| Core formatting/check/tests/strict Clippy/release | **PASS** | `bash scripts/check.sh`; locked builds, all-target/all-feature `-D warnings`; 382 tests, zero failures |
| Binding formatting/check/test-build/strict Clippy | **PASS** | Separate `python/Cargo.toml`; locked gates; zero binding Rust unit tests; release compilation included in wheel build |
| Source/product version consistency | **PASS** | `scripts/check_version_consistency.py`; `0.1.0`, `datapack-engine`, `datapack` |
| Python grammar/lint/format | **PASS** | Seven Python/stub files parse with Python 3.9 grammar; Ruff 0.12.12 check and format check with target `py39` |
| Typing and signatures | **PASS** | Installed PEP 561 marker and stub; all five operation signatures/defaults match the installed runtime |
| SDK suite | **PASS** | 15 tests against the newly installed wheel, zero failures |
| Current Linux ABI3 wheel build/hygiene | **PASS** | maturin 1.14.1, Zig, manylinux2014 auditwheel check, exact eight-member runtime allowlist, remapped build paths |
| Isolated wheel certification | **PASS** | Five installed-distribution tests on CPython 3.14.4; `-I`, wheel-only `--no-index --no-deps` install outside the checkout, Rust/Cargo/maturin absent from runtime `PATH` |
| P6 CI profile | **PASS** | Existing wheel-certifier integration; five frozen input sizes, SHA-256 values, and planner modes unchanged; six workload paths, five format cases, 12 edge cases, and all operational/resource/Compare cases |
| P6 full manual-beta profile | **PASS** | Same final harness and wheel; six workload paths, five format cases, 14 edge cases, including 4,097 columns and the oversized-record probe; forced v2 ETL path has 249 chunks |
| Release CLI | **PASS** | Version/help, clean JSON stdout, compression/validation/decompression, exact roundtrip, overwrite rejection, Compare, stable errors, stream separation; expected failure exits remain 1 |
| `cargo deny` core/binding | **PASS; EXPECTED WARNING** | cargo-deny 0.20.2; existing unmatched-license and duplicate-`syn` warnings only |
| `cargo audit` core/binding | **PASS; EXPECTED WARNING** | cargo-audit 0.22.1; only accepted `RUSTSEC-2025-0141` for bincode 1.3.3 |
| `cargo package --locked --allow-dirty` | **PASS** | Packaged 168 files and passed verification build; retained command completion confirmed, exit 0 |
| Protected V1/V2 compatibility | **PASS** | Four compatibility tests; all six frozen fixture sizes and SHA-256 values unchanged |
| Supplemental 16 GiB evidence review | **PASS** | Existing analysis, validation, process logs, saved SHA-256 records, and archive/restored sizes inspected; dataset execution not repeated |

The local runtime evidence is Linux CPython 3.14.4. The P6 Linux/Windows and
CPython 3.9-3.14 hosted matrix passed in run `34404009535`, as recorded in
section 5.3. Windows local execution remains environment-limited; Windows
certification comes from that hosted run. Public distribution and P7 work
remain outside this certification.

### 5.1 Exact revision and retained evidence

The 1,358-line recovered harness was preserved and formatted with the prior
certification baseline, Ruff 0.12.12. The resulting 1,604-line file has an
identical Python AST. No generator, assertion, or behavior changed. CI and the
complete manual beta both ran after this final edit.

| Certification input/artifact | SHA-256 |
| --- | --- |
| Final `technical_beta_ii.py` | `5c1bf0337d2d493217932095fe1a53548e3f1992f7a8e4203d560d06a8888973` |
| Final `certify_python_wheel.py` | `5cba35090b3882343d3f47a5074fe463adc60e0ad223ad496b52afee4822fe5c` |
| Fresh Linux ABI3 wheel, 991,852 bytes | `d8bfd09b740b2e1fd3dbc8571b71dab143f181fc18f22783a2b4775c649eb146` |
| Final manual-beta JSON | `a80a42e82e0daea3ae3d73592a594291112de790c53d2197fc8c62de7a6156d5` |

The wheel filename is
`datapack_engine-0.1.0-cp39-abi3-manylinux_2_17_x86_64.manylinux2014_x86_64.whl`.
It was built from the current P6 tree. Its hash matches the P5 artifact because
P6 changes certification scripts and documentation, which are outside the
wheel; the retained build log establishes fresh build provenance.

Logs and artifacts remain outside Git in the temporary certification directory
`datapack-p6-certification.9cRCjT`: `rust.log`, `binding.log`, `security.log`,
`wheel-build.log`, `wheel-certification.log` (including the CI JSON),
`python-sdk.log`, `beta.json`, `beta.log`, and `package.log`. The retained harness
copy matches the source hash above. Temporary files are local evidence and are
not a durable hosted archive; no datasets, wheels, or logs are committed.

### 5.2 Earlier interruptions and classifications

Earlier session-only reports are superseded by the retained final local runs.
Windows Application Control, WSL `E_ACCESSDENIED`, and the managed sandbox's
read-only Cargo cache were environment problems, not product defects. Approved
WSL cache access allowed the final gates to complete. Earlier stale-wheel,
CLI-path, and exit-expectation mistakes were harness/environment issues.

An initially selected newer Ruff version differed from the prior certification
baseline. Restoring Ruff 0.12.12 and formatting the harness resolved the tooling
and formatting issues without changing lint policy or executable behavior.
Expected rejection, cancellation, invalid-archive, resource-limit, and
unsupported structured-analysis outcomes passed their existing assertions.
No engine, planner, wire-format, or public-API defect was demonstrated.

### 5.3 Hosted certification closure

Hosted CI run
[`34404009535`](https://github.com/Gom-svg/Datapack/actions/runs/34404009535)
completed with result **SUCCESS** on `productization/foundation` at technical
commit `621574a2607e3857fe932dbe9db8ec6eddc5a9fd`. Run status and all job
conclusions were inspected without rerunning certification. All 18 required
hosted jobs passed:

| Hosted coverage | Jobs | Result |
| --- | ---: | --- |
| Rust 1.85 on Linux and Windows | 2 | **PASS** |
| Python SDK foundation | 1 | **PASS** |
| Linux GNU x86_64 and Windows MSVC x86_64 ABI3 wheel builds | 2 | **PASS** |
| Dependency policy, cargo audit, and cargo package | 1 | **PASS** |
| Isolated Linux wheel certification, CPython 3.9-3.14 | 6 | **PASS** |
| Isolated Windows wheel certification, CPython 3.9-3.14 | 6 | **PASS** |
| Total | 18 | **18/18 PASS** |

The 12 isolated wheel jobs include the installed SDK and P6 CI profile. The
manual-beta profile, release CLI, and supplemental 16 GiB run retain their
local evidence scope. Local certification and hosted certification have both
passed: P6 Technical Beta II is **CERTIFIED — CLOSED**.

P6 required no engine semantic change, planner semantic change, V1/V2 wire
change, protected hash change, public API change, dependency change, or CI
topology change.

P7 — Release Artifacts / Release Engineering is next; it has not begun. This
documentation-only closure does not change product behavior or authorize
publication. DataPack is not yet production-ready, and performance measurements
remain observational.

## 6. Supplemental real-world 16 GiB evidence

This section records an operator-performed manual run outside the repository.
It is not a deterministic CI fixture, and the dataset is not copied into Git.
The final WSL certification session inspected the existing `analyze.json`,
`validate.json`, compression log, validation/decompression process logs, all
three saved SHA-256 records, and archive/restored file sizes. They agree with
the facts below. The large-file run and full-file hashing were not repeated;
the independent `cmp --silent` PASS retains its operator-reported provenance.

### 6.1 Source and analysis

| Fact | Result |
| --- | --- |
| Dataset | `HI-Large_Trans.csv` |
| Source bytes | 17,052,760,651 (approximately 15.88 GiB) |
| Source SHA-256 | `d13635e297c64673826217631fb88d635c4f506052e1bde833895eda2a65c3f2` |
| Detected format / delimiter | CSV / comma |
| Analysis scope | Sampled, partial |
| Records / bytes analyzed | 10,000 / 979,049 |
| Planner selection | `csv_columnar_dictionary` |
| Reason | `HIGH_REPETITION_DETECTED`: high repetition in 9 of 11 columns |
| Estimated savings | 13.583547% |
| Estimated dictionary memory | 3,420.8477 MiB (approximately 3.34 GiB) |
| Diagnostics | `SAMPLE_RECORD_LIMIT_REACHED`, `DUPLICATE_COLUMN_NAME`, and two `CARDINALITY_LIMIT_REACHED` diagnostics |

The full structured path was deliberately **NOT RUN**. It did not fail. The
operator chose v2 bounded RawZstd because analysis was partial and the planner's
structured-memory estimate was large.

V1 structured compression is not described as wholly unbounded. Existing CLI
controls include `--max-dictionary-values` (default 65,535) and
`--max-dictionary-mb` (default 64 MiB per column). The accurate limitation is
that v1 whole-file structured compression does not provide the same global
memory-admission and bounded streaming model as v2 chunked RawZstd.

### 6.2 V2 compression

Configuration:

- archive version 2, chunked RawZstd;
- 64 MiB chunks;
- four threads and four in-flight chunks;
- 256 MiB memory-admission bound; and
- adaptive level disabled.

| Fact | Result |
| --- | ---: |
| Input bytes | 17,052,760,651 |
| Archive bytes | 3,541,994,339 |
| Compression ratio | 4.8145x |
| Storage reduction | 79.23% |
| Chunk count | 255 |
| zstd level distribution | level 3: 255 chunks |
| Archive SHA-256 | `f6d722ad2cd11e4ff5509169d8a1821090d8a0e57d2295e1a9577c1b2b595ad9` |
| Exit status | 0 |

Observed profile values were 123,005 ms read, 19,040 ms hash, 71,566 ms
compress, 2,791 ms write, 132,807 ms pipeline elapsed, and 132,814 ms total.
`/usr/bin/time` observed 2:12.82 wall clock, 214,412 KiB maximum RSS
(approximately 209 MiB), zero swaps, and approximately 122.449 MB/s.

These process measurements are **OBSERVATIONAL**. The source was read through
WSL from Windows NTFS, and a prior SHA-256 pass had already read it. They are not
a controlled benchmark or a universal throughput/memory claim.

### 6.3 Validation and physical roundtrip

`validate archive --against source` reported:

- `valid=true` and source status `matched`;
- archive version 2 and 255 chunks;
- passed header, metadata, payload-structure, decompression, restored-length,
  chunk-table, per-chunk SHA-256, global SHA-256, and trailing-data checks;
- no diagnostics; and
- exit status 0.

Validation timing (3:26.21 wall, 95,208 KiB peak RSS) is observational.

Physical decompression produced exactly 17,052,760,651 bytes. The restored
SHA-256 was
`d13635e297c64673826217631fb88d635c4f506052e1bde833895eda2a65c3f2`,
matching the source. An independent `cmp --silent` also passed, establishing
byte-for-byte equality. DataPack decompression observed 2:08.20 wall clock,
95,612 KiB peak RSS, and exit status 0; those process values are observational.
The separate approximately 18-minute `cmp` crossed Linux/NTFS filesystems and
is not attributed to DataPack.

### 6.4 Valid conclusions and limits

The run is evidence that v2 chunked RawZstd processed this real approximately
16 GiB CSV with bounded admission, complete v2 integrity validation, physical
decompression, matching SHA-256, and independent byte equality.

It does **not** demonstrate that:

- every 16 GiB CSV behaves similarly;
- DataPack is production-ready;
- v1 structured compression failed or would use exactly 3.34 GiB;
- v1 structured compression would produce a worse archive;
- the observed throughput is a scientific benchmark; or
- RawZstd should always be selected for large files.

The 3.34 GiB planner value is an estimate for an unexecuted structured
candidate; 209 MiB is observed process RSS for a different v2 path. They are
not an apples-to-apples comparison and do not prove a multiplicative memory
improvement.

The useful future-design observation is narrower: resource cost should be an
explicit planning input alongside theoretical compression attractiveness.
Global resource budgeting, adaptive scheduling, bounded/chunked structured
dictionaries, and hybrid decisions belong to a later v3/adaptive-compute
program. P6 implements none of them.

## 7. Known limitations and publication status

- The hosted P6 profile is intentionally smaller than the manual-beta profile.
- Hosted wheel jobs do not exercise the release CLI or beta-only hard-limit
  probes.
- The real-world 16 GiB dataset is external, non-redistributable evidence and
  is not a CI dependency.
- No external security audit or production readiness assessment has occurred.
- No timing regression threshold or universal performance claim is introduced.
- No PyPI, TestPyPI, crates.io, GitHub Release, tag, merge, or publication action
  is authorized or performed by P6.
