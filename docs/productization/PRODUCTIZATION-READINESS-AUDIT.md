# DataPack Productization Readiness Audit

Status: **CERTIFIED — P0 read-only audit**

Audit date: 2026-08-23

Audited branch: `productization/foundation`

Audited commit: `e782ea8c361f89e3aa111fe2e15e916c127fc737`

Certified modernization checkpoint: `modernization-complete-2026-08-23`
(`6ddc50f695b918e1f6c7daaeedd8d3674ef753d7`)

This document records the repository state before Productization implementation.
It does not reopen a modernization phase and does not claim that a recommendation
has been implemented.

## 1. Evidence labels

- **FACT** — directly observed in the audited checkout, command output, certified
  project evidence, or the named public registry page on the audit date.
- **INFERENCE** — a conclusion drawn from one or more facts. It is not implemented
  behavior.
- **RECOMMENDATION** — proposed Productization work. It remains unimplemented
  until a later phase supplies code, tests, documentation, and certification.

## 2. Executive verdict

**FACT:** DataPack has a certified engine, frozen v1/v2 compatibility evidence,
typed Rust application services, transactional compression/decompression, a thin
PyO3 adapter, and successful hosted Linux, Windows, Python, dependency-policy,
and package gates.

**INFERENCE:** The core is suitable as the protected technical base of a product,
but the repository is not yet a reproducible public distribution. The strongest
parts are archive correctness, integrity, bounded v2 processing, service reuse,
and CI. The weakest parts are release identity, public package naming, artifact
production, wheel coverage, long-running operation control, and installation
documentation.

**RECOMMENDATION:** Preserve the default phase order. Stop after P0 for an explicit
public distribution-name decision because both exact registry names currently in
the manifests are owned by unrelated projects. Do not begin P1 version or artifact
policy work until the intended Rust and Python distribution identities are known.

## 3. Required P0 report

### 3.1 Repository state

| Required fact | Evidence | Assessment |
| --- | --- | --- |
| 1. Current branch | **FACT:** `productization/foundation` | Correct Productization branch. |
| 2. HEAD | **FACT:** `e782ea8c361f89e3aa111fe2e15e916c127fc737` | Matches the certified baseline supplied for this program. |
| 3. Upstream | **FACT:** `origin/productization/foundation` | Correct upstream; no push was performed. |
| 4. Worktree | **FACT:** `git status --porcelain=v2 --untracked-files=all` was empty before the audit documentation was created. Ignored local build caches exist under `target/`, `fuzz/target/`, and Python `__pycache__/` directories. | Clean/known source state. Existing ignored caches were preserved. |

### 3.2 Rust and package version state

**FACT:** The root package is both a library and CLI named `datapack`, version
`0.1.0`, edition 2021, with `rust-version = "1.85"`. The exact repository
toolchain is Rust 1.85.0 with rustfmt and Clippy. The root `Cargo.lock` identifies
the local `datapack` package as 0.1.0.

**FACT:** `cargo run --locked -- --version` prints `datapack 0.1.0`. Clap derives
this value from the root Cargo package version. It does not print a commit, build
profile, target, release channel, or supported archive versions.

**FACT:** The separate binding crate is `datapack-python` 0.1.0 and depends on the
root crate through `datapack = { version = "=0.1.0", path = ".." }`. The Python
distribution metadata is `datapack` 0.1.0, and `datapack.__version__` is a fourth
literal `0.1.0`. The Python lockfile records both Rust packages at 0.1.0.

**FACT:** Package/application version 0.1.0 and archive header versions 1 and 2
are separate values in code. No release policy currently defines their formal
relationship.

**INFERENCE:** Version values are coherent today but are manually duplicated and
can drift. A package version bump does not technically require an archive-format
change, but that independence is documented only implicitly rather than governed
by release policy.

**RECOMMENDATION:** P1 should define one release version source and explicit
cross-language consistency checks. It must state that SemVer package releases
and `.dpack` wire versions are independent. Retain Rust 1.85.0 as the certified
MSRV/toolchain until a separately approved migration.

### 3.3 Python packaging state and supported contract

