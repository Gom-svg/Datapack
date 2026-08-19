# DataPack modernization report

- Status: **complete through Phase 17; final local certification passed**
- Report date: 2026-08-18 (America/Costa_Rica)
- Initial certification target: `7a46d47074f40b8279fc8b4c86705d60d233862c`
- Second-recovery recertification target: `23f875bc06e7c883fb5cc72b4abe76075f7cc05e`
- Phase 16 tag: `rfc-016-performance-regression-suite`
- Phase 17 tag: `rfc-017-dpack-v3-design`
- Certified toolchain: Rust/Cargo 1.85.0
- Repository action: local commits and annotated tags only; nothing pushed

## 1. Executive outcome

The DataPack modernization program is complete through its authorized scope.
The repository now has:

- frozen and continuously checked `.dpack` v1/v2 compatibility;
- a bounded shared analysis core and deterministic planner contract;
- structured analysis and safe v1 compression for comma, semicolon, tab, and
  pipe data;
- a bounded v2 RawZstd pipeline with transactional output and per-chunk/global
  integrity;
- versioned analysis, validation, comparison, advisor, and Application API
  reports;
- a thin Python SDK that delegates to the Rust Application API;
- pinned CI, dependency, package, and repository-hygiene policy;
- a deterministic correctness/performance regression harness that keeps
  wall-clock measurements observational; and
- a detailed `.dpack` v3 structured-chunk design with no v3 implementation.

Final local certification passed every executed core, binding, dependency,
package, compatibility, and performance-correctness gate. No protected archive
hash changed. No v1/v2 wire semantic changed. No generated benchmark dataset,
archive, restored output, JSON result, wheel, or multi-gigabyte external source
was added to Git.

### Evidence status vocabulary

This report uses the following labels consistently:

| Label | Meaning in this report |
| --- | --- |
| **IMPLEMENTED** | Executable behavior is present in the repository. |
| **CERTIFIED** | The stated local correctness, compatibility, package, or policy gates passed at the named target. |
| **EXPERIMENTAL** | Executable but non-default behavior with narrower evidence and no production-readiness claim. |
| **OBSERVATIONAL** | Environment-specific evidence, including wall-clock measurements, that is not a deterministic failure threshold. |
| **DESIGN ONLY** | A reviewed design boundary exists, but executable behavior is intentionally absent. |
| **DEFERRED** | Work is outside this modernization scope and requires later authorization and acceptance gates. |
| **NOT RUN** | A gate was configured or identified but was not executed by the local modernization session. |

## 2. Power-loss recovery provenance

The program resumed after an unexpected workstation power loss. Recovery was
performed before new implementation work and without reset, restore, checkout,
clean, stash, rebase, deletion, or regeneration.

The last known Phase 15 checkpoint was:

- commit `8aae565afedbe4d4373211913b6304a383d986c2`;
- tag `rfc-015-documentation-truth-pass`; and
- `main...origin/main [ahead 35]` at that checkpoint.

The recovered repository was farther ahead than the Phase 15 baseline:

- HEAD was `5b174c397042eaf8cba2b8116370bb903cf3067f`;
- branch state was ahead 36, behind 0;
- commit `5b174c3 feat: add performance regression harness` was coherent Phase 16
  work;
- the only worktree item was the complete untracked
  `docs/reports/PHASE-16-PERFORMANCE-OBSERVATION.md`; and
- the Phase 16 tag did not yet exist.

The recovered-state classification was **D: Phase 16 was already committed
before shutdown but had not been tagged**. The benchmark executable, shared
support module, tests, methodology, RFC, and observation report had complete
endings and no evidence of truncation. Work resumed from that commit and
untracked report; neither was replaced.

Read-only recovery artifacts remain outside the repository:

