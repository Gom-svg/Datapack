# DataPack Productization Report

Program: DataPack Productization Foundation

Branch: `productization/foundation`

Modernization status: **CERTIFIED — CLOSED**

## Evidence vocabulary

This report uses only these status labels:

- **IMPLEMENTED** — present in the repository with direct evidence;
- **CERTIFIED** — required evidence and relevant certification gates passed;
- **EXPERIMENTAL** — implemented only for explicitly limited evaluation;
- **OBSERVATIONAL** — measured fact without a universal or gating claim;
- **DESIGN ONLY** — documented design with no executable behavior;
- **DEFERRED** — intentionally outside the current completed scope; and
- **NOT RUN** — a check or activity has not been executed.

## Certified base

| Evidence | Status | Record |
| --- | --- | --- |
| Stable baseline | **CERTIFIED** | `e782ea8c361f89e3aa111fe2e15e916c127fc737` |
| Modernization checkpoint | **CERTIFIED** | `modernization-complete-2026-08-23` at `6ddc50f695b918e1f6c7daaeedd8d3674ef753d7` |
| Hosted technical CI | **CERTIFIED** | run `32681781845` |
| Hosted final documentation CI | **CERTIFIED** | run `32683577404` |
| Rust toolchain | **CERTIFIED** | Rust/Cargo 1.85.0 |
| Rust test suite | **CERTIFIED** | 359 tests, zero failures at modernization closure |
| Archive compatibility | **CERTIFIED** | frozen v1/v2 fixtures, sizes, SHA-256 values, and byte-exact guarantees |
| `.dpack` v3 | **DESIGN ONLY** | no reader, writer, validator, option, fixture, or executable semantics |
| GPU/hybrid compute | **DEFERRED** | no implementation authorized in Productization Foundation |

## Productization phase ledger

| Phase | Status | Evidence / blocker |
| --- | --- | --- |
| P0 — Productization Readiness Audit | **CERTIFIED** | `docs/productization/PRODUCTIZATION-READINESS-AUDIT.md` records the read-only baseline audit. Full local Rust certification and protected-fixture verification passed before the logical checkpoint commit. |
| P1 — Release Foundation | **CERTIFIED** | Release/version, identity, compatibility, channel, platform, Python/MSRV, changelog/release-note, artifact, manifest, and checksum policies are implemented and locally certified. |
| P2 — Python Distribution Foundation | **IMPLEMENTED** | Wheel-first `datapack-engine` packaging, manylinux2014 policy, exact artifact inspection, isolated installed-SDK testing, and build-once/test-six Linux/Windows CI are committed. Linux is locally certified; Windows and the hosted P2 matrix are explicitly not run at this unpushed checkpoint. Publication remains unauthorized. |
| P3 — Progress API | **DEFERRED** | Existing typed Rust foundation audited; no Productization change implemented. |
| P4 — Cooperative Cancellation | **DEFERRED** | No public cancellation capability exists. |
| P5 — Public API / Error Product Polish | **DEFERRED** | Existing surfaces audited; no Productization change implemented. |
| P6 — Technical Beta II | **DEFERRED** | No Productization beta-II methodology or run implemented. |
| P7 — Release Artifacts / Release Engineering | **DEFERRED** | No release artifact workflow implemented. |
| P8 — Desktop Foundation | **DEFERRED** | Desktop architecture and implementation remain deferred. |

## P0 outcome

The P0 audit is **CERTIFIED**. It found no reason to change the protected core,
wire formats, fixtures, security limits, or default P0–P8 phase order.

The exact distribution name `datapack` is already owned by unrelated projects on
both PyPI and crates.io. Selecting public Rust/Python distribution names or seeking
ownership transfer is a permanent external product decision. Work therefore stops
after P0 as required. No package was published, no release was created, no tag was
added or moved, and no push or merge was performed.

### P0 blocker resolution

After P0, the owner authorized DataPack as the product brand, `datapack` as the CLI
and Python import, `datapack-engine` as the future PyPI distribution, and
`datapack` as the internal Rust package. crates.io publication is deferred and out
of scope. This resolved the P1/P2 preparation blocker without authorizing registry
reservation or publication.

## P0 local certification

| Gate | Status | Result |
| --- | --- | --- |
| `cargo fmt --all -- --check` | **CERTIFIED** | PASS |
| `cargo check --locked` | **CERTIFIED** | PASS |
| `cargo test --locked` | **CERTIFIED** | PASS — 359 tests, zero failures |
| `cargo clippy --all-targets --all-features --locked -- -D warnings` | **CERTIFIED** | PASS |
| `cargo build --release --locked` | **CERTIFIED** | PASS |
| Protected fixture test | **CERTIFIED** | PASS — four compatibility tests |
| Protected fixture sizes and SHA-256 | **CERTIFIED** | PASS — all six values unchanged |
| `git diff --check` | **CERTIFIED** | PASS |
| Python wheel rebuild | **NOT RUN** | P0 changed documentation only; the certified hosted wheel evidence was audited without changing packaging. |