**FACT:** `python/pyproject.toml` uses maturin 1.x and PyO3 0.29.0. PyO3 enables
`abi3-py39`, the extension module is `datapack._native`, the Python source is
under `python/python`, and `requires-python` is `>=3.9`. Classifiers list Python
3.9 through 3.14 and Rust. The package has no runtime Python dependencies.

**FACT:** The wheel includes the intended native extension through maturin and
the source package contains `__init__.py`, a typed native stub, and `py.typed`.
The workflow does not inventory the finished wheel contents or assert an exact
platform tag.

**FACT:** Hosted CI builds one Linux ABI3 wheel with the Python 3.14 interpreter,
installs it into the same checkout environment, and runs four installed-package
tests. It does not build a Windows wheel, upload the Linux wheel, test the Python
3.9 floor, test multiple Python interpreters, or test installation from a directory
that cannot see the source checkout.

**FACT:** A completed wheel is self-contained enough for the certified installed-
package tests, but building that wheel currently requires the complete DataPack
checkout because `python/Cargo.toml` has a path dependency on `..`.

**Answer to required question 7:** No, the wheel cannot currently be built without
the full checkout. A built wheel is intended to install without the checkout, but
the present CI does not prove source invisibility in a truly isolated smoke test.

**Answer to required question 8:** The declared contract is Python 3.9 or newer
using the CPython stable ABI with a Python 3.9 floor. The hosted execution evidence
is Linux Python 3.14 only.

**FACT:** Metadata includes name, version, description, README, Python requirement,
MIT license text, and classifiers. It lacks project URLs, keywords, authors or
maintainers, an explicit operating-system classifier set, and a documented wheel
platform policy. `python/README.md` explicitly calls the surface a checkout-built
foundation rather than production packaging.

**INFERENCE:** ABI3 is a strong foundation, but the claimed Python range is broader
than the tested matrix. The project is publication-build capable from a checkout,
not yet public-distribution ready.

**RECOMMENDATION:** P2 should build Linux x86_64 and Windows x86_64 ABI3 wheels,
test the Python 3.9 floor plus the newest supported interpreter, inspect wheel
contents and tags, and install/test from an isolated directory. Publication must
remain disabled.

### 3.4 Public registry identity — approval required

**FACT:** On 2026-08-23 the exact PyPI distribution name `datapack` resolves to
an unrelated project, version 0.0, described as “Data Packages for Science,”
released in 2017: <https://pypi.org/project/datapack/>.

**FACT:** On 2026-08-23 `cargo search datapack` resolves the exact crates.io name
`datapack` to an unrelated project, version 2.0.1, described as Minecraft datapack
tooling: <https://crates.io/crates/datapack>.

**INFERENCE:** The existing `project.name = "datapack"` cannot be publicly uploaded
to PyPI, and the root Cargo package cannot be publicly uploaded to crates.io under
its current exact name, unless ownership is transferred by the current registry
owners. Repository, CLI executable, Rust library target, and Python import names
need not be identical to registry distribution names.

**RECOMMENDATION — DECISION REQUIRED:** The owner must choose or authorize public
Rust and Python distribution names, or explicitly declare one registry out of
scope. The CLI command and Python import can remain `datapack` if desired. Do not
reserve, claim, transfer, or publish a registry name automatically.

### 3.5 Release workflow and artifact state

**FACT:** The repository has one workflow, `.github/workflows/ci.yml`. It has no
tag/release trigger, release workflow, artifact upload, GitHub Release creation,
checksum generation, artifact manifest, provenance statement, signing step, or
isolated CLI artifact smoke test.

**FACT:** The current CI compiles/tests the Rust crate on hosted Linux and Windows,
builds and installs one Linux Python wheel, runs dependency policy/audit checks,
and runs `cargo package --locked`. The workflow has read-only repository
permissions.

**FACT:** `cargo package --locked --list` reports 149 packaged paths and warns that
the manifest has no documentation, homepage, or repository metadata. The package
is broad: it contains workflows, architecture/security documents, all RFCs and
reports, scripts, samples, source, benches, tests, golden files, and protected
fixtures. No wheel, executable release bundle, checksum, or generated benchmark
report is tracked.

