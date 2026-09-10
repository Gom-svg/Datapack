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
| Hosted P3 progress CI | **CERTIFIED** | run `32701817032` at technical checkpoint `6c40e70dc155930ff2f42931e4c650b5d6dddf49`; all 18 required jobs passed |
| Hosted P3 documentation closure CI | **CERTIFIED** | run `32703591864` at checkpoint `138cdf1812191ad0e45d07f9feb009a1b29d6c6e`; all 18 required jobs passed |
| Hosted P4 cancellation CI | **CERTIFIED** | run `32709843703` at Windows strict-Clippy remediation checkpoint `585074907c98a6ff3fe52d50fa9688b43fe5aaf7`; all 18 required jobs passed |
| Certified pre-P5 checkpoint | **CERTIFIED** | `bc08ed570a21a32cd72a9c494e710dbe6ed09109` on `productization/foundation`, `origin/productization/foundation`, `main`, and `origin/main` |
| Hosted pre-P5 branch CI | **CERTIFIED** | `productization/foundation` run `32712910071` and `main` run `32713671052`; both SUCCESS |
| Hosted P5 API/error CI | **CERTIFIED** | run `32716713250` at technical checkpoint `5bed871036621911863d26b67e27a54019370104`; all 18 required jobs passed |
| P6 starting checkpoint | **CERTIFIED** | clean post-P5 checkpoint `ca1f0f7777a81481dfbecb3e1b8e3112bf6045c5` on `productization/foundation` |
| Hosted P6 Technical Beta II CI | **CERTIFIED** | run `34404009535` at technical checkpoint `621574a2607e3857fe932dbe9db8ec6eddc5a9fd`; SUCCESS, all 18 required jobs passed |
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
| P3 — Progress API | **CERTIFIED** | One additive Rust progress contract now serves Application callers, CLI, and Python; terminal success, optional totals, deterministic cadence, ordered v2 chunk facts, callback behavior, and observational byte equivalence are certified locally and in hosted CI run `32701817032`. |
| P4 — Cooperative Cancellation | **CERTIFIED** | Explicit `CancellationToken`/`OperationControl`, typed cancellation outcomes, transactional commit precedence, safe v1/v2/read/benchmark checkpoints, and Python cancellation are implemented and certified locally and in hosted CI run `32709843703`. Failed run `32708310316` and its Windows strict-Clippy remediation remain recorded below. |
| P5 — Product API & Error Experience Polish | **CERTIFIED — CLOSED** | Additive Rust error categories/codes, contextual CLI rendering/help, Python docstrings/typing/exception identity, focused compatibility tests, and public API/error documentation passed local certification and all 18 hosted jobs in run `32716713250`. |
| P6 — Technical Beta II | **CERTIFIED — CLOSED** | Local certification passed, including current installed-wheel CI/manual beta, release CLI, security/package, protected compatibility, and supplemental 16 GiB evidence review. Hosted certification passed in run `34404009535`: SUCCESS, all 18 required jobs passed. P6 is formally closed. See `TECHNICAL_BETA_II.md`. |
| P7 — Release Artifacts / Release Engineering | **IMPLEMENTED**, locally **CERTIFIED** | Internal CLI/wheel candidate builds, extracted CLI and isolated wheel certification, checksums, source-provenance manifests, and a manual two-platform/aggregation workflow. All applicable local gates passed; hosted certification **PENDING**. See `RELEASE_ARTIFACTS_AND_ENGINEERING.md`. |
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

P3 is **IMPLEMENTED** and **CERTIFIED** locally and in hosted CI. It preserves the existing public
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
terminal event cannot undo an already successful commit. At the P3 checkpoint,
P4 cancellation remained **DEFERRED** and no cancellation token, async runtime,
or task runtime had been added; the later P4 outcome is recorded below.

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
| Windows and CPython 3.9–3.14 hosted P3 regression | **CERTIFIED** | PASS — all 18 required jobs passed in run `32701817032`, including isolated Linux and Windows execution on CPython 3.9 through 3.14. |
| `git diff --check` | **CERTIFIED** | PASS |

### P3 hosted certification closure

Run `32701817032` — result **SUCCESS**, evidence status **CERTIFIED**, at P3
technical checkpoint `6c40e70dc155930ff2f42931e4c650b5d6dddf49`.
All 18 required hosted jobs passed:

- Rust 1.85 on Ubuntu and Windows;
- the Python SDK foundation and dependency/package policy;
- Linux GNU x86_64 and Windows MSVC x86_64 ABI3 wheel builds, with identity,
  tag, metadata, content inspection, and CI artifact upload; and
- isolated execution of each platform's wheel on CPython 3.9, 3.10, 3.11,
  3.12, 3.13, and 3.14.

This hosted evidence confirms that the Product-Grade Progress API preserves
Linux and Windows Rust execution, the Python SDK, the ABI3 packaging contract,
both platform wheel executions, the declared CPython range, and the P2
distribution guarantees. Progress remains observational and distinct from
cancellation. At this P3 closure checkpoint, P4 cooperative cancellation
remained **DEFERRED**.