## P1 release foundation outcome

P1 is **CERTIFIED** locally. It preserves development version `0.1.0` and does not
create a release. The adopted model includes:

- Semantic Versioning for one shared package/application version;
- explicit alpha, beta, release-candidate, and stable-package channels;
- a pre-1.0 policy that still requires approval for breaking public changes;
- complete independence between package SemVer, `.dpack` v1/v2, and V1 report
  schema versions;
- immutable v1/v2 compatibility as a release-blocking requirement;
- a canonical `VERSION` file and automated Rust/Python/CLI/lockfile consistency
  gate;
- internal Rust crates marked non-publishable;
- Linux GNU x86_64 and Windows MSVC x86_64 as the initial CLI release targets,
  conditional on P7 artifact smoke certification;
- Linux x86_64 and Windows x86_64 ABI3 wheels as the initial Python artifact
  targets, conditional on P2 wheel certification;
- CPython 3.9 as the runtime/ABI floor, with current declared versions through
  CPython 3.14;
- Rust 1.85 as MSRV and exact Rust/Cargo 1.85.0 release certification toolchain;
- cumulative `CHANGELOG.md` plus versioned release-note templates; and
- deterministic artifact names, manifest fields, SHA-256 formatting, isolated
  smoke-test requirements, and an explicit reproducible-process boundary.

The temporary P1 wheel normalized the authorized distribution name to
`datapack_engine-0.1.0-cp39-abi3-manylinux_2_34_x86_64.whl`. It installed as
`datapack-engine`; `import datapack` and version `0.1.0` were preserved. This is
local packaging evidence, not a P2 Linux portability claim or a published artifact.

## P1 local certification

| Gate | Status | Result |
| --- | --- | --- |
| Product identity/version consistency | **CERTIFIED** | PASS — canonical and mirrored version `0.1.0`; brand/CLI/distribution/import/crate identities matched policy |
| CLI version behavior | **CERTIFIED** | PASS — `datapack 0.1.0`; Clap version equals `CARGO_PKG_VERSION` |
| `cargo fmt --all -- --check` | **CERTIFIED** | PASS |
| `cargo check --locked` | **CERTIFIED** | PASS |
| `cargo test --locked` | **CERTIFIED** | PASS — 359 tests, zero failures |
| Strict locked Clippy | **CERTIFIED** | PASS |
| `cargo build --release --locked` | **CERTIFIED** | PASS |
| Binding format/check/test/strict locked Clippy | **CERTIFIED** | PASS |
| ABI3 wheel build and isolated install | **CERTIFIED** | PASS — maturin 1.14.1, CPython 3.14.4, ABI3 floor 3.9 |
| Installed-package SDK tests | **CERTIFIED** | PASS — five tests, zero failures |
| `cargo deny` core and binding | **CERTIFIED** | PASS with the existing non-failing license/duplicate-version warnings |
| `cargo audit` core and binding | **CERTIFIED** | PASS with only allowed `RUSTSEC-2025-0141` |
| `cargo package --locked --allow-dirty` | **CERTIFIED** | PASS — 158 packaged files, 1.6 MiB unpacked, 388.6 KiB compressed |
| Protected fixture tests, sizes, and SHA-256 | **CERTIFIED** | PASS — four tests and all six immutable values unchanged |
| `git diff --check` | **CERTIFIED** | PASS |
| Hosted P1 CI | **NOT RUN** | No push was authorized or performed. |

## P1 change scope

P1 changes only release/packaging metadata, tests/gates, and documentation:

- `.github/workflows/ci.yml`;
- `CHANGELOG.md`;
- `Cargo.toml`;
- `README.md`;
- `VERSION`;
- `docs/productization/DATAPACK-PRODUCTIZATION-REPORT.md`;
- `docs/productization/PRODUCTIZATION-READINESS-AUDIT.md`;
- `docs/productization/RELEASE_ARTIFACTS.md`;
- `docs/productization/VERSIONING_AND_RELEASE_POLICY.md`;
- `docs/reference/CI_AND_REPOSITORY_POLICY.md`;
- `docs/reference/PYTHON_SDK_FOUNDATION.md`;
- `docs/releases/README.md`;
- `docs/releases/RELEASE-NOTES-TEMPLATE.md`;
- `python/README.md`;
- `python/pyproject.toml`;
- `python/tests/test_sdk.py`;
- `scripts/check.ps1`;
- `scripts/check.sh`;
- `scripts/check_version_consistency.py`; and
- `tests/cli_surface.rs`.

