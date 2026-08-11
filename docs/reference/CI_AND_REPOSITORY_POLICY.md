# CI and repository policy

Status: Phase 14 baseline

DataPack's local and hosted checks are pinned to Rust 1.85.0. The crate also
declares `rust-version = "1.85"`; `rust-toolchain.toml` makes the exact
certification toolchain and required rustfmt/Clippy components reproducible.

## Hosted CI

`.github/workflows/ci.yml` runs on pushes, pull requests, and manual dispatch.
It grants only read access to repository contents.

The workflow has three independent jobs:

1. **Rust 1.85** runs formatting, `cargo check --locked`, the full Rust test
   suite, and strict all-target/all-feature Clippy on Linux and Windows.
2. **Python SDK foundation** checks and lints the separate binding crate,
   builds an ABI3 wheel with maturin, installs the wheel on Python 3.14, and
   runs the installed-package tests.
3. **Dependency policy and package** runs cargo-deny and cargo-audit for both
   Rust manifests/lockfiles, then verifies `cargo package --locked` for the
   core crate.

Hosted CI configuration is locally reviewable, but a successful hosted run can
only be reported after the workflow is executed by GitHub. No workflow run is
fabricated as part of the local modernization.

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

`cargo package --locked` verifies the core Rust crate. No fabricated repository,
homepage, security contact, or release metadata is added. The Python package is
an alpha checkout-based foundation and is not published by this workflow.