| Artifact | Bytes | SHA-256 |
| --- | ---:| --- |
| `/tmp/datapack-power-recovery-status.txt` | 50 | `afb22dd0dda0726c5d74316a3532e5c0c3786b3200f96884660dd47aab59756c` |
| `/tmp/datapack-power-recovery.patch` | 0 | `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |
| `/tmp/datapack-power-recovery-stat.txt` | 0 | `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |
| `/tmp/datapack-power-recovery-log.txt` | 1,389 | `d2e0d9d2480dc068bcea989cd5a40908ed881dde2fbf0672a9ebc6ee31c63199` |
| `/tmp/datapack-recovered-PHASE-16-PERFORMANCE-OBSERVATION.md` | 5,481 | `28f2bdfbfe7b30a076d3c55cf943cbd0bc350154c5629b0f2f6f67d588aedf7a` |

The empty patch/stat snapshots are expected: tracked Phase 16 work had already
been committed, while the report was untracked and separately preserved byte
for byte.

### Second power interruption recovery

The workstation lost power again after the first recovery had completed Phase
16, Phase 17, and the initial modernization report. The second forensic pass
found:

- HEAD `23f875bc06e7c883fb5cc72b4abe76075f7cc05e` on `main`, ahead 39 and behind 0;
- a clean worktree with no modified, deleted, or untracked files;
- annotated Phase 16 and Phase 17 tags still pointing at their certified
  commits;
- complete Phase 16 source, tests, methodology, RFC, and observation report;
- a complete documentation-only Phase 17 RFC with no v3 implementation; and
- no truncated file, merge residue, unexpected commit, or unrelated user
  change.

The recovered-state classification was **G: modernization appears fully
completed locally**. Work therefore resumed from `23f875b` at repository-wide
recertification rather than recreating or replacing any prior work.

The second-recovery snapshots remain outside the repository:

| Artifact | Bytes | SHA-256 |
| --- | ---:| --- |
| `/tmp/datapack-second-recovery-status.txt` | 33 | `094dfdf6491ed66a98f49dffc424c3ee51a624188bdd85137f03492d90be6c41` |
| `/tmp/datapack-second-recovery.patch` | 0 | `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |
| `/tmp/datapack-second-recovery-stat.txt` | 0 | `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |
| `/tmp/datapack-second-recovery-log.txt` | 2,072 | `8e8b468524c9e0c41feb4f0a7a5535c5f96307439f692e4ffd9fe5cf69224de1` |
| `/tmp/datapack-second-recovery-commits.txt` | 1,770 | `3d45f8154aedf41e1acf77d97aef156052f95a447f558f109b424d868daa1301` |

## 3. Modernization phase record

The pre-phase frozen compatibility baseline is tagged
`rfc-001b0-baseline`. The numbered program then completed these phases:

| Phase | Tag | Outcome |
| ---:| --- | --- |
| 1 | `rfc-001b-compatible-analysis-core` | Characterized legacy behavior, extracted compatible factual analysis, and retained PlannerPolicyV1 authority. |
| 2 | `rfc-002-cli-modularization` | Separated CLI command adapters/rendering without changing the public command surface or wire formats. |
| 3 | `rfc-003-analysis-json` | Added deterministic, privacy-safe Analysis JSON V1 with explicit scope and reason semantics. |
| 4 | `rfc-004-bounded-analysis` | Enforced byte, record, column, cardinality, and accounted-memory bounds with truthful partial facts. |
| 5 | `rfc-005-delimited-engine` | Added one crate-private byte-oriented delimited scanner and compatibility adapters. |
| 6 | `rfc-006-delimited-analysis` | Integrated quote-aware comma/semicolon/tab/pipe detection into analysis while preserving legacy contracts. |
| 7 | `rfc-007-structured-delimited-compression` | Safely enabled the existing v1 DCSV01 route for four delimiters with RawZstd fallback. |
| 8 | `rfc-008-planner-encoder-contract` | Made the planned Plain/Dictionary projection executable and bounded without inventing new v1 modes. |
| 9 | `rfc-009-archive-validation` | Added read-only typed validation for v1/v2, including truthful unavailable-v1 guarantees. |
| 10 | `rfc-010-datapack-compare` | Added factual full/quick comparison with per-run SHA-256 artifact and restoration checks. |
| 11 | `rfc-011-datapack-advisor` | Added deterministic advice projected only from existing facts and planner policy. |
| 12 | `rfc-012-public-rust-application-api` | Established typed synchronous Analyze/Compress/Decompress/Validate/Compare/Benchmark services. |
| 13 | `rfc-013-python-sdk-foundation` | Added a thin PyO3/ABI3 SDK over the Rust Application API with no duplicate core logic. |
| 14 | `rfc-014-ci-repository-hygiene` | Pinned Rust 1.85, added hosted CI/policy/package gates, and removed generated targets from the Git index. |
| 15 | `rfc-015-documentation-truth-pass` | Reconciled documentation with implemented architecture, evidence, and product boundaries. |
| 16 | `rfc-016-performance-regression-suite` | Added representative deterministic correctness gates plus environment-complete observational timings. |
| 17 | `rfc-017-dpack-v3-design` | Completed a concrete v3 structured-chunk design only; no executable v3 semantics were added. |