**FACT:** There is no `CHANGELOG.md`, release-note file/directory, release artifact
document, or release manifest. Productization tags do not exist. Existing RFC and
modernization tags are intact.

**Answer to required question 9:** Release engineering is absent beyond CI package
verification and an ephemeral Linux wheel build. There are no user-downloadable,
checksummed release artifacts in the repository workflow.

**INFERENCE:** The repository can prove source correctness but cannot reproduce a
complete end-user release or establish which files constitute one release.

**RECOMMENDATION:** P1 should define pre-1.0 channels, changelog/release-note policy,
supported targets, artifact naming, and checksum/manifest conventions. P7 should
implement release CI and isolated artifact smoke tests. Do not publish or create a
formal stable release without explicit authorization.

### 3.6 Current CLI version behavior

**Answer to required question 10 — FACT:** `datapack --version` is enabled through
Clap's package-version behavior and prints exactly `datapack 0.1.0`. There is no
long version or build metadata command. Archive version and backend are reported by
operation results/profile/validation, not by `--version`.

**INFERENCE:** The current output is predictable but insufficient for field issue
reports and artifact provenance.

**RECOMMENDATION:** Preserve the simple one-line form. P1/P5 may add a separate
machine-readable version/details surface or a carefully compatible long-version
option containing package version, commit/build metadata when available, target,
and supported archive versions.

### 3.7 Progress architecture and long-running UX

**FACT:** `datapack::application` is the progress source of truth. Every public
Rust service has a silent synchronous function and a `*_with_progress` function.
`ProgressEvent` contains typed operation, phase, lifecycle state, completed/total
bytes, and completed/total items. Events exclude paths, rows, field values, names,
hashes, and presentation strings. Observers execute synchronously on the calling
thread.

**FACT:** I/O observers rate-limit advanced byte events to five-second intervals.
V2 compression/decompression adapt chunked-pipeline facts into byte and chunk
counters. The CLI derives phase labels, elapsed time, percentage, ETA, and
throughput. It does not parse rendered output.

**FACT:** Analyze, Validate, and Compare currently wrap internal bulk calls with
start/completion events and do not provide useful intermediate events during those
calls. The CLI uses typed progress for Compress and Decompress, but its Analyze,
Validate, and Compare adapters call silent paths. Python exposes no progress
callbacks. No callback-overhead measurement exists.

**Answer to required question 11:** The architecture is correctly typed and shared,
but it is a technical foundation rather than the complete product-grade contract.

**INFERENCE:** Future Desktop can consume the Rust event model without parsing
terminal strings, but current event coverage would leave several long-running
operations visually stalled. Synchronous callbacks can also perturb Benchmark
measurements or block the operation when consumers are slow.

**RECOMMENDATION:** P3 should retain the existing source of truth, define monotonic
counter and lifecycle invariants, fill practical intermediate-event gaps, adapt the
same facts into Python, document callback threading/reentrancy expectations, and
measure callback overhead. Derived percentage/elapsed/throughput should only be
reported where truthful.

### 3.8 Cancellation and transactional output

**FACT:** The public Rust Application API, CLI, and Python SDK expose no cancellation
token or cancellation outcome. The v2 pipeline has an internal `AtomicBool` used to
shut down sibling stages after pipeline failure; it is not caller-controlled
cancellation.

**FACT:** Compression and decompression write through sibling `TempOutput` files.
On ordinary failure the temporary output is removed unless `keep_partial` is true.
Final output commit occurs only after successful writing/verification; overwrite
preserves and restores an existing output around commit where possible.

**Answer to required question 12:** There is no cooperative user cancellation.
Transactional output provides the cleanup mechanism P4 needs, but cancellation
polling and typed result semantics do not yet exist.

**FACT:** `DatapackError` is a public enum and is not marked `#[non_exhaustive]`.
Downstream Rust code can therefore match all current variants exhaustively.

**INFERENCE:** Adding a `Cancelled` error variant to the existing enum would be a
source-breaking API change for exhaustive downstream matches. Reusing
`InvalidFormat` or parsing an error message would not satisfy the typed-cancellation
requirement.