No `.dpack` v1/v2 semantics, protected fixtures, planner decisions, codec
behavior, Application operation results, Python identity, ABI floor, platform
policy, dependency, CI workflow, or release/publication state changed in P3.

## P4 cooperative cancellation outcome

P4 is **IMPLEMENTED** and **CERTIFIED** locally and in hosted CI. The technical
checkpoint is `c1d542c8cd2a472244868795786b47c6563aa25d`; its Windows strict-Clippy
remediation is `585074907c98a6ff3fe52d50fa9688b43fe5aaf7`. Successful hosted run
`32709843703` passed all 18 required jobs. The starting checkpoint was
`138cdf1812191ad0e45d07f9feb009a1b29d6c6e`, whose P3 documentation closure
passed all 18 hosted jobs in run `32703591864`.

The initial read-only audit found no public cancellation primitive. The v2
compression pipeline had a private `Arc<AtomicBool>` used only to stop sibling
reader/worker work after an internal pipeline failure; callers could neither
request nor observe it. All six Application operations already had simple and
`*_with_progress` functions. Compress and Decompress used sibling
transactional `TempOutput` files, Drop cleanup, a final flush/commit boundary,
backup-and-restore overwrite behavior, and an existing `keep_partial` policy.
Analyze and Validate create no output; Compare owns a bounded temporary
workspace; Benchmark owns its existing temporary artifacts. Python detached
long Rust work and reattached only for progress callbacks, but exposed no
cancellation type. Python `KeyboardInterrupt` behavior was not converted into
operation control, and CLI Ctrl+C depended on process termination.

The public `DatapackError` enum is exhaustively matchable, so adding a
`Cancelled` variant would be a breaking source change. P4 instead evolves the
Application API additively:

- `CancellationToken` is a cloneable `Arc<AtomicBool>` handle with monotonic,
  idempotent `cancel()` and `is_cancelled()` operations using Release/Acquire
  ordering;
- `OperationControl` independently carries an optional `ProgressObserver` and
  optional cloned cancellation token;
- each of Analyze, Compress, Decompress, Validate, Compare, and Benchmark adds
  one `*_with_control` entry point while all simple and P3 progress-aware calls
  remain unchanged; and
- non-exhaustive `OperationError::{Cancelled, Failed(DatapackError)}` supplies a
  typed cancellation outcome without modifying `DatapackError`.

Cancellation is checked before meaningful work, around deterministic bounded
I/O and progress delivery, between major in-memory stages, at v2 ordered chunk
boundaries, within validation/hash loops, between comparison and Benchmark
suboperations, and immediately before transactional commit. Ordinary errors
that occur before cancellation is observed remain ordinary failures. A
cancellation observed at a checkpoint wins at that checkpoint.

For Compress and Decompress, cancellation before the final commit leaves no
final output and preserves an existing destination. Default Drop cleanup
removes the sibling partial. Existing `keep_partial=true` continues to retain
only the explicitly named sibling `.partial` artifact; it is never promoted or
reported as a complete output. The final pre-commit check defines the race:
observed cancellation prevents commit, while a successful commit wins over a
later request and cannot be retroactively deleted or reported as cancelled.
Only the latter path emits terminal `Finalizing/Completed` progress.

V2 compression preserves its reader/bounded-worker/ordered-writer topology,
bounded channels, ordered archive bytes, and scoped thread joins. A request may
allow already-running bounded work to finish during teardown, but queued or
in-flight results are not committed after cancellation wins. V2 decompression
and validation check every chunk. V1 raw zstd checks its natural 64 KiB stream
loop; structured v1 and several legacy Benchmark transforms use truthful coarse
stage boundaries. Analyze retains bounded sampling. Compare checks its 256 KiB
snapshot reads, 64 KiB hashes, run boundaries, and cleanup. Benchmark stops the
active controlled suboperation and does not start later planned measurements
after cancellation is observed.

Python now exports frozen `datapack.CancellationToken`, with `cancel()` and the
read-only `is_cancelled` property, plus typed `datapack.CancelledError` under
`DataPackError`. Analyze, Compress, Decompress, Validate, and Compare accept an
optional keyword-only `cancellation=` argument. They still detach during Rust
work; a second Python thread can invoke the shared native token without a
Python polling worker. A progress callback can explicitly call `token.cancel()`.
P3 callback exceptions remain reported through `sys.unraisablehook`, disable
later callbacks, and allow the Rust operation to continue; callback return
values and exceptions never become cancellation.

Python `KeyboardInterrupt` conversion and CLI signal-to-token integration are
**DEFERRED**. P4 adds no signal dependency, unsafe handler, Tokio/async runtime,
task scheduler, or background service. The shared token is suitable for a
future Desktop operation handle, but Desktop/Tauri remains **DEFERRED**.

The exact contract, checkpoint granularity, transaction precedence, Python
threading, frontend limitations, and future adapter boundary are documented in
`docs/productization/CANCELLATION_API.md`.

### P4 cancellation audit answers

