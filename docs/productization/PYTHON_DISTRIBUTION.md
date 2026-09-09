# DataPack Python Distribution

Status: **CERTIFIED — Productization P2**

This document defines the wheel-first Python distribution contract implemented
in P2. It does not authorize a registry upload or claim that a public release
exists.

## 1. Product identity

| Identity | Value | Evidence |
| --- | --- | --- |
| Product | DataPack | **IMPLEMENTED** |
| PyPI distribution | `datapack-engine` | **IMPLEMENTED** in package metadata |
| Python import | `datapack` | **CERTIFIED** from an installed wheel |
| Version | `0.1.0` | **CERTIFIED** against canonical `VERSION` |
| CLI executable in the Python wheel | none | **CERTIFIED** by the wheel-content allowlist and installed entry-point check |
| crates.io package | none | **DEFERRED**; the internal Rust package remains `datapack` and non-publishable |

`datapack-engine` and `datapack` intentionally differ. A wheel is queried as
the `datapack-engine` distribution through `importlib.metadata`, while
application code uses `import datapack`.

## 2. Runtime and artifact support

| Dimension | Contract | Evidence |
| --- | --- | --- |
| Python implementation | GIL-enabled CPython | **IMPLEMENTED** |
| Python versions | 3.9, 3.10, 3.11, 3.12, 3.13, and 3.14 | **CERTIFIED** locally on Linux and in hosted isolated-install matrices on Linux and Windows |
| ABI | CPython stable ABI, floor `abi3-py39` | **CERTIFIED** by `cp39-abi3` wheel tags and reuse of one wheel across six CPython minors |
| Linux | GNU x86_64, glibc 2.17 or newer | **CERTIFIED** locally and hosted as manylinux2014 / `manylinux_2_17_x86_64` |
| Windows | MSVC x86_64 | **CERTIFIED** by hosted build, inspection, and six-version isolated execution; local build **NOT RUN** because Windows Application Control blocked `rustc.exe` with OS error 4551 |

The exact Linux filename policy is:

```text
datapack_engine-{version}-cp39-abi3-manylinux_2_17_x86_64.manylinux2014_x86_64.whl
```

The wheel contains both equivalent Linux tags:

```text
cp39-abi3-manylinux_2_17_x86_64
cp39-abi3-manylinux2014_x86_64
```

The exact Windows filename and tag policy implemented in CI is:

```text
datapack_engine-{version}-cp39-abi3-win_amd64.whl
cp39-abi3-win_amd64
```

Hosted run `32698666658` produced and accepted the exact Windows filename and
tag, then reused that one platform wheel across CPython 3.9 through 3.14.

Free-threaded CPython uses a different ABI family and is not part of this
`abi3-py39` contract.

## 3. End-user installation contract

Until publication is separately authorized, install only a wheel obtained as a
reviewed CI artifact:

```bash
python -m pip install /path/to/datapack_engine-0.1.0-cp39-abi3-PLATFORM.whl
python -c "import datapack; print(datapack.__version__)"
```

An end user installing a matching wheel does not need a source checkout, Rust,
Cargo, maturin, or a compiler. The wheel includes the `datapack` Python package,
the native extension, type stub, `py.typed` marker, package metadata, and MIT
license. It has no Python runtime dependencies and exposes no console script.

`pip install datapack-engine` is the intended future registry UX, but it is not
valid project installation guidance until a separately authorized publication
occurs. The unrelated PyPI project named `datapack` is not this engine.

## 4. Build architecture

The package remains a thin adapter:

```text
Python caller
    -> datapack Python package
    -> PyO3 native adapter
    -> datapack::application
    -> DataPack core
```

There is one implementation of analysis, planning, compression,
decompression, validation, comparison, archive handling, security limits, and
integrity behavior. Python does not reproduce engine semantics.

The binding crate has an exact root path dependency:

```toml
datapack = { version = "=0.1.0", path = ".." }
```

