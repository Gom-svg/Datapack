# P8 — Desktop Foundation

Development version: **0.1.0**. Primary target: **Windows x86_64 / MSVC**.

Status: **IMPLEMENTED — LOCAL CERTIFICATION PASSED**. Hosted P8
certification: **PENDING**. Operator visual acceptance: **PENDING**.
P8 does not close Final Productization Acceptance or authorize publication.

## 1. Recovered baseline and architectural audit

Work started on clean `productization/foundation` at
`dd98805dcee4f3f828b8204b1da7ea78f184579f`, matching origin (`0 / 0`). Both diffs
and the untracked-file inventory were empty. P7's documentation closure, normal
CI `34546542428`, candidate `34660317844`, and closure CI `34820837471` were
preserved. No reset, clean, restore, rebase, or unrelated edit was needed.

The root is a standalone engine/CLI crate, with separate Python and fuzz
manifests/lockfiles; there was no Desktop or frontend infrastructure. The public
`datapack::application` layer already owns Analyze, Compress, Validate,
Decompress, Compare, and Benchmark requests/results. Analyze exposes parser,
sampling, planner, diagnostic, and resource facts. Validation exposes archive
metadata, checks, and optional source agreement. Compression/decompression expose
actual modes, sizes, diagnostics, and applicable verification facts. Compare's
measurement model is available but unnecessary for the essential P8 lifecycle.

P3 observers are synchronous on the caller's thread. P4 uses a cloneable
`CancellationToken` and distinguishes cancellation from failure; success wins
once committed. P5 provides stable `code()`/`category()` alongside Display
context. Those contracts are sufficient without a public-API or format change.
P7 packages native CLIs and ABI3 wheels through a frozen seven-file bundle;
Desktop gets a separate candidate extension and does not change that bundle.

## 2. Shell choice and dependency investigation

Chosen architecture: a **native Win32 Desktop shell in Rust**, through Microsoft's
`windows-sys = 0.61.2`, over a platform-independent adapter and presentation model.
Windows supplies controls, dialogs, fonts, accessibility interfaces, and rendering.
An embedded application manifest enables common-controls V6, PerMonitorV2 DPI,
and ordinary-user execution. There is no webview, JavaScript, embedded browser,
async runtime, graphics backend, downloaded font, or UI compression logic.

The exact bindings version declares Rust 1.71 and `MIT OR Apache-2.0`. Its
`windows-link 0.2.1` dependency is also `MIT OR Apache-2.0`. Both versions already
exist in the certified root lockfile. Desktop adds a direct bindings dependency
and a path dependency on the engine; tests reuse `tempfile`. Its independent
lockfile preserves every registry version and checksum from the root graph.
The version checker enforces this subset relationship. No dependency-policy
allowance or advisory exception was added. Platform-filtered metadata contains
54 packages including the two local crates and build/test dependencies.

