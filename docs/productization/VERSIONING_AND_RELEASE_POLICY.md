# DataPack Versioning and Release Policy

Status: **CERTIFIED — Productization P1**

This policy governs package/application releases. It does not create a release,
publish a package, change an archive format, or claim production readiness.

## 1. Authorized product identity

| Surface | Authorized identity | Policy |
| --- | --- | --- |
| Product brand | DataPack | Do not rename the product. |
| CLI executable | `datapack` | Stable executable/command identity. |
| Python distribution | `datapack-engine` | Future PyPI distribution name; not published or reserved by this phase. |
| Python import | `datapack` | Users continue to write `import datapack`. |
| Rust package/crate | `datapack` | Internal source/package identity. |
| crates.io | Deferred/out of scope | Both Rust crates set `publish = false`; do not publish during Productization Foundation. |

Distribution and import names are intentionally different. If public publication
is separately authorized later, installation may use `pip install datapack-engine`
while code continues to use `import datapack`.

## 2. One product version

`VERSION` is the repository's canonical package/application version. Tool-native
manifests still require mirrored literals. The following values must match it:

- root Rust package version;
- PyO3 binding-crate version;
- the binding's exact root-crate dependency version;
- Python distribution version;
- `datapack.__version__`;
- the root, Python, and fuzz lockfile entries; and
- the CLI version derived from Rust `CARGO_PKG_VERSION`.

`scripts/check_version_consistency.py` enforces the identity and version mirrors.
Hosted CI runs the check. Rust CLI tests prove that Clap reports the Cargo package
version, and installed-wheel tests compare `datapack.__version__` with the
`datapack-engine` distribution metadata.

The current value is `0.1.0`. P1 does not bump it because P1 does not create a
release. A release version is selected only as part of an authorized release
candidate or release preparation change.

P7 internal artifact-production candidates retain this development value. They
exercise release engineering without a SemVer promotion, release tag, or public
release. The release-preparation steps below apply when a release/version change
is separately authorized, not to a P7 development artifact build.

## 3. Semantic Versioning policy

DataPack package/application versions use `MAJOR.MINOR.PATCH` Semantic Versioning,
with optional `-alpha.N`, `-beta.N`, or `-rc.N` prerelease identifiers.

Before 1.0:

- PATCH releases contain backward-compatible fixes, documentation corrections,
  packaging fixes, and compatible security hardening;
- MINOR releases may add product/API capabilities and may contain an explicitly
  approved breaking public API change;
- a breaking Rust, Python, or established CLI proposal requires written
  justification and user approval even though SemVer permits greater flexibility
  before 1.0; and
- `1.0.0` requires explicit authorization and evidence supporting a stable public
  contract. Productization must not arrive at 1.0 implicitly.

After 1.0, normal SemVer compatibility applies: breaking public package/API/CLI
changes require a major package version. Archive compatibility remains governed by
the stricter wire-format policy below, not by package SemVer.

Released versions are immutable. Fixes use a new version; artifacts are never
silently replaced under an existing version.

## 4. Release channels

| Version form | Channel | Meaning |
| --- | --- | --- |
| `X.Y.Z-alpha.N` | Alpha | Internal or narrow technical evaluation; incomplete product contracts are expected. |
| `X.Y.Z-beta.N` | Beta | External workflow evaluation with documented limitations; not a production-readiness claim. |
| `X.Y.Z-rc.N` | Release candidate | Intended release contents and contracts are frozen except for release-blocking fixes. |
| `X.Y.Z` | Stable package release | Supported immutable release of that exact package version. A pre-1.0 stable package remains governed by the pre-1.0 API policy. |

“Stable package release” means the artifacts for that version passed its release
gates. It does not by itself mean `1.0`, production-ready, enterprise-ready, or
universally suitable for every workload.

Prerelease promotion always creates a new version. For example, `0.2.0-beta.1`,
`0.2.0-rc.1`, and `0.2.0` are distinct immutable releases.

## 5. Package version versus archive format

The package/application version and `.dpack` wire-format version are independent:

| Version | Describes | Current implemented values |
| --- | --- | --- |
| Package/application SemVer | DataPack CLI, Rust source package, Python distribution, APIs, fixes, and product artifacts | `0.1.0` development value |
| Archive format version | On-disk `.dpack` interpretation | v1 and v2 only |
| Report schema suffix/version | Shape and semantics of machine-readable API/JSON reports | Existing V1 report contracts |

A package release does not increment the archive version. A new archive version
does not require the package major number to match it. Package `2.0.0`, for example,
would not mean `.dpack` v2.

Every Productization release must continue to interpret valid frozen v1 and v2
archives according to their existing semantics. Existing protected fixture sizes
and SHA-256 values are immutable. A new wire version requires separate explicit
authorization, compatibility design, fixtures, security review, and executable
evidence. `.dpack` v3 remains design only.

## 6. Compatibility and deprecation

- V1/v2 reading and byte-exact reconstruction are release-blocking compatibility
  requirements.
- RawZstd fallback remains an intentional behavior, not a degraded hidden mode.
- Existing simple synchronous Rust and Python calls remain available when progress
  or cancellation variants are added.