No engine, planner, codec, storage, archive parser/writer, protected fixture,
resource limit, progress, cancellation, or error implementation changed.

## P2 Python distribution outcome

P2 implements the binary distribution contract without changing the DataPack
engine. The distribution is `datapack-engine`, the import remains `datapack`,
and the version remains `0.1.0`. The PyO3 adapter still calls the Rust
Application API; no Python compression, decompression, analysis, validation,
comparison, planning, archive, or integrity implementation was added.

The initial audit distinguished build dependence from install dependence. The
binding crate's exact root path dependency requires a complete checkout for a
builder. The prebuilt wheel itself installed and executed outside the checkout
without Rust, Cargo, maturin, a repository path, or any Python runtime
dependency.

The audit also found that the P1 host-built Linux wheel was truthful only for
`manylinux_2_34_x86_64`, omitted the license file, and contained local absolute
paths in native panic-location data and maturin's generated path-dependency
SBOM. P2 resolves those distribution defects by:

- building the Linux artifact against glibc 2.17 with the manylinux2014 policy;
- pinning maturin 1.14.1 and retaining Rust/Cargo 1.85.0, PyO3 0.29.0, and
  `abi3-py39`;
- stripping the extension and remapping workspace/home source paths;
- excluding the generated Rust SBOM until its absolute path data can be
  sanitized;
- including an MIT license file that is automatically required to match the
  canonical root license; and
- enforcing an eight-member runtime wheel allowlist.

The locally produced Linux wheel has the exact filename:

```text
datapack_engine-0.1.0-cp39-abi3-manylinux_2_17_x86_64.manylinux2014_x86_64.whl
```

Its WHEEL metadata contains both `cp39-abi3-manylinux_2_17_x86_64` and
`cp39-abi3-manylinux2014_x86_64`. Auditwheel constrained it to glibc 2.17 and
reported only policy-provided `libc`, `libm`, `libpthread`, and `libdl` symbol
dependencies. No native library was grafted into the wheel. The final local
artifact is 966,326 bytes with SHA-256
`7e4d12293ba1bea8693d7bd23f7016c2163874337a06ce4b742e565d59d4e22d`.
The Windows artifact size and digest are **NOT RUN**.
An immediate same-environment rebuild was byte-identical by `cmp` and SHA-256;
this is **OBSERVATIONAL** evidence, not an independent-runner reproducibility
claim.

One unchanged Linux wheel was installed and exercised under CPython 3.9.25,
3.10.21, 3.11.16, 3.12.14, 3.13.15, and 3.14.7. Each interpreter ran three
installed-distribution tests covering identity/isolation, the complete v1
public workflow, and a v2 native-engine round trip: 18 executions, zero
failures.

The Windows build/test design produces
`datapack_engine-0.1.0-cp39-abi3-win_amd64.whl`, requires its actual WHEEL tag
to equal `cp39-abi3-win_amd64`, and reuses that single artifact across CPython
3.9 through 3.14. The local Windows build is **NOT RUN** because Windows
Application Control rejects `rustc.exe` with OS error 4551. The new hosted P2
jobs are also **NOT RUN** because this checkpoint is not pushed. No Windows
result is inferred from Linux.

The source-distribution audit is **DEFERRED**. Maturin could assemble a complete
sdist and an extracted copy could build without the original checkout, but the
result exposes a Rust/native-toolchain installation contract and normal PEP 517
construction produced a native `linux_x86_64` wheel rather than the certified
manylinux artifact. P2 therefore remains deliberately wheel-first.

See `docs/productization/PYTHON_DISTRIBUTION.md` for the exact platform,
installation, CI, contents, sdist, and publication contract.

## P2 local certification

