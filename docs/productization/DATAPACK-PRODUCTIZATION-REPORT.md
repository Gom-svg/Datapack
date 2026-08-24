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
| Hosted P1 CI | **CERTIFIED** | run `32694107348` |
| Hosted P2 distribution CI | **CERTIFIED** | run `32698666658` at technical checkpoint `cc128bb687b1cfbe64c6b6c7b912f31862176588` |
| Hosted P2 final checkpoint CI | **CERTIFIED** | run `32699440785` at checkpoint `a433dbad5761a169681bdab64273938b86b291eb`; all 18 required jobs passed |
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
| P2 — Python Distribution Foundation | **CERTIFIED** | Wheel-first `datapack-engine` packaging, manylinux2014 and Windows MSVC ABI3 artifacts, exact inspection, isolated installed-SDK testing, and build-once/test-six execution on both platforms passed technical run `32698666658` and final checkpoint run `32699440785`. Failed run `32697219488` and its Linux build-path remediation remain recorded below. Publication remains unauthorized. |
| P3 — Progress API | **CERTIFIED** | One additive Rust progress contract now serves Application callers, CLI, and Python; terminal success, optional totals, deterministic cadence, ordered v2 chunk facts, callback behavior, and observational byte equivalence are locally certified. Hosted P3 evidence is **NOT RUN**. |
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
| Hosted P1 CI | **CERTIFIED** | PASS — run `32694107348` |

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
The hosted Windows artifact is 729,926 bytes with SHA-256
`311ec7a6fe4de51e4c1f73884d4aec25fd8f3024e7348ee726250435c4d5b946`.
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
Application Control rejects `rustc.exe` with OS error 4551. Hosted run
`32698666658` built, inspected, uploaded, installed, and exercised the Windows
artifact successfully across CPython 3.9 through 3.14. This is independent
Windows evidence; no Windows result is inferred from Linux.

The source-distribution audit is **DEFERRED**. Maturin could assemble a complete
sdist and an extracted copy could build without the original checkout, but the
result exposes a Rust/native-toolchain installation contract and normal PEP 517
construction produced a native `linux_x86_64` wheel rather than the certified
manylinux artifact. P2 therefore remains deliberately wheel-first.

See `docs/productization/PYTHON_DISTRIBUTION.md` for the exact platform,
installation, CI, contents, sdist, and publication contract.

## P2 hosted CI run 32697219488 and Linux path remediation

Run `32697219488` — **FAILED**.

- The Rust 1.85 Ubuntu and Windows jobs, Python SDK foundation job, and
  dependency-policy/package job passed.
- The Windows MSVC x86_64 ABI3 wheel built, passed its identity/tag/metadata/
  contents inspection, and uploaded as a CI artifact.
- The Linux GNU x86_64 ABI3 wheel built with the expected manylinux2014 tags.
  Inspection then failed because `datapack/_native.abi3.so` contained
  `/home/runner/`.
- The Linux artifact upload and downstream CPython 3.9–3.14 distribution matrix
  were skipped as a consequence. They are not separate root causes.

The failure was a Rust compiler path-remapping defect, not an engine, Python
API, PyO3, maturin metadata, build-script, linker, or DWARF defect. The action's
Docker command mounted the checkout at the unchanged host path
`/home/runner/work/Datapack/Datapack` and used that path as its working
directory. The existing Linux `RUSTFLAGS` remapped `/io` and `/root`, but the
project was never compiled from `/io` in this action configuration.

A faithful release-wheel reproduction found 18 occurrences across 16 project
source paths. Every occurrence was in the stripped ELF `.rodata` section as a
rustc panic/location file-name string; there were no DWARF/debug sections. The
remediation adds an environment-derived `${GITHUB_WORKSPACE}` prefix remap to
the Linux-only workflow step while retaining the `/io` mount and `/root` Cargo
home remaps. All replacement prefixes are relative and stable. The Windows
conditional and its already-passing remaps are unchanged.

The unchanged wheel certifier passes the remediated local manylinux2014 wheel.
Explicit binary and whole-wheel scans found no `/home/runner`, `/home/`,
`/root/`, Windows drive/user, runner, or workspace-specific path. Eleven literal
`/io/` substring matches were inspected individually; all were the `/src/io/`
portion of Rust's virtual `/rustc/.../library/...` standard-library paths, with
zero `/io` Docker mount paths. The exact eight-member allowlist and absolute-path
hygiene policy remain unchanged.

## P2 hosted certification closure

