# CI and repository policy

Status: Phase 14 baseline plus Productization P1-P7 certification controls

DataPack's local and hosted checks are pinned to Rust 1.85.0. The crate also
declares `rust-version = "1.85"`; `rust-toolchain.toml` makes the exact
certification toolchain and required rustfmt/Clippy components reproducible.

## Hosted CI

`.github/workflows/ci.yml` runs on pushes, pull requests, and manual dispatch.
It grants only read access to repository contents.

Ordinary pushes, pull requests, and manual CI runs execute five compatibility
job groups, expanding to 18 required jobs:

1. **Rust 1.85** runs formatting, `cargo check --locked`, the full Rust test
   suite, and strict all-target/all-feature Clippy on Linux and Windows.
2. **Python SDK foundation** checks and lints the separate binding crate,
   verifies product identity/version consistency, builds an ABI3 wheel with
   maturin, installs the wheel on Python 3.14, and runs the installed-package
   tests, including distribution/import version identity.
3. **Python wheel build** builds and strictly inspects one Linux manylinux2014
   ABI3 wheel and one Windows MSVC ABI3 wheel, then uploads each only as a
   temporary CI artifact.
4. **Python wheel test** reuses each platform wheel across CPython 3.9 through
   3.14. Each of the 12 isolated jobs runs the installed public-SDK tests and
   the Productization P6 Technical Beta II CI profile with repository source,
   Rust, Cargo, maturin, and network installation absent.
5. **Dependency policy and package** runs cargo-deny and cargo-audit for both
   Rust manifests/lockfiles, then verifies `cargo package --locked` for the
   core crate.

Hosted CI configuration is locally reviewable, but a successful hosted run can
only be reported after the workflow is executed by GitHub. No workflow run is
fabricated as part of the local modernization.

The Technical Beta II beta-scale profile, release CLI exercise, and external
large-file evidence remain manual/local. Hosted P6 timings are observational;
only correctness assertions gate the jobs. See
`docs/productization/TECHNICAL_BETA_II.md`.

P6 local certification passed and hosted certification passed. Hosted run
[`34404009535`](https://github.com/Gom-svg/Datapack/actions/runs/34404009535)
completed with **SUCCESS** at technical commit
`621574a2607e3857fe932dbe9db8ec6eddc5a9fd`: all 18 required hosted jobs passed.
P6 Technical Beta II is formally closed with status **CERTIFIED — CLOSED**.
The P6 workflow topology was unchanged. P7 now shares the wheel build through
`.github/actions/build-python-wheel/action.yml` and adds an opt-in manual
release-candidate route; normal CI retains all 18 required compatibility jobs.

## Internal release-candidate workflow

`.github/workflows/release-candidate.yml` builds and certifies two native platform
outputs, then aggregates them in a third job. It supports manual dispatch and a
call from CI, guarded by a manual event. CI's boolean `release_candidate` input
defaults to false. Setting it to true on a manual invocation skips the broad
compatibility matrix for that invocation and calls the P7 workflow instead.
This lets the existing registered CI workflow start branch-only P7 certification
without a merge or an automatic push trigger.

Both native jobs use Rust/Cargo 1.85.0, package and extract their CLI, exercise
V1/V2 exact-byte smokes, and certify the platform wheel with the unchanged
isolated SDK/P6 certifier on CPython 3.14. Aggregation requires both platforms,
matching clean source/workflow/lockfile provenance, package inspection, manifest
and checksum verification, and an independent `sha256sum --check` before upload.

All permissions remain `contents: read`. Uploads are internal Actions artifacts
with 14-day retention. No release, registry, tag, signing, or publication action
exists. P7 is **CERTIFIED — CLOSED** at technical commit
`8f70179bd68701ad7dfa7fe909870e880845ad77`. Normal push CI
[`34546542428`](https://github.com/Gom-svg/Datapack/actions/runs/34546542428)
passed all 18 required jobs. Manual `release_candidate=true` run
[`34660317844`](https://github.com/Gom-svg/Datapack/actions/runs/34660317844)
(attempt 1) passed Linux native candidate `103461273675`, Windows/MSVC native
candidate `103461273537`, and aggregation `103462190885`; the normal matrix
was skipped for this invocation.

The final internal artifact,
`datapack-candidate-bundle-8f70179bd68701ad7dfa7fe909870e880845ad77`,
contains the two canonical CLI packages, two standard `cp39-abi3` wheels, notes,
canonical manifest, and checksum file. Both hosted aggregation and subsequent
download verification passed manifest metadata, file-set/size/hash, clean-source
provenance, and independent checksum checks (six entries OK). Both platform
records agree on the technical SHA, `source.dirty=false`, lockfile hashes,
Rust/Cargo 1.85.0, and workflow/run/attempt. See
`docs/productization/RELEASE_ARTIFACTS_AND_ENGINEERING.md`, section 8, for exact
filenames, hashes, native execution evidence, and limitations. Closure changed
documentation only; development `VERSION` remains `0.1.0`, and P8 remains next.

## Local checks

From the repository root:

```bash
bash scripts/check.sh
```

or on native Windows PowerShell:

```powershell
./scripts/check.ps1
```

Both scripts run the same core formatting, check, test, strict Clippy, and
release-build gates with the committed lockfile.

The Python binding has its own commands because it is intentionally a separate
crate; see `python/README.md`.

## Generated artifacts

No `target/` directory belongs in version control, including nested targets
such as `fuzz/target/`. Fuzzer crash artifacts under `fuzz/artifacts/` are also
ignored. The following are source/evidence and remain tracked:

- `fuzz/Cargo.toml` and `fuzz/Cargo.lock`
- every file under `fuzz/fuzz_targets/`
- the checked-in corpus under `fuzz/corpus/`

Phase 14 removes generated `fuzz/target/` files from the current Git index. It
does not rewrite history and does not discard a developer's local build cache.

## Dependency advisory truth

Historical v1 metadata uses bincode 1.3.3. Replacing it would change a frozen
compatibility surface, so `deny.toml` contains one explicit exception for
`RUSTSEC-2025-0141`, which reports that bincode is unmaintained. This is a
**KNOWN ACCEPTED WARNING**, not an assertion that the advisory is absent.

`cargo audit` remains visible in CI and local certification. New vulnerabilities
or advisories are not silently ignored by this compatibility exception.

## Packaging

`cargo package --locked` verifies the core Rust crate. The manifest records the
verified source repository and explicitly sets `publish = false`; crates.io
publication is out of scope for Productization Foundation. The Python package is
an alpha checkout-based foundation whose authorized distribution name is
`datapack-engine` and whose import remains `datapack`. It is not published by this
workflow.

`VERSION` is the canonical package/application version. The root and binding Cargo
versions, exact path dependency, Python distribution version, Python import
version, lockfile entries, CLI identity, and distribution/import identities are
checked by `scripts/check_version_consistency.py`.

P1 defines release/versioning and artifact/checksum policy. P7 implements internal
candidate production without changing `VERSION` or publishing artifacts publicly.
Public distribution remains separately authorized.