**RECOMMENDATION:** P4 should prefer an additive controlled-operation API, token,
and typed outcome/error that preserves existing synchronous signatures. If a change
to `DatapackError` is proposed, stop for explicit breaking-change approval. Test
compression, decompression, validation, comparison/benchmark where practical,
temporary cleanup, `keep_partial`, overwrite preservation, Python interruption,
and commit-point behavior.

### 3.9 Public Rust API readiness

**FACT:** The public application boundary has owned, path-based request types,
constructors with defaults, `#[non_exhaustive]` request/result/options enums and
structs in most of the versioned surface, versioned reports, typed backend/mode
facts, silent service calls, and typed progress observers. It is independent of
Clap and terminal output. The CLI and Python adapter reuse it.

**FACT:** Analyze, Compress, Decompress, Validate, Compare, and legacy Benchmark are
public. Advisor is not a public application service. Errors are typed Rust variants,
but they expose no stable machine-readable category/code method. Several variants
combine unrelated configuration, format, resource, and operational failures under
`InvalidFormat(String)`. Result diagnostics have codes; fatal errors generally do
not.

**Answer to required question 13:** The core service direction, path ownership,
transaction controls, archive facts, and versioned report shapes are sufficiently
stable to build Productization adapters on. Error evolution, cancellation,
long-running progress coverage, version introspection, and the intended Advisor
boundary remain beta-quality.

**RECOMMENDATION:** Do not rewrite the functional API. Prefer additive methods and
new entry points. P5 should add stable error categorization/actionability without
forcing callers to parse display text and without changing v1/v2 behavior.

### 3.10 Python API readiness

**FACT:** Python calls the Rust Application API through PyO3 and does not duplicate
compression, planning, comparison, or validation. It accepts `str` and
`os.PathLike`, including `pathlib.Path`; exposes v1/v2 option objects; releases the
interpreter during Rust work; returns Rust report data as dictionaries; ships type
information; maps Rust variants structurally into typed exception subclasses; and
preserves overwrite, resource-limit, validation, and `keep_partial` semantics.

**FACT:** The surface is synchronous and lacks progress, cancellation, Benchmark,
Advisor, native package/build details, and a public wheel matrix. The Python version
is hard-coded separately from build metadata. Installed-package coverage consists
of four tests on hosted Linux Python 3.14.

**Answer to required question 14:** It is a coherent SDK foundation and useful for
technical beta work, not yet a public-distribution-quality SDK.

**RECOMMENDATION:** Preserve the architecture and simple synchronous calls. P2
should solve distribution/metadata/isolation first; P3/P4 should adapt shared Rust
progress and cancellation rather than adding Python engines; P5 should improve
discoverability, version reporting, exceptions, and examples.

### 3.11 Desktop reuse readiness

**FACT:** The owned Rust requests/results, typed privacy-safe progress, path-based
services, transactional outputs, and adapter-independent core are reusable by a
Tauri backend. No Desktop code, Tauri project, IPC DTO policy, command allowlist,
security model, background task manager, cancellation handle, or desktop artifact
pipeline exists.

**Answer to required question 15:** Service reuse is promising, but Desktop is not
ready to build safely until versioned distribution, product-grade progress,
cancellation, stable error projection, and user-consumable Rust artifacts exist.

**RECOMMENDATION:** Keep P8 last. First create
`docs/productization/DESKTOP_ARCHITECTURE.md`; keep all compression/planning/codecs
in Rust; define narrowly versioned IPC DTOs and local-file access rules before a
large UI.

### 3.12 CI and release gaps

**FACT:** Existing hosted certification covers Rust 1.85 on Linux and Windows,
Python foundation on Linux, dependency/package policy, strict Clippy, locked builds,
audit visibility, and crate package verification. Protected fixture tests remain in
the Rust suite.

**Answer to required question 16 — gaps:**