Accordingly, a wheel builder needs a complete repository checkout. This is
build dependence, not install dependence: the compiled wheel is self-contained
for its matching platform.

P2 pins the build backend to maturin 1.14.1, PyO3 to 0.29.0, Rust/Cargo to
1.85.0, and the ABI feature to `abi3-py39`. Release builds strip linker debug
data and remap workspace/home paths. Maturin's automatically generated Rust
SBOM is disabled because path dependencies currently record build-machine
absolute paths; it may be re-enabled only when that data is sanitized.

The local Linux certification uses maturin's supported Zig strategy with an
explicit manylinux2014 policy. Hosted Linux uses a pinned maturin action and a
manylinux2014 x86_64 container. Maturin's auditwheel check must accept the
artifact; relabeling a host-built wheel is not permitted.

## 5. Linux portability evidence

The locally certified release wheel is constrained to glibc 2.17. Auditwheel
reported only policy-provided `libc`, `libm`, `libpthread`, and `libdl` symbol
dependencies, with no grafted native library. This is the narrowest useful
target supported by Rust 1.85's glibc floor and is represented truthfully by
manylinux2014 / `manylinux_2_17_x86_64`.

The same unchanged wheel was installed and executed locally on:

- CPython 3.9.25;
- CPython 3.10.21;
- CPython 3.11.16;
- CPython 3.12.14;
- CPython 3.13.15; and
- CPython 3.14.7.

All 18 installed-distribution tests passed: three tests per interpreter, zero
failures. This is **CERTIFIED** local ABI3 evidence, not merely a configuration
claim.

The final local Linux wheel is 966,326 bytes with SHA-256
`7e4d12293ba1bea8693d7bd23f7016c2163874337a06ce4b742e565d59d4e22d`.
The hosted Windows wheel is 729,926 bytes with SHA-256
`311ec7a6fe4de51e4c1f73884d4aec25fd8f3024e7348ee726250435c4d5b946`.

An immediate second local build from the same checkout, toolchain, target, and
environment was byte-identical (`cmp` and SHA-256). This is **OBSERVATIONAL**
same-environment reproducibility evidence, not a claim that independent runners
are bit-for-bit reproducible.

## 6. Isolated-install certification

`scripts/certify_python_wheel.py` implements the distribution gate:

1. require exactly one wheel and its exact platform filename;
2. verify distribution metadata, version, Python floor, license, classifiers,
   project URL, maturin version, ABI tags, and `RECORD`;
3. require the exact runtime-content allowlist and reject unexpected tests,
   fixtures, caches, build output, SBOMs, entry points, unsafe paths, common
   secret markers, and developer-specific absolute paths;
4. copy only the final wheel, installed-test harness, and Technical Beta II CI
   harness to an operating-system temporary directory outside the repository;
5. create a fresh virtual environment with the selected matrix interpreter;
6. remove `PYTHONPATH`, `PYTHONHOME`, Cargo, and rustup environment influence;
7. restrict `PATH` so Rust, Cargo, and maturin are unavailable;
8. install with `pip --no-index --no-deps`;
9. run both copied tests under Python isolated mode from the temporary directory;
10. prove `datapack` and its native module resolve inside the temporary virtual
    environment and no repository path occurs in `sys.path`;
11. execute the installed public-SDK smoke cases; and
12. execute the Technical Beta II CI profile across deterministic structured
    workloads, format fidelity, edge behavior, cancellation/retry, resource
    limits, corruption, and Compare, including independent byte and SHA-256
    checks.

The Technical Beta II harness is a certification input, not a wheel member.
The exact runtime-content allowlist remains unchanged. See
`TECHNICAL_BETA_II.md` for its evidence and scope boundaries.

P6 local certification rebuilt the Linux manylinux2014 `cp39-abi3` wheel with
maturin 1.14.1 and passed this complete isolated gate on CPython 3.14.4,
including all five installed-distribution tests and the P6 CI profile. The
full manual-beta profile, 15 SDK tests, and installed stub/runtime signature
checks also passed against that current wheel. P6 hosted certification remains
**PENDING / NOT RUN**; the P2 hosted evidence below is historical.