| Audit question | Finding |
| --- | --- |
| Existing primitive | No caller-facing primitive. V2's private atomic flag handled only internal pipeline failure teardown. |
| Suitable operations | All six Application operations have useful cooperative boundaries; some v1/legacy Benchmark work remains coarse. |
| Safe checkpoints | Before work; bounded reads/hashes; v2 ordered chunks; major v1 stages; validation/comparison/Benchmark boundaries; progress delivery; and immediately pre-commit. |
| Commit points | `TempOutput::commit_with_cleanup_warning` publishes Compress/Decompress output; report return is the logical point for output-free operations. |
| Temporary output | Sibling transactional `.partial` files for Compress/Decompress, comparison workspace files, and existing Benchmark temporary artifacts. |
| Ordinary cleanup | `TempOutput` Drop removes owned partials unless `keep_partial`; comparison/Benchmark guards remove their owned paths. |
| `keep_partial` | Already existed. P4 preserves it and never promotes retained cancellation output to the final path. |
| Typed error compatibility | Modifying exhaustive `DatapackError` would be breaking; additive non-exhaustive `OperationError` avoids that break. |
| Python interrupt state | Long work detached from Python; no prior token or proven portable `KeyboardInterrupt` conversion. |
| CLI interrupt state | Ctrl+C was process termination only; no typed cooperative control or portable signal adapter. |
| Approval/dependency decision | No stop condition arose. A standard-library atomic implementation was sufficient and no dependency was added. |

### P4 performance observation

One local uncontrolled debug-profile observation used the same deterministic
4 MiB input, 256 KiB v2 chunks, one worker, and at most two chunks in flight.
Compression without operation control took 208.897471 ms; compression with an
installed but never-cancelled token took 209.407553 ms across 16 chunks. The
archives were byte-for-byte identical. This WSL/NTFS-path measurement is
**OBSERVATIONAL**, has no wall-clock CI threshold, and makes no universal
latency or overhead claim.

### P4 local certification

| Gate | Status | Result |
| --- | --- | --- |
| `cargo fmt --all -- --check` | **CERTIFIED** | PASS |
| `cargo check --locked` | **CERTIFIED** | PASS |
| `cargo test --locked` | **CERTIFIED** | PASS — 375 tests, zero failures, including a deterministic mid-read validation-hash checkpoint |
| Strict locked Clippy | **CERTIFIED** | PASS |
| `cargo build --release --locked` | **CERTIFIED** | PASS |
| Binding format/check/test/Clippy/release build | **CERTIFIED** | PASS — binding crate has zero Rust unit tests |
| Python 3.9-target syntax, Ruff lint, and Ruff formatting | **CERTIFIED** | PASS |
| Source SDK tests | **CERTIFIED** | PASS — 12 tests, zero failures, including deterministic callback and cross-thread cancellation |
| Linux ABI3 wheel build and exact P2 certification | **CERTIFIED** | PASS — `cp39-abi3-manylinux_2_17_x86_64.manylinux2014_x86_64`, exact eight-member allowlist, 988,668 bytes, SHA-256 `d308c40d9cd947f9227ed3c4fe73eaa13a8c948307fe1bac81d3c2f68658349e` |
| Isolated installed-wheel SDK | **CERTIFIED** | PASS — four tests, zero failures, including native typed transactional cancellation; Python 3.14.4; repository and build tools absent |
| P1 identity/version consistency | **CERTIFIED** | PASS — `0.1.0` |
| `cargo deny` core and binding | **CERTIFIED** | PASS with the existing non-failing unmatched-license and duplicate-`syn` warnings |
| `cargo audit` 0.22.1 core and binding | **CERTIFIED** | PASS with only allowed `RUSTSEC-2025-0141` |
| `cargo package --locked --allow-dirty` | **CERTIFIED** | PASS — 163 files, 1.7 MiB, 422.7 KiB compressed, verification build passed |
| Protected fixtures, sizes, and SHA-256 | **CERTIFIED** | PASS — four compatibility tests and all six immutable values unchanged |
| Progress and cancellation regression | **CERTIFIED** | PASS — no false terminal success on cancellation; never-cancelled controlled output remained byte-identical |
| Windows and CPython 3.9–3.14 hosted P4 regression | **CERTIFIED** | PASS — all 18 required jobs passed in run `32709843703`, including Windows strict Clippy and isolated Linux and Windows wheel execution on CPython 3.9 through 3.14. |
| `git diff --check` | **CERTIFIED** | PASS |

### P4 hosted Windows strict-Clippy remediation

Hosted run `32708310316` — result **FAILED**. Linux Rust, Windows formatting,
check, and tests, the Python SDK, dependency/package policy, Linux and Windows
ABI3 wheel build/inspection/artifact upload, and isolated wheel execution on
both platforms across CPython 3.9 through 3.14 all passed. The only failed gate
was Windows Rust 1.85 strict Clippy:

`src/storage/chunked.rs:1183` returned
`Result<(), ControlledV2ValidationError>`, whose
`ControlledV2ValidationError::Failed(V2ValidationError)` variant contains at
least 128 bytes on Windows. With `-D warnings`,
`clippy::result_large_err` correctly failed the job. Windows Rust tests had
already passed, so this is a platform-specific lint portability issue rather
than a functional cancellation or validation failure.