Run `32698666658` — result **SUCCESS**, evidence status **CERTIFIED**, at
technical checkpoint `cc128bb687b1cfbe64c6b6c7b912f31862176588`.

All required jobs passed:

- Rust 1.85 on Ubuntu and Windows;
- Python SDK foundation;
- dependency policy and package verification;
- Linux GNU x86_64 ABI3 wheel build, inspection, and CI artifact upload;
- Windows MSVC x86_64 ABI3 wheel build, inspection, and CI artifact upload;
- isolated execution of the one Linux platform wheel on CPython 3.9, 3.10,
  3.11, 3.12, 3.13, and 3.14; and
- isolated execution of the one Windows platform wheel on CPython 3.9, 3.10,
  3.11, 3.12, 3.13, and 3.14.

Each platform therefore has direct hosted build, artifact-hygiene, upload,
clean-install, import, identity, version, and representative native SDK
execution evidence. The same ABI3 wheel was reused across all six CPython
versions on its platform. Run `32697219488` remains the historical failed run;
it is not rewritten or counted as successful evidence.

The documentation-closure checkpoint `a433dbad5761a169681bdab64273938b86b291eb`
then passed final hosted run `32699440785`: all 18 required Rust, Python,
dependency/package, Linux/Windows wheel, inspection/artifact, and CPython
3.9–3.14 isolated-wheel jobs succeeded. This is the final P2 hosted baseline.

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
| Windows wheel build/inspection | **CERTIFIED** | PASS in hosted run `32698666658`; local Windows Application Control still blocks `rustc.exe` (OS error 4551) |
| Windows wheel installed-version matrix | **CERTIFIED** | PASS — one `cp39-abi3-win_amd64` wheel executed on CPython 3.9 through 3.14 in run `32698666658` |
| Remediated hosted Linux wheel inspection | **CERTIFIED** | PASS — manylinux2014 build, exact inspection, and CI artifact upload in run `32698666658` |
| Hosted P2 installed-version matrix | **CERTIFIED** | PASS — all 12 Linux/Windows CPython 3.9–3.14 jobs in run `32698666658` |
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

## P3 progress API outcome

P3 is **CERTIFIED** locally. It preserves the existing public
`OperationKind`, `ProgressPhase`, `ProgressState`, `ProgressEvent`,
`ProgressObserver`, and six silent/`*_with_progress` Application function pairs.
The audit found a sound shared Rust foundation: Application I/O supplied
rate-limited byte events, the v2 storage pipeline supplied caller-thread ordered
chunk facts, legacy Benchmark supplied typed stage events, and the CLI already
adapted Compress/Decompress events. Analyze, Validate, and Compare were coarse;
Python had no observer surface.

P3 evolves that contract additively:

- `Analyzing` distinguishes bounded analysis from planning, while its completed
  byte total is the sample scope rather than the complete source;
- `Finalizing/Completed` is the only terminal-success fact and follows a
  successful transactional commit where an output exists;
- stable snake-case identifiers, Serde output adaptation, and a Rust
  `percentage()` helper derive presentation from integer counters;
- unknown totals remain `None`, completed values never exceed known totals, and
  a zero-byte phase has no in-progress percentage;
- core streaming I/O cadence is deterministic at 8 MiB rather than wall-clock
  throttled;
- v2 reports chunks and original bytes only when the ordered writer commits the
  chunk to the transactional temporary stream, never merely when a worker
  finishes;
- Analyze, Validate, Compare, and in-memory v1 phases remain truthfully coarse
  rather than manufacturing percentages; and
- CLI human presentation is enabled only when stderr is a terminal, preserving
  redirected and machine-readable output.

The Python SDK now accepts optional keyword-only `progress` callbacks on
Analyze, Compress, Decompress, Validate, and Compare. Callbacks receive a
frozen `datapack.ProgressEvent` containing operation, stage, state, integer
byte/item counters, optional totals, the Rust-derived percentage, and terminal
success. Rust work detaches from Python; each callback reattaches safely on the
operation's calling thread. A Python callback exception is explicitly isolated:
it is sent to `sys.unraisablehook`, later callbacks for that operation are
disabled, and the Rust operation continues. It is not implicit cancellation.

Rust observers remain synchronous and infallible by type. A panic follows
normal Rust unwinding. A focused test proves an observer panic at a v2 advanced
event does not publish the transactional output; a panic at a post-commit
terminal event cannot undo an already successful commit. P4 cancellation
remains **DEFERRED** and no cancellation token, async runtime, or task runtime
was added.