Upstream references: [Microsoft Rust bindings](https://github.com/microsoft/windows-rs),
[Windows visual styles](https://learn.microsoft.com/en-us/windows/win32/controls/visual-styles-overview),
and [native common dialogs](https://learn.microsoft.com/en-us/windows/win32/dlgbox/using-common-dialog-boxes).
The actual pinned crate manifests and source were inspected locally.

Alternatives considered:

| Approach | Decision |
| --- | --- |
| egui/eframe 0.31.1 | Initially prototyped: direct Rust integration and Rust 1.81 compatibility were attractive. Rejected at dependency certification: `ttf-parser` has unmaintained advisory [RUSTSEC-2026-0192](https://rustsec.org/advisories/RUSTSEC-2026-0192.html), with no patched version; fonts/clipboard/platform dependencies also require license allowances absent from repository policy. No egui, rfd, font, or graphics dependency remains. |
| Tauri | Webview/IPC, frontend tooling, security configuration, and packaging add unnecessary integration surface for this Windows-first phase. |
| Slint | Licensing/distribution terms and generated UI/build tooling require a broader adoption decision than this phase needs. Not adopted or added. |
| CLI subprocess wrapper | Rejected because typed, path-based Rust services already exist. Parsing terminal text would discard established contracts. |
| Direct native Windows controls | Selected for the smallest dependency and packaging footprint under the frozen MSRV/policy. Tradeoff: platform-specific FFI maintenance and no Linux GUI commitment. |

The earlier egui resolution also selected newer engine transitive packages. That
experimental lockfile was replaced with the frozen core inventory before native
implementation certification. Root, Python, and fuzz locks remain unchanged.

## 3. UI / core boundary

```text
Windows controls and dialogs (ui.rs)
        ↓ owned paths / Job / CompressionChoice
Controller and worker adapter (adapter.rs)
        ↓ public *_with_control(request, OperationControl)
Certified DataPack application layer → engine
        ↓ typed result / error / ProgressEvent
Presentation DTOs (model.rs) → pure text views (presentation.rs)
```

`desktop/` is intentionally a separate package, as `python/` already is. Root
engine builds do not pull in Desktop code. UI never imports codec/storage/planner
internals. It never shells out to `datapack`. `smoke.rs` exercises the same
Controller and adapter through the packaged executable.

The native FFI is confined to `ui.rs`; a boxed `RefCell` outlives the window and
message loop. Reentrant notifications use `try_borrow_mut`; workers cannot touch
controls or window handles. Standard controls provide keyboard focus and native
text selection. DPI changes replace fonts and relayout controls. Result text
scrolls when the window is small. System colors/fonts are used; no custom dark
theme or branding asset is introduced. The Windows default developer icon is
explicitly temporary.

## 4. Workflows and truthful presentation

The start screen explains local compression and exact bytes. Users can select a
file, analyze its structure, choose compression, inspect the result, open the
resulting archive in the validation/restoration workspace, validate against the
original, and restore to a new destination. Filename, size, detected format and
delimiter, recommendation, sampling scope, resource estimates, progress, output
location, ratio, reduction, and integrity evidence are visible without a terminal.

Selection reads metadata on the worker, never contents. Format detection happens
on Analyze; it is not guessed from an extension. Analysis marks partial sampling,
shows bytes and records inspected, and warns that remaining data may differ.
Estimated dictionary memory is explicitly not a process-memory bound. A general
512 MiB presentation threshold highlights substantial estimates; it does not
alter planner selection or special-case a dataset.

Main choices are Automatic / Recommended and Chunked. Automatic passes the
existing V1 request defaults unchanged. Chunked uses the existing V2 API with
8 MiB chunks, two workers, two in-flight chunks, adaptive level disabled, and a
16 MiB admission limit. The limit covers configured in-flight chunk bytes, not
RSS. These conservative Desktop request defaults do not change engine defaults.
Advanced settings explain these facts; low-level tuning controls are deferred.
Validation uses the API's default 512 MiB limit; decompression explicitly uses
the same existing memory-limit option. No expected output size is fabricated:
validation metadata can supply it before restoration.

Compression completion reports actual archive mode, version, input/archive
sizes, ratio, and reduction (including negative savings on expansion). Empty
input has no fabricated ratio. Archive creation does not claim verification.
Validation displays Valid/INVALID, source agreement, archive version, expected
restored size, chunk count, diagnostics, and expandable individual checks.
V2 restoration uses the API's enabled SHA-256 verification. V1 reports that no
embedded SHA-256 exists and asks for against-source validation to establish exact
agreement. Source comparison describes the files actually read; users should
keep them unchanged during operations. No arbitrary file preview is implemented.

## 5. State, progress, cancellation, and output safety

`State` explicitly models Idle, Selecting, FileSelected, Analyzing,
AnalysisReady, Compressing, Cancelling, CompressComplete, Validating,
ValidationComplete, Decompressing, DecompressComplete, Cancelled, and Failed.
Only one operation is admitted. Selection clears stale file/analysis state.
Operation IDs, per-worker channels, and completion matching prevent stale events
from replacing a later operation. Controls recover after failure/cancellation.

Progress uses the P3 event phase and its `percentage()` function, with real byte
counters. Unknown totals use a native indeterminate progress bar. Percentages
are explicitly **of the current phase**, never invented whole-job estimates.
Phase completion alone does not claim operation success. An 80 ms window timer
polls actual worker facts; it does not generate progress. The mailbox stores only
one coalesced snapshot and one completion. UI polling uses `try_lock`, `try_recv`,
and `JoinHandle::is_finished`; it never waits for active engine work. Fast phases
may pass between renders. This is a snapshot display, not an event recorder.

Cancel requests the P4 token, disables repeat cancellation, and shows Cancelling.
The returned operation result remains authoritative, so post-commit success wins.
Early/mid-operation cancellation cannot be rendered as success. Each retry gets
a fresh token. Bounded analysis and whole-file structured phases may have coarse
cancellation granularity; this is the existing core contract, not a UI timer.

Close during work offers safe cancellation and keeps the responsive window alive
until the worker returns. Normal Windows session shutdown is vetoed while work
is running and requests cancellation. Forced OS termination/power loss remains
outside cooperative cancellation guarantees. No worker is force-terminated by
Desktop. Unexpected worker panics become a distinct Desktop failure with an
instruction to inspect output; panic payloads and user paths are not logged.

The adapter never sets `overwrite` or `keep_partial`. The engine remains the
only output safety authority, including commit-time races, aliases, existing
files, cleanup, and transactional semantics. Existing destinations produce an
error and a clear choice to use a different name. The native save dialog never
grants overwrite permission. No second output writer,
Desktop temporary-output format, or cleanup implementation exists.

P5 stable code/category and full contextual Display text are preserved. Users
see a readable primary message; technical identity is secondary. Invalid
validation reports are rendered INVALID even though the service returned `Ok`.
No Rust Debug dump or JSON wall is presented.

## 6. Build, CI, and internal candidate boundary

Native development commands:

```powershell
cargo fmt --manifest-path desktop/Cargo.toml --check
cargo check --manifest-path desktop/Cargo.toml --locked
cargo test --manifest-path desktop/Cargo.toml --locked
cargo clippy --manifest-path desktop/Cargo.toml --all-targets --all-features --locked -- -D warnings
cargo build --manifest-path desktop/Cargo.toml --release --locked
cargo run --manifest-path desktop/Cargo.toml --release --locked
```

Linux can run the same Rust checks and an executable adapter smoke, but has no
window backend. It cannot certify Windows compatibility:

```bash
export CARGO_TARGET_DIR="$HOME/.cache/datapack-modernization"
cargo run --manifest-path desktop/Cargo.toml --release --locked -- \
  --smoke-test /tmp/datapack-desktop-new-smoke
```

The smoke destination must not already exist. On Windows `--ui-smoke` creates the
real top-level window and native controls, pumps the message loop, checks live
controls/visibility, and closes. It is an automated launch boundary, not a visual
acceptance test. Normal launch does not run a smoke or write fixture data.

`.github/workflows/desktop.yml` is reusable and manually dispatchable. It adds
one native Windows job and one Linux dependency/package-policy job. Normal CI
retains all original 18 compatibility jobs and adds these two focused Desktop
jobs. A manual `CI` dispatch with `desktop_candidate=true` runs only P8's two
jobs and may upload a 14-day internal artifact. The default is false. P7's
`release_candidate=true` routing takes precedence if both inputs are set;
its existing three-job workflow and bundle remain unchanged.

Windows gates include exact Rust/Cargo 1.85, formatting, check, tests, strict
Clippy, release build, executable V1/V2 exact-byte smoke, and native window smoke.
The policy job uses unchanged cargo-deny policy, pinned cargo-audit 0.22.1, and
focused Python package tests. All permissions remain `contents: read`.

`scripts/desktop_candidate.py build --out dist/desktop` builds natively for MSVC,
remaps build paths using existing read-only P7 helpers, assembles the ZIP,
inspects it, extracts it, and executes both packaged smokes. Only native Windows
can produce a certifiable candidate. Local `--allow-dirty` output is a rehearsal;
hosted builds require clean source. The output directory must be new.

The output consists of:

- `datapack-desktop-0.1.0-windows-x86_64.zip`;
- `desktop-manifest.json` with source/toolchain/lock/workflow provenance,
  package size/hash, member list, and smoke results;
- `SHA256SUMS.txt` covering the package and manifest.

The ZIP's exact flat allowlist is `datapack-desktop.exe`, `README.md`,
`LICENSE-MIT`, `THIRD-PARTY-LICENSES.txt`, and `demo.csv`. License text is collected
from the resolved native dependency graph, including build/test dependencies.
PE inspection requires x86_64, PE32+, and the Windows GUI subsystem. Package
inspection rejects unexpected/duplicate/non-regular members, modified demo
bytes, private build paths, and sensitive markers. Verification checks the
exact output set, toolchain/source/lockfile/smoke provenance, manifest
size/hash/member facts, and SHA-256 file. `verify --expected-commit <sha>` also
requires matching clean source. These are
internal integrity/provenance controls, not signatures or reproducible-build
claims. No installer, signing, tag, GitHub Release, registry upload, automatic
update, or public publication is implemented.

A future P7 bundle extension can consume this separately certified package and
receipt. P8 deliberately does not alter P7's mandatory seven-file assembly.

## 7. Local evidence

Local certification completed on 2026-09-14. Recovery of the interrupted work
confirmed only P8 modifications, no staged changes, and no prior P8 commit.
Completed unchanged engine/SDK/security gates were reused. The final native
layout change received a new build, window smoke, formatting, and strict Clippy
check. Only documentation and final review followed executable certification.

| Gate | Local result |
| --- | --- |
| Core Rust 1.85 formatting/check/test/strict Clippy/release | **PASS** — 382 tests, zero failures |
| Desktop Linux formatting/check/test/strict Clippy/release | **PASS** — 28 focused tests; executable V1/V2 exact-byte smoke |
| Desktop native Windows/MSVC 1.85 check/test/strict Clippy/release | **PASS** — 28 tests, including explicitly sparse 16 GiB metadata selection; no large dataset compression |
| Native Windows development executable | **PASS** — V1/V2 adapter lifecycle and exact-byte equality; real window/control launch smoke |
| Final native Windows internal package | **PASS** — remapped MSVC release build, exact five-member ZIP, license inventory, privacy scan, extracted executable lifecycle and window smokes |
| Independent package verification | **PASS** — verifier, exact file set, provenance fields, PE architecture/subsystem, size/hash/member checks, and both `sha256sum --check` entries |
| Windows GNU cross-check/strict Clippy | **PASS** — supplemental evidence, superseded for platform confidence by actual native MSVC execution |
| Binding format/check/test-build/strict Clippy/release | **PASS** — binding has zero Rust unit tests |
| Python identity/grammar/Ruff | **PASS** — 0.1.0, Python 3.9 grammar across 11 Python/stub files, Ruff 0.12.12 check/format |
| Installed current SDK | **PASS** — 15 public SDK tests |
| Fresh Linux ABI3 wheel | **PASS** — maturin 1.14.1/Zig 0.16.0, manylinux2014, exact package inspection, five isolated tests on CPython 3.14.4, installed-wheel P6 CI profile |
| Release tooling tests | **PASS** — 30 total: 21 unchanged P7 tests plus nine Desktop package tests |
| Workflow validation | **PASS** — actionlint 1.7.12 and YAML parsing; original normal-CI jobs retained |
| Dependency policy, all three manifests | **PASS** — unchanged allowlist; no new exception |
| cargo-audit 0.22.1, all three locks | **PASS** — only accepted bincode `RUSTSEC-2025-0141` warning |
| Core package | **PASS** — `cargo package --locked --allow-dirty`, 180 files, verification build; Desktop remains a separate unpublished crate |
| Protected V1/V2 | **PASS** — four compatibility tests and all six frozen sizes/SHA-256 values unchanged |
| P6/P7 preservation | **PASS** — no change to engine, planner, public API, root/Python/fuzz locks, P6 certifier, P7 candidate script/workflow, shared wheel action, or bundle contract |

There are **410 distinct core + Desktop Rust tests** (382 + 28); the same
28 Desktop tests also passed natively on Windows. Tests deliberately avoid a
second copy of the engine's full suite. Desktop's 28 cases cover all A–O
acceptance scenarios, including early/mid/late cancellation, retry, destination
races, invalid-result mapping, metadata-only selection, and non-blocking polling.

The final Windows local rehearsal is retained outside version control:

- Package: `dist/desktop-p8-local/datapack-desktop-0.1.0-windows-x86_64.zip`,
  **676,557 bytes**, SHA-256
  `94a10f62531648fd831c573d6bcbcc496a49ad0b45d192c68c5d26ffc42d9f52`.
- Extracted launch path: `dist/desktop-p8-demo/datapack-desktop.exe`,
  **1,635,328 bytes**, SHA-256
  `061b75f32e5342ae1467546941c450a69e071e8f7c0517f1845d32b3c92ac473`.
- Receipt and checksums: `dist/desktop-p8-local/desktop-manifest.json` and
  `dist/desktop-p8-local/SHA256SUMS.txt`. The receipt honestly records the P7
  checkout SHA plus `source.dirty=true`; this is a local P8 rehearsal, not a
  clean-source hosted candidate. A hosted run must rebuild the technical commit.
- A real Windows-only window capture was produced during shell review at
  `dist/p8-window-fba7bb42-23e0-4954-beba-032668f87057.png`. It shows the rendered
  native controls, not a generated mockup. It predates the small navigation/DPI
  refinement; the final binary subsequently passed the native window smoke.
  This inspection does not close operator visual acceptance.

The current Linux wheel is **991,610 bytes**, SHA-256
`44f064b5d2f249386150aa11b6dafffe21231506fb6220eedb99fce5a55a8312`.
It passed the unchanged installed-distribution/P6 certifier. No real 16 GiB
workload or manual beta-scale performance run was repeated.

Retained logs use the `/tmp/datapack-p8-` prefix outside Git. Principal suffixes:
`core-gates.log`, `binding-gates.log`, `tests-final.log`, `clippy-final.log`,
`windows-tests-build.log`, `windows-final-tests.log`, `windows-final-clippy.log`,
`windows-smoke.log`, `windows-candidate-final.log`, `local-smoke.log`,
`wheel-build-final.log`, `wheel-certify.log`, `sdk-tests.log`,
`script-tests-final.log`, `package-final.log`, and the per-manifest deny/audit
logs. `source-review.json` records the reviewed source inventory.

Resolved environment issues: the first wheel invocation lacked Zig on PATH;
it was rebuilt with the established P7 remapping/build environment and passed
certification. Windows blocked an unsigned script on the WSL UNC path; native
Cargo commands ran directly through PowerShell without changing execution
policy. No engine fix or security-policy exception was needed. Final diff inspection
recognized the demo's intentional CRLF with a file-specific Git whitespace
attribute; the fixture bytes were preserved and the staged diff check passed.

## 8. Acceptance and remaining work

Automation uses only the tiny deterministic CRLF fixture at
`tests/fixtures/desktop/demo.csv` plus small generated inputs. The 16 GiB
selection test is sparse (explicitly marked sparse on Windows), examines only
metadata, and performs no large dataset compression. Tests separate pure
presentation/calculations, Controller transitions/concurrency, and adapter
integration. They cover the A–O acceptance scenarios; engine internals are not
reimplemented in Desktop tests.

Operator review on Windows must still check readable layout at normal/high DPI,
keyboard navigation, file dialogs, progress/cancellation on a meaningful local
file, error recovery, output paths, and the complete demo sequence in
`desktop/README.md`. Automated launch checks cannot judge stakeholder polish.

Deferred: custom dark mode, official icon, installer/file associations, persisted
settings/recent files, advanced resource controls, Linux GUI, macOS/ARM, broad
accessibility certification, and public distribution. No cloud, telemetry,
authentication, licensing, GPU, V3, or Adaptive Compute work is included.

The next sequence is user review → push `productization/foundation` → normal CI
and controlled hosted Windows/Desktop certification → operator visual review →
documentation-only P8 closure → Final Productization Acceptance. No hosted run
is claimed until it actually executes. P8 remains open until that closure.
