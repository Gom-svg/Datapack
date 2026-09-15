# P8 — Real-World Hardening

Development version: **0.1.0**. Desktop Foundation checkpoint:
`6380351ecfbaebc8e593877f8682dce6256588fd`.

This stage exercises an installed **public Python SDK** on a real file and leaves
a Windows Desktop operator procedure for that same workload. It does not redesign
Desktop, close P8, or begin Final Productization Acceptance.

Status: **REAL-WORLD SDK HARDENING COMPLETE — LOCALLY CERTIFIED**. The real run
finished on 2026-09-14; its retained result was recovered and checked on
2026-09-15. Large-file Desktop operator execution remains **PENDING**.

## Starting evidence and scope

Recovery found clean `productization/foundation`, matching the expected checkpoint
and origin, with empty working/staged diffs. Desktop Foundation's hosted evidence
was checked against that exact commit:

- [Normal CI 34893776372](https://github.com/Gom-svg/Datapack/actions/runs/34893776372): **SUCCESS**.
- [Controlled Windows Desktop certification 34894959730](https://github.com/Gom-svg/Datapack/actions/runs/34894959730): **SUCCESS**, including the Windows x86_64 and dependency/package-policy jobs.

The owner additionally confirmed independent candidate download/verification,
manifest/checksums/provenance, and a manual Desktop `demo.csv` lifecycle through
Analyze, Compress, Validate against original, and Decompress, with independent
SHA-256 and byte equality. This is owner-reported small-fixture functional
acceptance, distinct from the large-file procedure and final visual acceptance.

P6's large-file result is historical **engine/CLI** evidence. The run in this
document is a separate P8 **installed Python SDK consumer** acceptance run. It is
not a rerun of the P6 certification suite, a GUI acceptance result, or a benchmark.
The Desktop operator procedure remains to be executed and recorded on Windows.

No Rust product code, UI, engine/planner behavior, public API, V1/V2 format,
dependency, lockfile, Rust 1.85 baseline, ABI3 policy, or `VERSION` changed.
P6's certifier and P7's release workflows, scripts, and artifact contracts remain
frozen. Normal CI gains one tiny-fixture harness-test step after SDK installation;
the real dataset is never an automated CI input.

## Acceptance consumer

`scripts/real_world_acceptance.py` uses the standard library and the installed
`datapack-engine` distribution (`import datapack`). It does not import binding
internals, call a codec, invoke the DataPack CLI, upload anything, or log rows.
It requires Python 3.9 or newer and an already built/installed compatible wheel.
Use the existing internal wheel or build it through the documented Python
distribution path; no package publication is needed.

The public calls, in order, are:

1. `datapack.analyze(source, progress=...)` with the existing bounded sample default.
2. Optional `datapack.compress(..., cancellation=CancellationToken())` cancellation probe.
3. `datapack.compress(..., options=V2CompressionOptions(...), progress=...)`.
4. `datapack.validate(archive, against=source, progress=...)`.
5. `datapack.decompress(archive, restored, verify=True, progress=...)`.
6. Independent Python size, streaming SHA-256, and streaming byte comparison.

The full profile explicitly selects V2 `chunked_raw_zstd`, 64 MiB chunks, four
threads, four in-flight chunks, `adaptive_level=False`, and 256 MiB admission.
It never translates a sampled Structured recommendation into full V1 execution.
The admission bound is configured in-flight chunk bytes, **not process RSS**.
Validation/restoration use the public 512 MiB memory limit, exact source-size
output ceiling, and expected chunk-count ceiling.

## Run and storage controls

From the checkout, using a Python environment with the installed SDK:

```bash
python scripts/real_world_acceptance.py \
  --input /external/data/source.csv \
  --work-dir /external/data/p8-evidence-new \
  --profile full --cancel-retry --keep-output
```

On Windows PowerShell, source and work paths can be supplied interactively:

```powershell
$source = Read-Host 'Existing source CSV path'
$work = Read-Host 'New evidence directory outside the checkout'
python scripts/real_world_acceptance.py --input $source --work-dir $work --profile full --cancel-retry --keep-output
```

`--expected-sha256 <64 hexadecimal characters>` optionally checks a known source
identity before output creation. `--profile preflight` performs identity, space,
version, and streaming hash checks only; its report explicitly says that full
acceptance was not completed. Use a **different new directory** for a later full
run. `--cancel-retry` requires the full profile.

The work directory must not exist, even as a dangling symlink, and its parent
must already exist. Work inside the Git checkout is rejected. The input must be
a nonempty regular file. Metadata, capacity, SDK identity, and streaming source
SHA-256 are checked before creating the evidence directory. Insufficient space,
an existing directory, an unexpected source hash, or a missing SDK exits with
code 2 without modifying existing files. The preflight diagnostic is on the
console because no safe new report directory was established.

Capacity is checked on the actual work volume. For source size `S` and
`N = ceil(S / 64 MiB)`, the conservative starting budget is:

```text
archive allowance = S + ceil(S / 100) + N * 128 + 1 MiB
required free     = archive allowance + S restored bytes + 8 GiB reserve
source copies     = 0
```

The archive allowance covers a near-incompressible result, rather than assuming
historical savings. A fresh transactional partial becomes the committed output
by rename; it is not a second retained full-size copy. Space is checked again
after hashing, before compression/cancellation, and before restoration. WSL's
virtual-disk free space must not be mistaken for physical Windows SSD capacity;
use a work directory on the intended Windows volume for this machine.

Only `archive.dpack`, `restored.csv`, and `real-world-acceptance.json` are owned
by a successful run. Every engine write uses `overwrite=False` and
`keep_partial=False`; restoration can never target the source. The script never
copies the source. Source identity/size/mtime are checked between operations;
keep both inputs and the isolated work directory unchanged during execution.

Outputs and failure evidence are retained by default. `--keep-output` makes
that explicit. Only an explicit `--cleanup-output` deletes successful archive
and restored outputs; the report and original remain. Failed-run evidence is
retained even with that option. The tool never deletes pre-existing directories
or performs broad cleanup. Reserve checks are admission checks, not a disk quota;
other applications can still consume space after a check.

## Progress, cancellation, and reporting

The SDK delivers synchronous public `ProgressEvent` snapshots. The observer
aggregates event counts, phase maxima, last totals, a terminal snapshot, and a
bounded list of violations. It checks counter monotonicity **within each phase**,
plausible byte/item bounds, valid optional percentages, exactly one terminal
success, and no callbacks after that terminal. Phases may reset counters and
unknown totals remain indeterminate. For write operations, the final destination
must already exist when terminal success is observed. A successful return, exact
reported file sizes, and later integrity checks provide additional commit evidence.
There is no event list proportional to file size, row preview, or timer progress.

The cancellation trigger is the first Compressing/Advanced event at or beyond
`min(S, max(64 MiB, ceil(S / 20)))`. The observer requests cooperative cancellation
on the public token. The acceptance probe requires stable `cancelled` /
`cancellation` identity, zero terminal success events, an absent final archive,
and no new partial or unexpected files. It then retries with a fresh token and
requires a fully committed successful result. A post-commit successful return is
never relabeled as cancellation; it would fail this deliberately pre-commit
probe. No worker or process is force-terminated. Existing-destination refusal is
tested with a sentinel in tiny tests; the real probe uses a fresh destination.

The JSON schema version is 1. It records runtime/distribution/platform identity,
harness SHA-256, input basename/size/SHA, preflight budget, sampled analysis and
planner reason/diagnostics, explicit compression configuration and observed
result, compact progress, cancellation/retry, validation checks/source match,
verified decompression, independent verification, observational durations, and
retained artifact sizes. Absolute source/work paths are excluded and error
context redacts them. No raw rows, user data fixture, or raw event log is stored.

Each operation advances the report stage before running. SDK/acceptance failures
produce `status=failed`, `acceptance_completed=false`, stable error code/category
where supplied, and exit code 1. Validation's `valid=False` is a failure even if
the SDK returned normally. Progress contract failures, an unverified restoration,
or any independent mismatch also fail. Abrupt process/power loss may leave the
last `running` checkpoint; that is incomplete evidence, never a pass.

Independent verification uses 4 MiB reads per stream, compares every block, and
hashes both files through EOF. It requires equal size, equal final SHA-256, equal
bytes, and an unchanged source SHA relative to preflight. This is separate Python
code, not a second call to DataPack's verifier. All full-run timings include
observer and verification overhead and depend on storage, filesystem, caches,
hardware, and platform. They are **OBSERVATIONAL** only.

## Local execution evidence

The new **full profile with cancellation/retry passed** under WSL2, CPython
3.14.4, installed `datapack-engine` 0.1.0, using the existing certified Linux
ABI3 wheel. Source and work artifacts were on the Windows volume; no source
copy was made. The run finished at **2026-09-14 22:49:28 UTC**. The next session
found no running worker, a completed passing JSON report, and both expected
artifacts with matching metadata. Nothing was restarted or deleted.

The compact, path-free receipt is preserved verbatim at
[`evidence/p8-real-world-acceptance.json`](evidence/p8-real-world-acceptance.json),
**11,106 bytes**, SHA-256
`aa55bd68a6bf12fd743f3b6143f3debb237aebea5e9b96a4a818d236506053d7`.
It contains metadata and aggregate progress only, not dataset rows or raw events.
The local report, archive, and restoration remain in the external directory
`datapack-p8-real-world-20260914`; no large artifact is committed.

| New SDK run fact | Observed result |
| --- | --- |
| Input | `HI-Large_Trans.csv`, **17,052,760,651 bytes** |
| Format / delimiter | CSV / comma |
| Sampling | **Partial**, 10,000 records, 979,049 bytes; record limit reached |
| Recommendation | Structured (`csv_columnar_dictionary`); `HIGH_REPETITION_DETECTED`, 9/11 columns |
| Sample-derived estimates | **13.583547%** savings; **3420.8477 MiB** dictionary memory |
| Analysis diagnostics | Sampling record limit; duplicate column name at index 4; censored cardinality tracking at indices 5 and 7 |
| Executed compression | **V2 chunked RawZstd**, 64 MiB / four threads / four in-flight / 256 MiB admission; no full Structured execution |
| Archive | **3,541,994,339 bytes**, **255 chunks** |
| Observed ratio / reduction | **4.814451695542362× / 79.22920275789895%** |
| Validation against original | **PASS**, valid, source **matched**, V2, 255 chunks, no diagnostics |
| Validation checks | All nine passed: header, metadata, chunk table, payload structure, decompression, restored length, per-chunk SHA-256, global SHA-256, trailing data |
| Restoration | **17,052,760,651 bytes**, verified V2 restoration, fresh destination, no diagnostics |
| Independent size / SHA-256 / byte comparison | **PASS / PASS / PASS**, including unchanged preflight source hash |

Source preflight SHA-256, independently re-read source SHA-256, and restored
SHA-256 all equal:

```text
d13635e297c64673826217631fb88d635c4f506052e1bde833895eda2a65c3f2
```

| Operation | Progress events | Terminal successes | Contract violations |
| --- | ---: | ---: | ---: |
| Analyze | 4 | 1 | 0 |
| Cancelled compression probe | 14 | 0 | 0 |
| Full compression / retry | 259 | 1 | 0 |
| Validate against source | 4 | 1 | 0 |
| Decompress | 259 | 1 | 0 |

Compression and decompression each reported 255 advanced chunk events, ending
at 17,052,760,651 bytes / 255 chunks, with output present at terminal success.
No event followed terminal completion. Analyze/Validate offer coarser phase
events; their counts are not represented as fine-grained progress.

The cancellation trigger was **852,638,033 bytes** (5% rounded up). Cancellation
was actually requested at the next reported chunk, **872,415,232 bytes / 13
chunks**. The public error was `cancelled` / `cancellation`; no terminal success,
final archive, or leftover partial was observed. A fresh-token retry then
completed successfully. The source remained intact.

| Observational duration | Seconds |
| --- | ---: |
| Initial streaming source SHA-256 | 106.833 |
| Analyze | 0.028 |
| Cancellation probe through safe return | 7.293 |
| Full compression | 174.762 |
| Validate against original | 226.027 |
| Decompress | 329.528 |
| Independent streaming SHA-256 and byte comparison | 235.291 |

These are measured SDK-consumer durations, not performance gates or comparative
benchmarks. No controlled peak-RSS measurement was collected. Configured
admission and bounded Python read buffers are not claims about total process RSS.
Matching the historical archive size does not merge P6 and P8 evidence or impose
a new determinism guarantee.

The run admitted **42,867,064,717 required free bytes**, including an
8,589,934,592-byte reserve, against **59,813,765,120 available bytes**. Before
restoration, 55,098,384,384 bytes were available against 25,642,695,243 required.
Retained archive plus restoration total **20,594,754,990 bytes** (about 19.18 GiB),
plus the small report. Completion free space was **38,219,882,496 bytes**; recovery
observed **38,430,109,696 bytes**. Free space can also change due to other programs.
No cleanup was requested or performed. A second full large-file acceptance run
would fail this conservative admission budget on that volume until the operator
explicitly arranges more capacity or cleanup; the completed evidence is reused.

Retained diagnostic logs outside Git use the `datapack-p8-real-` prefix:
`run.log`, `harness-tests.log`, `sdk-tests.log`, and `windows-tests.log`.
The earlier read-only preflight and executed-script copy are also retained.

## Windows Desktop operator procedure

Use the same external `HI-Large_Trans.csv` and a fresh work directory on a volume
with sufficient space. Retain the original. Avoid keeping an unnecessary second
archive/restoration beside SDK evidence: arrange capacity before starting, and
only delete prior artifacts after an explicit operator cleanup decision.

1. Launch the internal Windows candidate, or follow the build/launch instructions
   in [Desktop help](../../desktop/README.md). Record commit/artifact identity,
   Windows version, available storage, and display scaling.
2. Select the source. Confirm its basename and **17,052,760,651 bytes**. Selection
   should remain responsive and should not load the whole file.
3. Analyze. Record format/delimiter, bytes and records inspected, sampling
   completeness, recommendation, reason, dictionary estimate, and diagnostics.
   Confirm the UI makes clear that the recommendation is sample-derived.
4. Explicitly choose **Chunked**, even if Structured is recommended. The current
   Desktop Foundation's fixed configuration is **8 MiB chunks, two threads, two
   in-flight chunks, 16 MiB admission**. It is bounded V2, but differs from this
   harness's 64/4/4/256 MiB configuration. Do not claim identical archive size or
   performance, and do not select Automatic for this large acceptance workload.
5. Choose a fresh archive destination and compress. Observe actual phase/byte
   progress; move/resize the window and inspect controls to assess responsiveness.
   Record storage reduction, mode, archive size/path, and any responsiveness issue.
6. Optionally cancel once progress shows real work. Record Cancelling, the final
   outcome, destination/partial behavior, and recovery of controls. Retry with a
   fresh destination as needed. A completion that already committed wins over a
   late cancellation request. Do not kill the process.
7. Open **Validate or restore result**, supply the original, and validate. Require
   Valid, source agreement, V2, expected length/chunks, and passing integrity
   checks. Record the details; invalidity must never be accepted as success.
8. Restore to a new filename. Observe progress/responsiveness, completion path,
   restored size, and the actual integrity result.
9. Independently compare the Desktop restoration. This bounded Python check can
   reuse the consumer's independent verification routine from the checkout:

   ```python
   import json
   import sys
   from pathlib import Path
   sys.path.insert(0, "scripts")
   from real_world_acceptance import compare_streams
   source = Path(input("Original path: "))
   restored = Path(input("Desktop restored path: "))
   result = compare_streams(source, restored)
   print(json.dumps(result, indent=2))
   assert result["size_match"] and result["sha256_match"] and result["byte_for_byte_match"]
   assert result["source_sha256"] == "d13635e297c64673826217631fb88d635c4f506052e1bde833895eda2a65c3f2"
   ```

10. Keep a small observation record: operator/date, commit/artifact, source
    basename/hash, mode/configuration, free-space checks, progress/responsiveness,
    cancellation/retry outcome if attempted, result sizes, validation, independent
    equality, and issues. Screenshots should avoid unnecessary private paths.
    Automated SDK evidence cannot certify the GUI interaction or visual quality.

## Certification and remaining stages

Focused local checks use only tiny generated fixtures: arguments, capacity
boundary, overwrite/symlink refusal, streaming hash and byte comparison,
mid-read mutation detection, compact report serialization/redaction, phase
progress and terminal ordering, bounded aggregation, cancellation interpretation,
real SDK cancellation/retry/roundtrip, explicit cleanup, expected-hash refusal,
preflight-only semantics, and failure evidence. No real dataset enters CI.

| Impacted check | Local result |
| --- | --- |
| Harness tests, installed Linux SDK / CPython 3.14.4 | **PASS — 25 tests**, including eight SDK integration cases |
| Existing public SDK suite | **PASS — 15 tests**, including V1/V2, stable errors, progress and cancellation |
| Native Windows / CPython 3.12 helper checks | **PASS — 16 tests**; eight SDK cases skipped because no SDK was installed in that interpreter, one symlink test skipped because unprivileged creation was unavailable |
| Python 3.9 grammar | **PASS**, both added Python files parsed with `feature_version=(3, 9)` |
| Ruff 0.12.12 | **PASS**, check and format check for both added files |
| Identity/version consistency | **PASS — 0.1.0** |
| Workflow syntax | **PASS**, actionlint 1.7.12 on the additive normal-CI step |
| Whitespace/diff review | **PASS**, `git diff --check`; only hardening tooling, tests, CI step and documentation |

The real run's receipt preserves the executed harness SHA-256
`29f28e3a8f0b1714b4d99b1be7c6977c69fb1ba1859baaae3b69b4aa12fb9e10`.
While it ran, a Windows-only **error presentation** defect was found: `OSError`
may escape filename backslashes. Final `error_record` also redacts that spelling;
a regression test passed on Linux and Windows. The executed script is retained
locally and its hash matches the receipt. That error-only change did not alter
any executed successful-run operation or verification; the expensive workload
was not repeated. All final harness tests include the correction.

No full historical Rust/security/package rebuild was repeated because Rust
product code and dependency/release inputs are unchanged. No shared Desktop
logic changed, so Desktop's previously certified Rust tests were reused rather
than repeated. Foundation's 382 core and 28 Desktop tests remain the prior Rust
evidence; this stage adds Python consumer tests, not new Rust tests.
The next product task is **2026 Desktop visual redesign concept and
implementation**. Operator visual acceptance and formal P8 closure remain pending;
Final Productization Acceptance remains after that closure.

| P8 stage | Status after hardening |
| --- | --- |
| Desktop Foundation | **CERTIFIED**, local and hosted; owner confirms small-fixture manual functional pass |
| Public Python SDK real-world hardening | **COMPLETE — LOCALLY CERTIFIED** |
| Desktop 16 GiB operator procedure | **EVIDENCE-READY**, execution pending |
| 2026 Desktop visual redesign | **PENDING**, separate approved concept/spec and implementation |
| Final visual/operator acceptance | **PENDING** |
| Formal P8 closure | **PENDING** |

Hosted execution of the newly added tiny-fixture CI step is **PENDING** until an
authorized future push. No push, publication, redesign, or closure is part of
this hardening commit.