## 4. Current certified architecture

### Implemented formats

`.dpack` v1 remains the established single-payload container:

- RawZstd can stream with bounded memory;
- DCSV01 structured compression supports the established four delimiters;
- structured v1 remains whole-file and is not presented as the large-file
  bounded architecture; and
- v1 has no stored per-chunk/global SHA-256 or exact trailing-data guarantee.

`.dpack` v2 remains the established fixed-header/fixed-table chunked RawZstd
container:

- bounded reader, persistent workers, ordered writer, and explicit in-flight
  limit;
- transactional compression/decompression;
- deterministic chunk/table/payload ordering for the promised build scope;
- per-chunk and global SHA-256 verification by default; and
- default DataPack chunk workers plus an explicit experimental native-zstd-MT
  backend that does not change archive interpretation.

The Rust Application API is the reusable service boundary. CLI and Python are
adapters; neither parses service text nor reimplements planners, codecs,
validation, comparison, or fallback.

### Analyze, Facts, Policy, and service boundaries

The **IMPLEMENTED** intelligence path separates observation from decision:
Analyze and the Delimited Data Engine produce bounded factual models;
`DatasetFacts` and coverage/limit diagnostics retain what was actually
observed; and `PlannerPolicyV1` alone maps those facts into the established v1
execution choices. The Advisor is a deterministic projection of those facts
and policy outcomes, not another planner. The encoder validates the executable
plan and safely falls back rather than inventing policy.

The Delimited Data Engine is the shared byte-oriented, quote-aware scanner for
comma, semicolon, tab, and pipe inputs. Validate provides read-only typed v1/v2
structure and integrity evidence. Compare provides factual DataPack versus
standalone-zstd measurements with complete per-run identity checks. The public
Rust Application API supplies Analyze, Compress, Decompress, Validate,
Compare, and Benchmark services to both CLI and Python adapters. The Python SDK
is an **IMPLEMENTED** ABI3 foundation over those Rust services, not a duplicate
compression stack and not a published PyPI product.

Security is **CERTIFIED** for the documented local scope through bounded
archive-controlled allocation, checked arithmetic, transactional output,
corruption/mutation coverage, default v2 hashes, resource-limit tests, and
dependency policy checks. This is not a claim of a completed external security
audit.

### Designed but not implemented

[RFC-007](../rfcs/RFC-007-dpack-v3-structured-chunk-design.md) defines the
future v3 boundary:

- a proposed fixed header plus bounded stripe/block manifest;
- record-aware stripe boundaries;
- exact raw-lexeme reconstruction;
- explicit RawBytes, Plain, and Dictionary transforms with RawZstd fallback;
- two-pass source identity and deterministic executable planning;
- payload, stripe, control-plane, and global integrity checks;
- transactional bounded decode and validation; and
- staged future security, fixture, fuzz, compatibility, and performance gates.

There is no v3 constant, reader, writer, validator, CLI option, Application API
variant, Python surface, fixture, or archive. V1 and v2 remain the only
implemented versions.

## 5. Phase 16 performance/regression evidence

The Phase 16 harness uses the repository's deterministic generator and public
Application API. It does not parse CLI output or implement another compressor,
planner, validator, or comparer.