- Compatible additive APIs are preferred.
- Breaking public API or established CLI changes require approval, a MINOR bump
  before 1.0 (MAJOR after 1.0), changelog/release-note disclosure, and migration
  guidance.
- Deprecations must identify the replacement and earliest possible removal
  version. No removal occurs merely for aesthetics.
- Versioned JSON/report schemas change only through their documented schema
  evolution rules.

## 7. Supported platform policy

The initial Productization release-target policy is deliberately narrow:

| Surface | Initial target | Current evidence | Release-support gate |
| --- | --- | --- | --- |
| Rust CLI | `x86_64-unknown-linux-gnu` | P7 hosted native candidate and extracted CLI smoke PASS; glibc 2.39 host, highest observed GLIBC symbol 2.34 | P7 release build plus isolated artifact smoke test and recorded runtime baseline |
| Rust CLI | `x86_64-pc-windows-msvc` | P7 hosted native MSVC candidate and extracted `datapack.exe` smoke PASS | P7 release build plus isolated artifact smoke test |
| Python wheel | Linux x86_64, CPython ABI3 | P7 manylinux2014 candidate PASS on CPython 3.14.7; normal CI matrix PASS on 3.9–3.14 | P2 wheel/tag/install matrix |
| Python wheel | Windows x86_64, CPython ABI3 | P7 native MSVC candidate PASS on CPython 3.14.7; normal CI matrix PASS on 3.9–3.14 | P2 wheel/tag/install matrix |

P7 is **CERTIFIED — CLOSED** at technical commit
`8f70179bd68701ad7dfa7fe909870e880845ad77`: normal CI `34546542428` and
candidate run `34660317844` passed, including both native platforms, aggregation,
and downloaded bundle manifest/checksum/clean-source verification. Exact evidence
is in `RELEASE_ARTIFACTS_AND_ENGINEERING.md`, section 8. These are internal
development `0.1.0` artifacts; no released artifact support or additional
older-glibc portability is inferred.

Linux and Windows source CI certification is not silently promoted into released
artifact support. A target becomes supported for a release only when that release's
artifact is built and smoke-tested in the documented workflow.

macOS, ARM/AArch64, musl, mobile platforms, and GPU-specific artifacts are not in
the initial release matrix. WSL may run a compatible Linux artifact, but WSL and
`/mnt/c` performance remain a distinct observational environment rather than a
separate binary target or throughput promise.

## 8. Python support policy

- Python runtime floor: CPython 3.9.
- Binding ABI: CPython stable ABI with the `abi3-py39` floor.
- Packaging metadata: `requires-python = ">=3.9"`.
- Current declared/classified versions: CPython 3.9 through 3.14.
- P2 must execute the oldest supported interpreter and the newest supported
  interpreter, not only rely on ABI3 theory.
- A future CPython release is supported only after the wheel is installed and the
  installed-package suite passes; an absent upper metadata bound is not evidence
  of certification.
- PyPy and other Python implementations are not supported unless separately
  tested and documented.

Dropping the Python floor is a compatibility change requiring justification,
release-note disclosure, and the appropriate SemVer change.

## 9. Rust/MSRV policy

- Declared MSRV: Rust 1.85 (`rust-version = "1.85"`).
- Exact build/certification toolchain: Rust/Cargo 1.85.0.
- Release workflows use the exact certified toolchain and committed lockfiles.
- Both the root and PyO3 crates must remain compatible with that toolchain.
- Changing the MSRV or exact release toolchain requires an explicit toolchain
  migration decision, compatibility evidence, changelog/release-note disclosure,
  and hosted Linux/Windows recertification.
- Frozen bincode 1.3.3 remains subject to the documented compatibility advisory;
  version policy does not authorize replacement of the v1 serialization graph.

## 10. Changelog and release notes

`CHANGELOG.md` is the cumulative repository record. Every user-visible change is
added under `[Unreleased]` using Added, Changed, Deprecated, Removed, Fixed, or
Security categories as appropriate. A release moves relevant entries into a
versioned section with an ISO `YYYY-MM-DD` date. It never rewrites prior release
entries to change history.

`docs/releases/` contains one human-focused release note per released version and
an explicit template. Release notes summarize installation, compatibility,
artifacts, checksums, changes, known limitations, security facts, and certification
evidence. A changelog is exhaustive change history; release notes are the supported
user narrative. Neither document is a substitute for artifact verification.

## 11. Release preparation and authority

An authorized release candidate must, at minimum:

1. start from a clean, reviewed commit on the authorized release/integration branch;
2. update `VERSION` and every checked mirror in one logical change;
3. update lockfiles without unrelated dependency drift;
4. pass identity/version consistency, existing CI, package, compatibility, and
   applicable wheel/artifact gates;
5. finalize changelog and versioned release notes;
6. build artifacts from the documented workflow;
7. generate and verify the artifact manifest and SHA-256 checksums; and
8. record the source commit, toolchain, target, and workflow evidence.

Creating a public release, tag, GitHub Release, PyPI project, or PyPI upload remains
an external action requiring explicit user authorization. crates.io publication is
out of scope for Productization Foundation.