The P4 wrapper is crate-private, non-serialized, and carries the existing
crate-private `V2ValidationError` by value. Local Linux debug-layout evidence
measures both errors at 112 bytes. The previously certified Windows validator
policy records `V2ValidationError` as exactly 128 bytes because of the Windows
`PathBuf`-bearing `DatapackError` layout. The P4 implementation retained that
Windows lint attribute on the new wrapper type instead of moving the
function-scoped policy to the replacement result-returning function, so
Clippy did not apply it at the reported boundary.

The remediation moves the existing
`#[cfg_attr(windows, allow(clippy::result_large_err))]` to only
`validate_raw_zstd_chunked_payload_with_control`. This follows the certified
pre-P4 precedent, avoids an otherwise unnecessary error-path heap allocation,
and does not change the error representation, cancellation/validation
semantics, public API, serialization, archive behavior, or global strict
Clippy policy. At the remediation checkpoint, a native Windows strict-Clippy
rerun was **NOT RUN** locally; subsequent hosted evidence is recorded below.

Local remediation recertification is **CERTIFIED**: all 375 Rust tests, strict
Linux Clippy, the binding gates, 12 source-SDK tests, the unchanged P2 wheel
certifier, four isolated installed-wheel tests, both dependency-policy checks,
both audit checks, crate packaging, and all four protected compatibility tests
passed. The rebuilt Linux artifact remains
`cp39-abi3-manylinux_2_17_x86_64.manylinux2014_x86_64`, passed the exact
eight-member allowlist, and is 988,667 bytes with SHA-256
`d60b3636423c9680a19c24f63ab333716416bb926086e0deb51022961752365a`.
All six protected fixture sizes and SHA-256 values are unchanged. Native
Windows strict Clippy is **NOT RUN** locally because this Linux/WSL environment
has no Windows Rust target or native Windows toolchain installed.

### P4 hosted certification closure

Run `32709843703` — result **SUCCESS**, evidence status **CERTIFIED**, at
Windows strict-Clippy remediation checkpoint
`585074907c98a6ff3fe52d50fa9688b43fe5aaf7`. All 18 required hosted jobs
passed:

- Rust 1.85 on Ubuntu and Windows, including Windows strict Clippy;
- the Python SDK foundation and dependency/package policy;
- Linux GNU x86_64 and Windows MSVC x86_64 ABI3 wheel build, identity/tag/
  metadata/content inspection, and CI artifact upload; and
- isolated execution of each platform wheel on CPython 3.9, 3.10, 3.11, 3.12,
  3.13, and 3.14.

This hosted evidence certifies P4 without changing its contract:
`CancellationToken` remains explicit typed operation control, cancellation
remains separate from P3 progress, and `OperationControl` composes the two.
Pre-commit cancellation prevents transactional publication; successful commit
wins over a later cancellation request; and cancellation never emits false
whole-operation terminal completion. Python retains typed
`CancellationToken`/`CancelledError`, tested cross-thread and explicit
progress-callback-driven cancellation, while P3 callback-exception isolation
remains unchanged.

Python `KeyboardInterrupt` conversion and CLI Ctrl+C signal-to-token
integration remain **DEFERRED**. Desktop/Tauri remained **DEFERRED** and P5
had not started at this P4 closure point. Failed run `32708310316` remains the
historical record of the Windows-only `result_large_err` strict-Clippy failure
and remediation above.

## Productization Beta Test 002

DataPack Productization Beta Test 002 — Progress + Cancellation +
Transactional Safety — result **PASS**. This manual external productization
validation used the Linux ABI3 wheel from hosted CI run `32710691878`, installed
into an isolated virtual environment outside the repository.

| Fact | Evidence |
| --- | --- |
| DataPack / distribution | `datapack` 0.1.0 / `datapack-engine` 0.1.0 |
| Environment | Python 3.14.4; WSL2 Linux x86_64 |
| Dataset | Deterministically generated external CSV; 134,219,306 bytes; 2,049,597 rows |
| Original SHA-256 | `1c1e6ae1be35f8c6ae2f8548f0d1648715964fb5cf4cacd185edb4a25efe383c` |
| Analyze | PASS — structured progress observed |
| Cancelled compression | PASS — cancellation followed real progress; typed `CancelledError`; no final archive, `.partial`, or false terminal success |
| Completed compression | PASS — `raw_zstd`; 55,615,065-byte archive; 2.4134x ratio; 58.56% reduction |
| Archive validation | PASS — `valid=true`; source comparison `matched` |
| Completed decompression | PASS — restored SHA-256 matched the original and independent byte comparison passed |
| Cross-thread decompression cancellation | PASS — existing destination preserved; no `.partial` or false terminal success |

The observed full-compression time of approximately 1.041 seconds is
**OBSERVATIONAL** evidence from this specific environment and workload. It is
not a controlled compression benchmark, performance threshold, or universal
throughput claim. The external dataset is not a protected repository fixture,
and this successful beta does not establish production readiness.

The complete methodology, limitations, and result record are in
`docs/productization/BETA-TEST-002.md`. P5 had not started when this external
test ran; its subsequent outcome is recorded below.

## P5 product API and error experience outcome