Its five categories are:

- repetitive structured v1;
- realistic structured v1;
- high-cardinality v1;
- random/low-redundancy v1; and
- realistic v2 multichunk.

For correctness, it gates source size/SHA-256, version/mode, same-run archive
size and SHA-256 stability, restored size/SHA-256/bytes, validation against the
source, format-specific structure, and v2 chunk/global/trailing checks. Elapsed
times and throughput are serialized as `observational_only` and never compared
with a failure threshold.

### Representative observation

The recovered Phase 16 report records an optimized three-run observation on
WSL Linux-native `/tmp`, not `/mnt/c`/NTFS. Its original JSON disappeared with
the power event, so the suite was rerun at the same implementation commit. The
re-certification JSON is
`/tmp/datapack-phase16-recovery-representative.json`, SHA-256
`c7849eea5460ef5e6ca4c91e1bbf706e6f4195c5d50cfdad56088c2467476b88`.
All deterministic source and archive facts reproduced exactly.

| Scenario | Source bytes | Archive bytes | Ratio | Chunks | Original observed compression/decompression median ms |
| --- | ---:| ---:| ---:| ---:| ---:|
| repetitive structured | 2,721,159 | 138,179 | 19.6930x | n/a | 103.137 / 8.298 |
| realistic structured | 3,868,893 | 445,816 | 8.6782x | n/a | 114.084 / 8.155 |
| high-cardinality | 4,511,021 | 2,264,590 | 1.9920x | n/a | 45.031 / 6.487 |
| random/low-redundancy | 3,422,884 | 2,600,636 | 1.3162x | n/a | 37.160 / 6.872 |
| realistic v2 multichunk | 11,609,824 | 2,233,926 | 5.1970x | 12 | 17.715 / 32.990 |

These timings apply only to the recorded build, machine, WSL mode, `/tmp`
placement, cache state, and load. They are not regression thresholds or
cross-platform claims. Complete detail is in the
[Phase 16 observation](PHASE-16-PERFORMANCE-OBSERVATION.md).

### Final clean-HEAD smoke

The final certification target ran the release bench profile with two runs for
all five smoke scenarios on WSL Linux-native `/tmp`:

- timestamp: `2026-08-18T03:13:04Z`;
- commit: `7a46d47074f40b8279fc8b4c86705d60d233862c`;
- Git dirty state: false;
- JSON: `/tmp/datapack-final-modernization-smoke.json` (not tracked);
- JSON SHA-256:
  `142d02d66d0e82915adb60dc8c357a6de74496a04367c3f3ad780393d668a855`;
- stable archive size and SHA-256 across both runs: pass for every scenario;
- byte-exact/source-SHA/against-source validation: pass for every scenario;
  and
- v2 19-chunk table, per-chunk SHA, global SHA, and trailing-data checks: pass.

## 6. Separate external Python beta evidence

Historical benchmark tables and the historical 7,392,492,161-byte external
run remain **OBSERVATIONAL** evidence from their recorded environments. That
source was unavailable and the run was **NOT RUN** during either recovery. Its
timing is not an official Phase 16 baseline, and Phase 16 neither rewrites the
historical values nor depends on the external source.

External Python Beta Test 001 remains separate functional/integrity evidence.
It is **OBSERVATIONAL** external beta evidence, not a repository fixture and
not part of CI.

| Fact | Value |
| --- | --- |
| Dataset | `yellow_tripdata_2016-01.csv` |
| Input bytes | 1,708,674,492 |
| V2 archive bytes | 360,142,313 |
| Ratio | 4.7444x |
| Space reduction | 78.92% |
| Chunks | 26 |
| Validation | PASS |
| Against original | MATCHED |
| Original SHA-256 | `fb785a71d2bc82e6480dc7fd63bcfc4632e9b94af643baefdc736fbbf234aca7` |
| Restored SHA-256 | `fb785a71d2bc82e6480dc7fd63bcfc4632e9b94af643baefdc736fbbf234aca7` |
| Independent byte comparison | PASS |