| Gate | Status | Result |
| --- | --- | --- |
| Product identity/version and packaging policy | **CERTIFIED** | PASS — `datapack-engine` / `datapack` / `0.1.0`, maturin 1.14.1, PyO3 0.29.0, `abi3-py39` |
| Linux release wheel build | **CERTIFIED** | PASS — manylinux2014 / glibc 2.17 x86_64 |
| Same-environment Linux wheel rebuild | **OBSERVATIONAL** | PASS — byte-identical filename, size, and SHA-256 |
| ABI3 wheel reuse | **CERTIFIED** | PASS — the same wheel on CPython 3.9.25 through 3.14.7 |
| Installed-distribution tests | **CERTIFIED** | PASS — 18 matrix executions, zero failures |
| Repository/toolchain isolation | **CERTIFIED** | PASS — temporary cwd/venv, repository absent from `sys.path`, `pip --no-index --no-deps`, Rust/Cargo/maturin absent from `PATH` |
| Wheel metadata/content/license/path hygiene | **CERTIFIED** | PASS — exact eight-member allowlist, no unexpected artifact or machine-specific absolute path |
| Linux native dependency audit | **CERTIFIED** | PASS — glibc 2.17 and policy libraries only |
| Windows wheel build/install | **NOT RUN** | Local Windows Application Control blocked `rustc.exe` (OS error 4551); hosted matrix implemented |
| Hosted P2 wheel matrix | **NOT RUN** | No push authorized or performed |
| Source distribution | **DEFERRED** | Technically assembleable, but no supported source-build contract is justified in P2 |
| PyPI / TestPyPI / crates.io | **NOT RUN** | No account, namespace, credential, reservation, trusted publisher, or upload action |
| `cargo fmt --all -- --check` | **CERTIFIED** | PASS |
| `cargo check --locked` | **CERTIFIED** | PASS |
| `cargo test --locked` | **CERTIFIED** | PASS — 359 tests, zero failures |
| Strict locked Clippy | **CERTIFIED** | PASS |
| `cargo build --release --locked` | **CERTIFIED** | PASS |
| Binding format/check/test/Clippy/release build | **CERTIFIED** | PASS — binding crate has zero Rust unit tests |
| Python syntax, Ruff lint, and Ruff formatting | **CERTIFIED** | PASS under the Python 3.9 syntax floor |
| Existing installed SDK tests | **CERTIFIED** | PASS — five tests, zero failures |
| `cargo deny` core and binding | **CERTIFIED** | PASS with the existing non-failing unmatched-license and duplicate-`syn` warnings |
| `cargo audit` core and binding | **CERTIFIED** | PASS with only allowed `RUSTSEC-2025-0141` |
| `cargo package --locked --allow-dirty` | **CERTIFIED** | PASS before commit; clean package verification is repeated after the checkpoint commit |
| Protected fixture tests, sizes, and SHA-256 | **CERTIFIED** | PASS — four tests and all six immutable values unchanged |
| Workflow YAML and six-version matrix structure | **CERTIFIED** | PASS |
| `git diff --check` | **CERTIFIED** | PASS |

## P2 change scope

P2 changes only Python packaging metadata, a mirrored license file, installed
distribution tests, certification automation, hosted CI, and documentation:

- `.github/workflows/ci.yml`;
- `README.md`;
- `docs/productization/DATAPACK-PRODUCTIZATION-REPORT.md`;
- `docs/productization/PYTHON_DISTRIBUTION.md`;
- `docs/productization/RELEASE_ARTIFACTS.md`;
- `docs/reference/PYTHON_SDK_FOUNDATION.md`;
- `python/DATAPACK-LICENSE-MIT`;
- `python/README.md`;
- `python/pyproject.toml`;
- `python/tests/installed_distribution.py`;
- `scripts/certify_python_wheel.py`; and
- `scripts/check_version_consistency.py`.

No Rust source, Python API surface, codec, planner, archive, wire format,
resource limit, protected fixture, fixture byte, or fixture SHA-256 value is
changed. PyPI, TestPyPI, crates.io, GitHub Releases, and Git tags remain
unmodified.

## Preserved external technical-beta evidence

Beta Test 001 remains historical external technical-beta correctness evidence.
Its timings remain **OBSERVATIONAL** and uncontrolled.

| Fact | Evidence |
| --- | --- |
| Dataset | `yellow_tripdata_2016-01.csv` |
| Input bytes | 1,708,674,492 |
| Archive bytes | 360,142,313 |
| Ratio | 4.7444x |
| Reduction | 78.92% |
| Chunks | 26 |
| Validation | PASS |
| Against original | MATCHED |
| Original SHA-256 | `fb785a71d2bc82e6480dc7fd63bcfc4632e9b94af643baefdc736fbbf234aca7` |
| Restored SHA-256 | `fb785a71d2bc82e6480dc7fd63bcfc4632e9b94af643baefdc736fbbf234aca7` |
| Independent byte comparison | PASS |

The run used a dev-profile wheel, WSL, `/mnt/c`, NTFS, and a OneDrive input path.
Its v2 archive used chunked RawZstd. The timings are not official benchmark results,
and the 4.7444x ratio is not evidence of structured-column compression advantage.

## Current claims boundary

- DataPack is not claimed to be production-ready, enterprise-ready, a universal ZIP
  replacement, a database/dataframe engine, or universally superior to zstd.
- RawZstd fallback remains an intentional, factual product behavior.
- No external security audit has occurred.
- `RUSTSEC-2025-0141` remains the documented compatibility advisory for frozen
  bincode 1.3.3 usage.
- Productization has not changed `.dpack` v1/v2 bytes or implemented `.dpack` v3.