- no wheel matrix across Linux/Windows;
- no Python 3.9 floor execution;
- no isolated source-invisible installed-wheel test;
- no exact wheel-content or tag assertion;
- no release-profile CLI artifact build in hosted CI;
- no uploaded workflow artifacts;
- no checksum or artifact-manifest verification;
- no tag/release workflow, provenance, or release smoke test;
- no version-consistency gate across Rust/Python literals;
- no progress callback-overhead evidence;
- no cancellation tests; and
- no changelog/release-note validation.

**RECOMMENDATION:** Add these gates in their owning phases. Preserve all existing
gates and do not add wall-clock performance thresholds.

### 3.13 Installation and documentation gaps

**FACT:** The root README documents commands, formats, safety, APIs, performance,
and source-tree build/test use. It does not provide a supported binary download,
checksum verification, `cargo install` instruction tied to a published crate,
wheel installation command tied to a published index or local artifact, uninstall/
upgrade guidance, release-channel meaning, supported-platform table, or artifact
selection guide.

**FACT:** `python/README.md` documents checkout builds only. `SECURITY.md` is
substantive about local threat boundaries, compatibility, resource limits, and
transactional output, but explicitly has no private reporting address or advisory
URL. Compatibility and performance methodologies are well documented. There is no
changelog or release-note state.

**Answer to required question 17:** A user cannot currently discover a supported
external install path, verify a download, determine update compatibility, or follow
a published release history from repository documentation.

**RECOMMENDATION:** P1/P2/P7 should add artifact-centered installation and update
documentation only after those artifacts exist. Do not document a registry command
before ownership and publication are authorized.

### 3.14 Highest-risk Productization issues

| Priority | Evidence label | Risk | Why it matters | Owning phase |
| ---: | --- | --- | --- | --- |
| 1 | **FACT / DECISION REQUIRED** | Exact Rust and PyPI distribution names are owned by unrelated projects. | Public publication cannot use current manifest names without ownership; the choice is externally persistent. | P1/P2 decision before implementation |
| 2 | **FACT** | No reproducible release workflow or artifact contract. | Users cannot obtain or verify a supported build. | P1, P7 |
| 3 | **FACT** | No cooperative cancellation. | Long operations cannot be stopped safely through public APIs or future Desktop. | P4 |
| 4 | **FACT** | Public `DatapackError` is exhaustively matchable. | A naive cancellation variant is source-breaking. | P4/P5 design |
| 5 | **FACT** | Python 3.9+ claim is tested only on Linux Python 3.14. | Wheel/tag/ABI/platform regressions could escape. | P2 |
| 6 | **FACT** | Progress is typed but incomplete for bulk Validate/Compare/Analyze and absent in Python. | Long-running operations can appear stalled. | P3 |
| 7 | **FACT** | Four duplicated 0.1.0 literals plus two lockfiles. | Releases can report inconsistent versions. | P1 |
| 8 | **FACT** | Installed-wheel CI retains the source checkout. | It does not fully prove install independence. | P2 |
| 9 | **FACT** | Fatal errors lack stable public codes/categories. | Automation/Desktop must otherwise infer behavior from variants/messages. | P5 |
| 10 | **FACT** | No private security reporting channel. | Vulnerability intake is not operationally ready. | P1/P7 owner action |

### 3.15 Recommended phase order

**Answer to required question 19 — RECOMMENDATION:** Keep the supplied order:

1. P0 — Productization Readiness Audit;
2. P1 — Release Foundation;
3. P2 — Python Distribution Foundation;
4. P3 — Progress API;
5. P4 — Cooperative Cancellation;
6. P5 — Public API / Error Product Polish;
7. P6 — Technical Beta II;
8. P7 — Release Artifacts / Release Engineering; and
9. P8 — Desktop Foundation.

**INFERENCE:** No implementation dependency justifies reordering. P3 should define
an additive operation-control seam that P4 can extend, and P4 must avoid an
unapproved breaking error change. P1/P2 cannot finalize public artifact/package
names until the distribution-identity decision is made.

### 3.16 Evidence-based changes recommended to the master plan

**Answer to required question 20 — RECOMMENDATIONS:**

1. Add an explicit registry-identity decision before P1 changes version/artifact
   naming. Treat CLI name, Rust crate/library name, Python import name, and public
   registry distribution names as separate decisions.