The 1.59 GiB source is not in the repository. Its dev-profile wheel, WSL,
`/mnt/c`, NTFS, and OneDrive conditions make its timings uncontrolled and
unsuitable for comparison with the optimized `/tmp` observations.

## 7. Final local certification

Certification ran against clean commit `7a46d47` with Rust/Cargo 1.85.0.

| Gate | Result |
| --- | --- |
| `cargo fmt --all -- --check` | PASS |
| `cargo check --locked` | PASS |
| `cargo test --locked` | PASS — 359 tests, 0 failures; 12.78 s wall time |
| `cargo clippy --all-targets --all-features --locked -- -D warnings` | PASS |
| `cargo build --release --locked` | PASS |
| release `performance_regression`, smoke, two runs | PASS — all correctness and stability gates |
| Python binding `cargo fmt --check` | PASS |
| Python binding `cargo check --locked` | PASS |
| Python binding `cargo test --locked` | PASS |
| Python binding strict Clippy | PASS |
| Python 3.14 ABI3 wheel build/install | PASS — maturin 1.14.1, CPython stable ABI ≥3.9 |
| installed-package Python tests | PASS — 4 tests, 0 failures |
| `cargo deny check` for core and Python manifests | PASS |
| `cargo audit` for core and Python lockfiles | PASS with one documented allowed warning |
| `cargo package --locked` | PASS — 148 files, 1.5 MiB unpacked, 358.8 KiB compressed |
| frozen v1/v2 fixture sizes and SHA-256 | PASS |
| `git diff --check` | PASS |

`cargo deny` retained non-failing policy warnings for an unused allowed license
and the Python lockfile's two `syn` major versions. `cargo audit` reported only
`RUSTSEC-2025-0141`, the documented warning that bincode 1.3.3 is unmaintained;
no vulnerability was reported.

### Second-recovery recertification

The second recovery reran certification against clean report commit `23f875b`
with Rust/Cargo 1.85.0 and Python 3.14.4:

| Gate | Result |
| --- | --- |
| `cargo fmt --all -- --check` | PASS |
| `cargo check --locked` | PASS |
| `cargo test --locked` | PASS — 359 tests, 0 failures; 20.35 s wall time including incremental compilation |
| strict all-target/all-feature Clippy | PASS |
| `cargo build --release --locked` | PASS |
| release `performance_regression`, smoke, two runs | PASS — all five scenarios; all correctness and stability gates |
| second-recovery smoke JSON | **OBSERVATIONAL** — `/tmp/datapack-second-recovery-smoke-23f875b.json`, SHA-256 `97d57191841a04d527a2b8fa2881a1abf917568936a59ba5648b87f6682419d0` |
| Python binding format/check/test/strict-Clippy | PASS |
| isolated maturin wheel build/install | PASS — maturin 1.14.1, Python 3.14.4, ABI3 for Python >=3.9 |
| installed-package Python tests | PASS — 4 tests, including v1, v2 multichunk, validation, byte equality, and typed failures |
| core and Python `cargo deny check` | PASS with the same documented policy warnings |
| core and Python `cargo audit` | PASS with only allowed `RUSTSEC-2025-0141` |
| `cargo package --locked` | PASS — 149-file package contents verified and packaged crate compiled successfully |
| protected fixture sizes and SHA-256 | PASS — all six values unchanged |
| tracked-file/generated-artifact review | PASS — largest tracked file about 71 KiB; no generated benchmark result or wheel tracked |
| `git diff --check` | PASS |

The second-recovery release observation ran in WSL on Linux-native `/tmp` and
is not compared with `/mnt/c`, NTFS, OneDrive, native Windows, or another
machine as an algorithmic delta.

Hosted GitHub CI is **CONFIGURED BUT NOT RUN ON THE LOCAL MODERNIZATION
COMMITS**. No hosted-CI success is claimed. The Linux/Windows workflow and its
read-only permissions remain committed for execution on push or pull request.

## 8. Protected compatibility evidence

The frozen compatibility manifest and files retain their exact bytes:

| File | Bytes | SHA-256 |
| --- | ---:| --- |
| `v1_raw_zstd_source.csv` | 810 | `81f5cc20f74dd9bea672f7bdbb5ecc1bf5fdb5379223c4af8c2cb650f54fb994` |
| `v1_raw_zstd.dpack` | 331 | `7d467f55a0927c0fafc0a75c1fdf7be5f7ad2db430a8bee65dc9312c71ddadab` |
| `v1_csv_columnar_source.csv` | 1,269 | `320c19793eea868aa109cb798887f92394de114879a3d5df3ed55fb1f49d16bf` |
| `v1_csv_columnar.dpack` | 265 | `0210e1b969e124731cbe374fb030fc75e391949e588195b676b2a7a2b36070a5` |
| `v2_chunked_source.bin` | 588 | `f41051e6c9fcbf543208b67f26116e0f11188d997d52815f7ee24444f643f8f3` |
| `v2_chunked_multichunk.dpack` | 1,542 | `a0644b941c86dac0c0d0ccd37e8cd6f07fdd25d45f38672a0d0c626faa5f8cbd` |

The four compatibility-fixture tests passed, including byte-exact v1 RawZstd,
v1 DCSV01 reproduction, v2 10-chunk restoration, and transactional rejection
of corrupted copies.

## 9. Repository hygiene and change scope

From the Phase 15 checkpoint through the Phase 17 certification target, the
repository changed 11 paths with 2,367 insertions and 22 deletions. The only
removed source was the authorized historical `benches/size_placeholder.rs`.
Phase 16 added the real harness/support/tests/docs; Phase 17 changed
documentation only.

The largest tracked file is about 71 KiB. No tracked file exceeds 10 MiB, and
no generated source dataset, `.dpack`, restoration, JSON benchmark output,
wheel, or target directory was added. Build outputs used external cache paths
under `/home/gompr/.cache` or isolated `/tmp` directories.

An old ignored `fuzz/target/` build cache still exists locally, including files
larger than 10 MiB. It is not tracked, was already governed by Phase 14's ignore
policy, and was intentionally preserved rather than destructively cleaned
during recovery.

Immediately before this final report was created:

- HEAD was `7a46d47` and tagged `rfc-017-dpack-v3-design`;
- `main...origin/main` was ahead 38 and behind 0;
- the worktree was clean; and
- no push had occurred.

This report is the only report-only change after that certified clean target.

## 10. Accepted limitations and future gates

- Bincode 1.3.3 remains required to read the frozen v1 metadata graph. Its
  unmaintained advisory is explicit, not hidden, and replacement would require
  a compatibility design rather than a casual dependency bump.
- V1 cannot provide archive-native global/per-chunk SHA-256 or exact
  trailing-data guarantees. Use `validate --against` for end-to-end identity.
- V1 structured encoding remains whole-file. V2 is the current bounded
  large-file mode and remains RawZstd-only.
- Native zstd MT remains experimental and non-default.
- Python remains a checkout-built SDK foundation, not a claim of published
  production packaging.
- Performance observations remain environment-specific and non-gating.
- Hosted CI results must be reported only after GitHub executes the workflow.
- V3 implementation requires new authorization and the staged fixture,
  resource, security, fuzz, compatibility, API, and observational evidence in
  RFC-007. Phase 17 itself provides no executable v3 behavior.
- Desktop, GPU/hybrid compute, SaaS/cloud, PyPI publication, and executable v3
  work are **DEFERRED** to a later productization program. No CUDA, NVIDIA,
  AMD-specific, or other hardware-specific archive variant is authorized.

## 11. Final verdict

The authorized modernization is locally certified and internally coherent.
Correctness and integrity remain hard gates; performance remains contextual
evidence. V1/v2 compatibility is protected by immutable hashes and executable
tests. The reusable Rust Application API is the source of truth for CLI and
Python consumers. Phase 16 supplies a non-flaky regression/observation
framework, and Phase 17 supplies a bounded, version-isolated v3 design without
crossing into implementation.

No stop condition was triggered, and nothing was pushed.