P5 is **IMPLEMENTED**, locally **CERTIFIED**, hosted-CI **CERTIFIED**, and
**CLOSED**. The technical checkpoint is
`5bed871036621911863d26b67e27a54019370104`; hosted run `32716713250`
completed with result **SUCCESS**. P5 started from certified checkpoint
`bc08ed570a21a32cd72a9c494e710dbe6ed09109`; the branch, upstream, worktree,
index, and shared main recovery boundary were verified before implementation.

The read-only public-surface audit found that DataPack already had a coherent
Application execution model: six typed Rust operations, versioned result
reports, additive progress/control entry points, transactional output, a thin
five-operation Python adapter, structured validation/comparison results, and
clean CLI report/progress stream separation. P5 therefore preserves:

- all simple, `*_with_progress`, and `*_with_control` Rust signatures;
- every request and result shape;
- all Python operation signatures and dictionary returns;
- the existing Python exception hierarchy;
- CLI numeric exit statuses and stdout/stderr roles;
- validation invalidity and against mismatch as structured report outcomes;
- comparison winners/differences as successful report facts; and
- all P2 distribution, P3 progress, P4 cancellation, safe-default, resource,
  transaction, and archive contracts.

The audit also found the product-polish gaps addressed by P5:

- `DatapackError` had typed variants but no uniform public category/code
  helper, while adding a variant would break exhaustive downstream matching;
- `OperationError` separated cancellation correctly but did not expose the
  same stable helper surface;
- Python structurally mapped Rust variants to useful subclasses, but those
  classes lacked explicit machine-readable identity, operation docstrings
  were absent, and the stub returned undifferentiated dictionaries;
- CLI runtime errors had contextual prose and established exit values but no
  stable displayed identifier; several primary command/positional help texts
  required source-code knowledge; and
- Python/reference documentation contained stale pre-certification Windows
  wording and did not give one consolidated result/error/exit policy.

P5 adds the non-exhaustive, Serde-adaptable `ErrorCategory` with stable
snake-case values. Existing exhaustive `DatapackError` is unchanged and gains
constant-time `category()` and `code()` helpers. `OperationError` gains the same
helpers, preserving `Cancelled` versus `Failed(DatapackError)` and delegating
ordinary identity to the wrapped error. Display messages and source chains are
unchanged. The exact categories, codes, and mappings are documented in
`docs/productization/PUBLIC_API_AND_ERRORS.md`.

CLI runtime errors now render `error[stable_code]: contextual message` on
stderr. Existing statuses remain unchanged: zero for success, one for general
command/operation failure including a completed invalid validation, two for
Clap usage and established invalid-input/output cases, and three for the
established row-range failure. Structured report/data output remains on
stdout. A completed invalid `validate --json` still emits its structured
`valid=false` report and a nonzero status; failure before a report leaves
stdout empty. Compare winners remain successful facts. Help now states path
roles, scope, verification/overwrite defaults, limits, partial retention, and
JSON behavior for the primary commands.

Python retains all exception types and signatures. Exception classes now
expose stable broad `category` and `code` attributes, and every subclass,
including `CancelledError`, remains catchable as `DataPackError`. Rust variants
continue mapping structurally rather than through message parsing. Runtime
docstrings cover Analyze, Compress, Decompress, Validate, Compare,
`ProgressEvent`, `CancellationToken`, option types, and exceptions. The Python
3.9-compatible stub adds path/literal/callback types and report-specific
`TypedDict` return descriptions without replacing the runtime dictionaries.

Path and destination messages already carried role, path, OS source context,
the explicit `--force` remedy, and transactional output status; focused P5
tests now freeze those invariants without asserting platform-specific OS prose.
Existing validation diagnostic codes keep resource-limit failures distinct
from corruption and mismatch. No limit is weakened and no automatic retry is
introduced.

P5 adds no dependency, allocation or branch to successful codec hot paths,
unsafe code, network action, telemetry, async runtime, format, planner rule, or
execution engine. Error helpers are constant matches used only when callers or
the CLI request identity. Future Desktop/IPC remains **DEFERRED**, while typed
categories, stable identifiers, and versioned reports are adapter-friendly.
P6 is separate additive certification work and does not alter the P5 contracts
described above.

### P5 local certification