The detailed contract, operation granularity, CLI/Python adapters, callback
semantics, future IPC compatibility, and P4 boundary are documented in
`docs/productization/PROGRESS_API.md`.

### P3 progress audit answers

| Audit question | Finding |
| --- | --- |
| Existing types | Public non-exhaustive `OperationKind`, `ProgressPhase`, `ProgressState`, `ProgressEvent`, and `ProgressObserver`; crate-private `ProgressEmitter`; crate-private v2 `ChunkedProgress`; legacy Benchmark typed events. |
| Public progress entry points | All six Rust Application operations already had silent and observer-aware pairs. |
| Existing emitters | Application wrappers and I/O, v2 ordered chunk storage callbacks, and legacy Benchmark phases. |
| Coarse operations | Analyze, Validate, Compare, v1 in-memory transform stages, and some Benchmark stages. |
| Truthful bytes/totals | File metadata, bounded analysis coverage, observed reads/writes, selected comparison scope, and v2 restored/original byte metadata. |
| Truthful chunks | V2 only; chunk totals come from its actual chunk table/plan. V1 receives no fabricated chunk unit. |
| Safe event boundaries | Bounded reads/writes, ordered v2 temporary-stream writes, coarse bulk-call boundaries, and post-commit result completion. |
| CLI relationship | Compress/Decompress already consumed Application events; P3 adds Analyze/Validate/Compare and terminal-only stderr presentation while preserving Benchmark's existing typed-event adapter. |
| Compatibility | The existing progress model was already public. P3 preserves its types/functions and adds enum variants, helpers, serialization, adapters, and events under their non-exhaustive contract. |

### P3 performance observation

One local uncontrolled debug-profile observation used a deterministic 4 MiB
input, 256 KiB v2 chunks, one worker, and max two in flight. Silent compression
took 214.150996 ms; compression with a lightweight collecting observer took
209.013616 ms and delivered exactly 20 events (16 ordered chunk advances, phase
start/completion, and terminal start/completion). Both archives were
byte-for-byte identical. This WSL/NTFS-path timing is **OBSERVATIONAL**, has no
CI threshold, and does not claim that callbacks improve performance.

### P3 local certification

| Gate | Status | Result |
| --- | --- | --- |
| `cargo fmt --all -- --check` | **CERTIFIED** | PASS |
| `cargo check --locked` | **CERTIFIED** | PASS |
| `cargo test --locked` | **CERTIFIED** | PASS — 363 tests, zero failures |
| Strict locked Clippy | **CERTIFIED** | PASS |
| `cargo build --release --locked` | **CERTIFIED** | PASS |
| Binding format/check/test/Clippy/release build | **CERTIFIED** | PASS — eight SDK tests, zero failures; binding crate has zero Rust unit tests |
| Python 3.9 syntax, Ruff lint, and Ruff formatting | **CERTIFIED** | PASS |
| Linux ABI3 wheel build and exact P2 certification | **CERTIFIED** | PASS — unchanged `cp39-abi3-manylinux_2_17_x86_64.manylinux2014_x86_64` policy and eight-member allowlist; 978,898 bytes; SHA-256 `51d414c5caea6340c7199a84507d3534dfc70df96dbc57b42009547835593ef0` |
| Isolated installed-wheel SDK | **CERTIFIED** | PASS — three tests, zero failures, including a structured native progress callback |
| `cargo deny` core and binding | **CERTIFIED** | PASS with the existing non-failing unmatched-license and duplicate-version warnings |
| `cargo audit` core and binding | **CERTIFIED** | PASS with only allowed `RUSTSEC-2025-0141` |
| `cargo package --locked --allow-dirty` | **CERTIFIED** | PASS |
| Protected fixtures, sizes, and SHA-256 | **CERTIFIED** | PASS — four compatibility tests and all six immutable values unchanged |
| Progress-disabled/enabled archive equality | **CERTIFIED** | PASS — byte-for-byte identical v2 artifacts |
| Windows and CPython 3.9–3.14 hosted P3 regression | **NOT RUN** | Requires the post-P3 hosted workflow; P2 final run `32699440785` remains the last hosted certification. |
| `git diff --check` | **CERTIFIED** | PASS |

No `.dpack` v1/v2 semantics, protected fixtures, planner decisions, codec
behavior, Application operation results, Python identity, ABI floor, platform
policy, dependency, CI workflow, or release/publication state changed in P3.

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