2. Add a cross-language version-consistency gate in P1; do not tie package SemVer
   to `.dpack` v1/v2.
3. Require P2's installed-wheel test to run outside the checkout and require both
   Linux x86_64 and Windows x86_64 wheel artifacts before calling the matrix ready.
4. Require P3's progress design to anticipate additive P4 operation control without
   making progress callbacks themselves a cancellation protocol.
5. Require explicit approval if P4 or P5 proposes modifying the exhaustively
   matchable `DatapackError` enum in a source-breaking way.
6. Keep registry publication, production release creation, signing-key provisioning,
   and private security-channel provisioning as owner-controlled external actions.

These changes refine gates and decision points; they do not reorder P0–P8.

## 4. Readiness classification

### A. Already product-ready

**FACT:** The following technical behavior is ready to preserve and reuse:

- frozen v1/v2 interpretation and byte-exact reconstruction guarantees;
- protected compatibility fixtures and immutable hashes;
- checked parsing/resource controls and default v2 integrity verification;
- transactional Compress/Decompress output and overwrite protection;
- bounded v2 chunk pipeline and safe RawZstd fallback;
- typed, path-based Rust Application API separation from CLI presentation;
- Python-to-PyO3-to-Rust dependency direction;
- deterministic factual Analyze/Validate/Compare/Advisor boundaries;
- strict Clippy, Linux/Windows Rust CI, Python CI, dependency policy, and package
  verification; and
- correctness/performance evidence separation.

### B. Technical-beta quality

**FACT:** The Rust public API, typed progress contract, Python SDK, CLI progress
presentation, package metadata, source package, and performance observation tools
work and are tested, but lack the distribution, operation-control, compatibility
policy, and matrix evidence expected of a public product.

### C. What prevents external installation

**FACT:** No supported binary or wheel is uploaded; no checksum/manifest exists; no
release workflow exists; external install documentation is absent; public registry
names conflict; and the wheel build requires the full checkout.

### D. What prevents safe long-running UX

**FACT:** There is no caller cancellation, several bulk operations expose only
start/end events, Python has no progress, callback cost is unmeasured, and there is
no operation/task handle for Desktop. Transactional cleanup is already available.

### E. What prevents reproducible releases

**FACT:** There is no release/version policy, changelog, release notes, artifact
naming policy, supported-platform contract, checksum/manifest/provenance process,
version consistency check, or release workflow.

### F. What prevents public Python distribution

**FACT:** The exact PyPI name is occupied; ownership/naming is undecided; Windows
and floor-Python wheels are untested; Linux tag policy and wheel contents are not
asserted; isolated source-invisible installation is not tested; metadata is
incomplete; and publication is intentionally unauthorized.

### G. What is missing for future Desktop reuse

**FACT:** Cancellation, complete progress, stable error projection, version/build
facts, release artifacts, background-task ownership, versioned IPC, filesystem
security rules, and Desktop architecture/acceptance criteria are missing.

### H. Public API decisions sufficiently stable

**INFERENCE:** Keep synchronous path-based simple calls, owned requests, versioned
results, privacy-safe typed progress facts, transactional overwrite/partial controls,
explicit v1/v2 option types, typed validation facts, and the shared Rust service as
the only engine. These are well supported by implementation, tests, and docs.

### I. What must not change during Productization

**RECOMMENDATION:** Do not change v1/v2 wire semantics, protected bytes/hashes,
RawZstd fallback honesty, the Rust source-of-truth architecture, transactional
commit guarantees, bounded/resource-checked behavior, strict Clippy, Rust 1.85.0,
security/dependency policy, or hardware-neutral archives. Do not implement v3,
GPU/hybrid compute, Python/Desktop codec duplication, PyPI publication, or a stable
production release under this audit.

## 5. P0 stop decision

**FACT:** Public Rust and Python package distribution currently requires permanent
name/ownership choices.

**RECOMMENDATION:** Stop after the P0 documentation checkpoint and request the
owner's public distribution-name decision. This is the program's specified stop
behavior; it is not a technical failure and does not reopen modernization.