| Gate | Status | Result |
| --- | --- | --- |
| Rust/Cargo toolchain | **CERTIFIED** | PASS — Rust/Cargo 1.85.0 |
| `cargo fmt --all -- --check` | **CERTIFIED** | PASS |
| `cargo check --locked` | **CERTIFIED** | PASS |
| `cargo test --locked` | **CERTIFIED** | PASS — 382 tests, zero failures |
| Strict locked Clippy | **CERTIFIED** | PASS — global `-D warnings` unchanged |
| `cargo build --release --locked` | **CERTIFIED** | PASS |
| Binding format/check/test/Clippy/release build | **CERTIFIED** | PASS — binding crate has zero Rust unit tests |
| Product identity/version consistency | **CERTIFIED** | PASS — `datapack-engine` / `datapack` / `0.1.0` |
| Python 3.9-target syntax, Ruff lint, and Ruff formatting | **CERTIFIED** | PASS |
| Source SDK tests | **CERTIFIED** | PASS — 15 tests, zero failures |
| CLI/API/error experience | **CERTIFIED** | PASS — focused help, error identity, stream, path, overwrite, validation, and exit-policy coverage plus the complete Rust suite |
| Linux ABI3 wheel build and exact P2 certification | **CERTIFIED** | PASS — `cp39-abi3-manylinux_2_17_x86_64.manylinux2014_x86_64`, exact eight-member allowlist, 991,852 bytes, SHA-256 `d8bfd09b740b2e1fd3dbc8571b71dab143f181fc18f22783a2b4775c649eb146` |
| Isolated installed-wheel SDK | **CERTIFIED** | PASS — five tests, zero failures; Python 3.14.4; repository and Rust/Cargo/maturin absent |
| P3 progress / P4 cancellation regression | **CERTIFIED** | PASS — typed events, callback isolation, explicit/cross-thread cancellation, no false terminal success, and transactional cleanup remain tested |
| `cargo deny` core and binding | **CERTIFIED** | PASS with existing non-failing unmatched-license and duplicate-`syn` warnings |
| `cargo audit` 0.22.1 core and binding | **CERTIFIED** | PASS with only allowed `RUSTSEC-2025-0141` |
| `cargo package --locked --allow-dirty` | **CERTIFIED** | PASS — 166 files, 1.8 MiB, 436.4 KiB compressed, verification build passed |
| Protected fixtures, sizes, and SHA-256 | **CERTIFIED** | PASS — four compatibility tests and all six immutable values unchanged |
| Beta Test 002 evidence | **CERTIFIED** | Preserved as historical PASS; its approximately 1.041-second timing remains **OBSERVATIONAL** |
| Linux/Windows and CPython 3.9–3.14 hosted P5 regression | **CERTIFIED** | PASS — all 18 required jobs passed in run `32716713250`, including native Linux and Windows Rust, strict Clippy, Python SDK, dependency/package policy, both ABI3 wheel builds/inspection, and isolated execution on CPython 3.9 through 3.14 on both platforms |
| `git diff --check` | **CERTIFIED** | PASS |

### P5 hosted certification closure

Run `32716713250` — result **SUCCESS**, evidence status **CERTIFIED**, at P5
technical checkpoint `5bed871036621911863d26b67e27a54019370104`. All 18
required hosted jobs passed:

- Rust 1.85 on native Ubuntu and Windows, including strict Clippy;
- the Python SDK and dependency/package policy, including cargo audit and
  package verification;
- Linux GNU x86_64 and Windows MSVC x86_64 ABI3 wheel build, identity/tag/
  metadata/content inspection, and CI artifact handling; and
- isolated execution of each platform wheel on CPython 3.9, 3.10, 3.11, 3.12,
  3.13, and 3.14.

This hosted evidence confirms that P5 preserves the certified Rust, Python,
progress, cancellation, dependency, packaging, wheel-hygiene, and cross-
platform distribution contracts. It does not establish production readiness
or authorize publication. P6 subsequently adds operational certification
coverage without changing the P5 contract.

P5 PyPI, TestPyPI, crates.io, GitHub Release, tag, merge, registry-credential,
and public-release actions remain **NOT RUN**. DataPack is not claimed
production-ready and has no external security audit.

## P6 Technical Beta II outcome

P6 is **CERTIFIED — CLOSED** as additive certification infrastructure. It starts from
clean post-P5 checkpoint
`ca1f0f7777a81481dfbecb3e1b8e3112bf6045c5` and does not modify engine, codec,
planner, archive, Rust API, Python ABI, dependency, or workflow-topology code.

`scripts/technical_beta_ii.py` runs through an installed `datapack-engine`
wheel. A specified SplitMix64 generator creates five deterministic workload
families with frozen CI row counts, seeds, byte sizes, SHA-256 values, and
expected planner modes. It then exercises:

- analyze, v1 planner-selected compression, forced v2 chunked RawZstd,
  validation against source, physical decompression, SHA-256, and independent
  byte equality;
- CSV, TSV, PSV, semicolon-delimited data, LF/CRLF, final-newline presence,
  quoting/escaping, Unicode, whitespace, numeric-text fidelity, empty/trailing
  fields, and long text;
- expected structured-analysis rejection followed by safe raw preservation for
  unsupported or malformed cases;
- early/mid/late compression cancellation, compression and decompression
  `keep_partial`, retry, false-terminal-event prevention, destination sentinel
  preservation, and overwrite behavior;
- compression admission, validation/decompression limits, source mismatch,
  corruption, and missing-archive behavior;
- Compare Quick/Full correctness, cancellation, and operational failure; and
- optional release-CLI identity/help, JSON stream cleanliness, stable error
  rendering, exit behavior, archive lifecycle, and exact restoration.

`scripts/certify_python_wheel.py` copies that harness into the existing
isolated wheel workspace and runs its CI profile after the installed public-SDK
tests. It remains outside the wheel, so the exact eight-member runtime allowlist
is unchanged. The existing Linux/Windows × CPython 3.9-3.14 wheel matrix
executed P6 in all 12 isolated jobs without adding a workflow or publication
path.

### P6 final local evidence