## 7. Hosted CI

The existing Rust Ubuntu, Rust Windows, Python SDK foundation, and dependency /
package jobs remain present. P2 adds:

- one release wheel build for Linux GNU x86_64;
- one release wheel build for Windows MSVC x86_64;
- exact metadata, ABI tag, content, license, and path-hygiene inspection before
  artifact transfer;
- temporary GitHub Actions artifact upload with 14-day retention; and
- a 2-platform by 6-Python test matrix that downloads and reuses the same wheel
  built for each platform.

These are **CI ARTIFACTS**, not public releases. No workflow step creates a
GitHub Release, pushes a tag, uploads to PyPI/TestPyPI, configures trusted
publishing, or uploads to crates.io.

The first hosted P2 run, `32697219488`, **FAILED** after the Linux wheel built:
inspection found `/home/runner/` source-location strings in the native
extension, so Linux artifact upload and the downstream matrix were skipped. The
failure remains historical evidence and is not rewritten as success. A narrow
Linux-only `${GITHUB_WORKSPACE}` path remap corrected the cause without changing
the certifier or its wheel-content and absolute-path policy.

Hosted run `32698666658` has result **SUCCESS** and evidence status
**CERTIFIED** at technical checkpoint
`cc128bb687b1cfbe64c6b6c7b912f31862176588`. Both platform wheels built, passed
exact inspection, and uploaded as CI artifacts. All 12 isolated jobs passed:
the same Linux wheel and the same Windows wheel were each installed and
exercised on CPython 3.9, 3.10, 3.11, 3.12, 3.13, and 3.14. The legacy/core,
Python SDK, dependency-policy, and package jobs also passed.

## 8. Wheel contents and typing

The certified allowlist contains only:

- `datapack/__init__.py`;
- one platform-native `datapack/_native` extension;
- `datapack/_native.pyi`;
- `datapack/py.typed`;
- distribution `METADATA`;
- distribution `WHEEL`;
- `licenses/DATAPACK-LICENSE-MIT` (automatically checked against the canonical
  root `LICENSE-MIT`);
- distribution `RECORD`.

Typing is **IMPLEMENTED** through the shipped native-module stub and PEP 561
marker. P2 does not generate a second Python implementation or runtime schema.

## 9. Source distribution status

Status: **DEFERRED**.

The P2 audit proved that maturin can assemble an sdist containing the root Rust
crate and binding crate, and that an extracted copy can build without the
original repository. However, that artifact exposes the complete source build
contract, requires Rust/Cargo/a native toolchain, includes substantially more
than the runtime wheel, and its normal PEP 517 build produced a native
`linux_x86_64` wheel rather than the certified manylinux artifact.

P2 therefore does not publish, certify, or promise an sdist. Wheel-first
distribution covers the authorized platforms without restructuring or
duplicating Rust source. A future release phase may revisit sdist policy if a
supported source-build user contract is explicitly desired.

## 10. Publication and limitations

- PyPI publication or namespace reservation: **NOT RUN / NOT AUTHORIZED**.
- TestPyPI publication: **NOT RUN / NOT AUTHORIZED**.
- crates.io publication or namespace reservation: **NOT RUN / DEFERRED**.
- GitHub Release or release tag: **NOT RUN / NOT AUTHORIZED**.
- Windows P2 artifact execution: **CERTIFIED** in hosted CI across CPython 3.9
  through 3.14; local compilation remains **NOT RUN** because Windows
  Application Control blocks `rustc.exe` with OS error 4551.
- macOS, Linux musl, non-x86_64 systems, PyPy, and free-threaded CPython:
  **DESIGN ONLY / OUTSIDE THE P2 SUPPORT CONTRACT**.

DataPack remains pre-1.0 alpha productization work. P2 distribution evidence is
not a production-readiness, enterprise-certification, performance, or external
security-audit claim.