P6 local implementation and certification are complete. Hosted certification
also passed; its closure evidence is recorded below.
The final executable tree passed certification in WSL on 2026-09-08. The
2026-09-09 continuation recovered the retained logs and confirmed that
`cargo package --locked --allow-dirty` had completed with exit status 0 before
the interruption. Completed gates were reused; the remaining changes finalize
documentation.

The recovered 1,358-line harness was formatted with the prior Ruff 0.12.12
baseline to 1,604 lines and verified AST-identical. Both CI and the full manual
beta ran after that final edit. The manual run included the 4,097-column and
8 MiB-plus-one-byte record probes, 249 forced-v2 ETL chunks, and a retained
65,536-byte decompression partial. No product behavior changed.

| Gate | Status | Result |
| --- | --- | --- |
| Core Rust formatting/check/test/strict Clippy/release | **CERTIFIED** | PASS — Rust/Cargo 1.85.0; 382 tests, zero failures; locked commands and global `-D warnings` unchanged |
| Binding format/check/test-build/strict Clippy/release | **CERTIFIED** | PASS — zero binding Rust unit tests; release compilation included in current wheel build |
| Python source/version/grammar/lint/format/typing | **CERTIFIED** | PASS — `0.1.0`, `datapack-engine`, `datapack`; Python 3.9 grammar; Ruff 0.12.12; installed stub/runtime signatures and PEP 561 marker |
| SDK suite | **CERTIFIED** | PASS — 15 tests against the current installed wheel |
| Current Linux ABI3 wheel and isolated install | **CERTIFIED** | PASS — maturin 1.14.1, manylinux2014 auditwheel check, exact eight-member allowlist; five isolated tests on CPython 3.14.4 with source and Rust/Cargo/maturin absent at runtime |
| P6 CI profile from installed current ABI3 wheel | **CERTIFIED** | PASS — all frozen sizes, SHA-256 values, planner expectations, workload/format/edge/operational/resource/Compare cases |
| P6 full manual-beta profile | **CERTIFIED** | PASS — final harness revision, including beta-only hard-limit probes |
| Release CLI sequence | **CERTIFIED** | PASS — shared WSL release cache; exact roundtrip, JSON/stdout/stderr, stable errors and established exit behavior |
| `cargo deny` core/binding | **CERTIFIED** | PASS — existing unmatched-license and duplicate-`syn` warnings only |
| `cargo audit` 0.22.1 core/binding | **CERTIFIED** | PASS — only accepted `RUSTSEC-2025-0141` |
| `cargo package --locked --allow-dirty` | **CERTIFIED** | PASS — 168 files; verification build completed, exit 0 |
| Protected V1/V2 compatibility | **CERTIFIED** | PASS — four tests and all six frozen sizes/SHA-256 values unchanged |
| Supplemental 16 GiB evidence review | **CERTIFIED** | PASS — existing reports, process logs, saved hashes and artifact sizes inspected; no dataset rerun |
| Hosted Linux/Windows and CPython 3.9-3.14 P6 matrix | **CERTIFIED** | PASS — run `34404009535`, SUCCESS, all 18 required hosted jobs passed at technical commit `621574a2607e3857fe932dbe9db8ec6eddc5a9fd` |

The fresh 991,852-byte Linux wheel has SHA-256
`d8bfd09b740b2e1fd3dbc8571b71dab143f181fc18f22783a2b4775c649eb146`.
It matches P5's artifact because P6 adds no wheel content; the current build
log and installed runs establish P6 provenance. The final harness hash is
`5c1bf0337d2d493217932095fe1a53548e3f1992f7a8e4203d560d06a8888973`.
Retained logs and manual-beta JSON are outside Git; their inventory and exact
input/artifact hashes are recorded in `TECHNICAL_BETA_II.md` section 5.

Earlier Windows Application Control, WSL startup, and read-only cache failures
were environment problems, resolved for the final WSL certification through
approved access. They were not product defects. No engine, wire, planner, or
public-API defect was demonstrated, and no required local gate remains blocked.

### P6 hosted certification closure

Hosted run
[`34404009535`](https://github.com/Gom-svg/Datapack/actions/runs/34404009535)
completed with result **SUCCESS** at technical checkpoint
`621574a2607e3857fe932dbe9db8ec6eddc5a9fd` on `productization/foundation`.
All 18 required hosted jobs passed: Rust 1.85 on Linux and Windows, Python SDK
foundation, both platform ABI3 wheel builds, dependency policy/audit/package,
and all 12 isolated Linux/Windows wheel jobs across CPython 3.9-3.14, including
the P6 CI profile.

Local certification passed and hosted certification passed. P6 Technical Beta
II is formally closed with status **CERTIFIED — CLOSED**. P7 — Release
Artifacts / Release Engineering follows this closure; its current outcome is
recorded below. The subsequent phases remain
P8 — Desktop Foundation, then the Final Productization Acceptance Test.
Adaptive Compute and V3 remain separate future programs.

This documentation-only closure does not change code, workflows, dependencies,
formats, or public API, and does not establish production readiness or
authorize publication. The manual and supplemental evidence caveats remain
unchanged.

### P6 supplemental real-world evidence

An operator-performed external run used `HI-Large_Trans.csv`, exactly
17,052,760,651 bytes (approximately 15.88 GiB), with source SHA-256
`d13635e297c64673826217631fb88d635c4f506052e1bde833895eda2a65c3f2`.
Analysis was honestly sampled/partial: 10,000 records and 979,049 bytes. The
planner selected `csv_columnar_dictionary` for high repetition in 9 of 11
columns and estimated 13.583547% savings plus 3,420.8477 MiB dictionary memory.
The full structured candidate was deliberately **NOT RUN**; it did not fail.

The operator selected v2 chunked RawZstd with 64 MiB chunks, four threads, four
in-flight chunks, a 256 MiB admission bound, and adaptive level disabled. The
17,052,760,651-byte input produced a 3,541,994,339-byte archive: 4.8145x,
79.23% reduction, 255 chunks, and archive SHA-256
`f6d722ad2cd11e4ff5509169d8a1821090d8a0e57d2295e1a9577c1b2b595ad9`.

Validation against the source reported `valid=true`, `matched`, no diagnostics,
and passed v2 header/metadata/payload/decompression/length/chunk-table/per-chunk
SHA/global-SHA/trailing-data checks. Physical decompression restored exactly
17,052,760,651 bytes with the original SHA-256, and independent OS-level
`cmp --silent` passed.

Observed compression, validation, decompression, throughput, and RSS values are
**OBSERVATIONAL** because the source was accessed through WSL from Windows NTFS
and cache conditions were not controlled. The planner's 3.34 GiB structured
estimate and approximately 209 MiB observed v2 process RSS describe different,
non-comparable paths. They do not prove a multiplicative memory improvement.
The valid future-design observation is only that resource cost should inform
planning; bounded structured dictionaries and adaptive/global resource
management remain deferred v3/adaptive-compute work.

The full methodology, workload matrix, evidence provenance, measurements,
limitations, and claims boundary are in
`docs/productization/TECHNICAL_BETA_II.md`.

P6 PyPI, TestPyPI, crates.io, GitHub Release, tag, merge, registry-credential,
and public-release actions remain **NOT RUN**. P6 does not establish production
readiness or an external security certification.

## P7 Release Artifacts / Release Engineering outcome

P7 is **IMPLEMENTED** and locally **CERTIFIED** as internal candidate-production
tooling, with hosted certification **PENDING**. It starts from clean P6 closure checkpoint
`365b1ca3a2b8ca55bc5e20ae8ff40569184c0bad`. It reuses P1's canonical archive,
notes, manifest, checksum, and five-member CLI package contract, plus P2/P6's
pinned wheel builds, exact inspection, isolated SDK tests, and CI profile.

`scripts/release_candidate.py` uses Python 3.9-compatible standard-library code
to build native release CLIs, create normalized archives, inspect/extract and
smoke-test packaged binaries, build/certify wheels, record per-platform evidence,
and assemble/verify a complete seven-file candidate. The final bundle contains
two CLI packages, two standard ABI3 wheels, notes, a canonical JSON manifest,
and conventional SHA-256 checksums. Exact source SHA, lockfile hashes, toolchain,
workflow identity, artifact bytes/hashes, and smoke results are recorded.
Dirty local rehearsals cannot be assembled into clean-source candidates.

The dedicated workflow has two native jobs (Linux GNU x86_64 and Windows MSVC
x86_64) and one aggregation job, with 14-day internal artifact retention.
Existing CI calls a shared wheel-build action and keeps its normal 18-job
compatibility matrix. An explicit manual `release_candidate=true` CI invocation
routes to P7 instead of that matrix, allowing branch-only testing before the
new dedicated workflow is registered on the default branch. This is the only
new CI routing; candidate production never runs automatically on push or PR.

Local certification passed: 382 Rust tests, strict core/binding checks, 21
release-tool tests, Python 3.9 grammar/Ruff checks across nine files, 15 SDK tests,
five isolated-distribution tests plus the P6 CI profile, native Linux extracted
CLI V1/V2 smoke, fresh ABI3 wheel, actionlint/YAML inspection, both dependency
policy/audit gates, package verification (175 files), and unchanged protected
compatibility. Local artifacts are explicitly marked dirty-source rehearsals;
complete candidate assembly requires matching clean commits. Narrow LF attributes
keep packaged documents and lockfile provenance stable across host checkouts.
Full evidence and artifact hashes are in
`RELEASE_ARTIFACTS_AND_ENGINEERING.md`. Windows native artifact execution and the
complete real two-platform bundle require hosted certification. Synthetic unit
fixtures are not Windows build evidence. No 16 GiB dataset execution is part of
P7.

P7 preserves engine/planner semantics, V1/V2 formats and protected hashes, exact
bytes, P3/P4/P5 contracts, public APIs, product identity, dependencies, MSRV, ABI3,
and development `VERSION` 0.1.0. It introduces no public release, registry upload,
tag, signature, installer, supply-chain certification claim, or production-
readiness claim. P8 remains **DEFERRED** until P7 hosted closure; Adaptive Compute
and V3 remain separate future programs.

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
