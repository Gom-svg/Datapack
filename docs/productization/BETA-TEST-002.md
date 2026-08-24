# DataPack Productization Beta Test 002

Test: Progress + Cancellation + Transactional Safety

Result: **BETA TEST 002 — PASS**

Evidence classification: manual external productization validation. All
wall-clock measurements are **OBSERVATIONAL**.

## Purpose

This test exercised the installed DataPack Python distribution as an external
user would use it. It validated structured progress, explicit cooperative
cancellation, typed Python cancellation, transactional output safety, retry to
successful completion, archive validation, and exact restoration using only a
hosted Linux ABI3 wheel installed outside the repository.

This test did not begin P5 and does not claim production readiness, controlled
benchmark performance, an external security audit, or registry publication.

## Artifact and isolation

| Fact | Value |
| --- | --- |
| Hosted CI artifact source | Run `32710691878` |
| Distribution | `datapack-engine` 0.1.0 |
| Import package | `datapack` 0.1.0 |
| Installation | Isolated virtual environment outside the DataPack repository |
| Python | CPython 3.14.4 |
| Platform | WSL2 Linux x86_64 |

The successful import and execution came from the installed hosted wheel, not
from repository-relative Python files.

## Dataset identity

| Fact | Value |
| --- | --- |
| Format | CSV |
| Generation | Deterministic; performed outside the repository |
| Rows | 2,049,597 |
| Bytes | 134,219,306 |
| SHA-256 | `1c1e6ae1be35f8c6ae2f8548f0d1648715964fb5cf4cacd185edb4a25efe383c` |

The dataset is external validation input. It was not added to the repository
and is not a protected compatibility fixture.

## Methodology

1. Install the Linux ABI3 wheel from hosted run `32710691878` into an isolated
   virtual environment outside the repository.
2. Analyze the external CSV while observing structured progress events.
3. Begin compression, wait until real progress has been observed, explicitly
   request cancellation, and verify the typed cancellation outcome and
   transactional cleanup.
4. Retry compression without cancellation and allow it to complete.
5. Validate the completed archive against the original source.
6. Fully decompress the archive, compare the restored SHA-256 with the source,
   and perform an independent byte-for-byte comparison.
7. Begin another decompression with an existing destination, request
   cancellation from another Python thread, and verify destination preservation,
   partial cleanup, and progress terminal-state behavior.

## Results

### Installation and analysis

| Check | Result |
| --- | --- |
| Hosted wheel isolated installation | PASS |
| Analyze with structured progress | PASS |

### Compression cancellation

| Check | Result |
| --- | --- |
| Cancellation requested after real progress | PASS |
| Typed Python `CancelledError` | PASS |
| No final archive published | PASS |
| No retained `.partial` | PASS |
| No false terminal successful progress | PASS |

### Completed compression and validation

| Fact | Result |
| --- | --- |
| Retry/full compression | PASS |
| Selected mode | `raw_zstd` |
| Original bytes | 134,219,306 |
| Archive bytes | 55,615,065 |
| Compression ratio | 2.4134x |
| Reduction | 58.56% |
| Observed compression time | approximately 1.041 seconds — **OBSERVATIONAL** |
| Archive validation against original | PASS |
| Validation result | `valid=true`; `against=matched` |

### Restoration and decompression cancellation

| Check | Result |
| --- | --- |
| Full decompression | PASS |
| Restored SHA-256 equals original SHA-256 | PASS |
| Independent byte-for-byte comparison | PASS |
| Cross-thread Python decompression cancellation | PASS |
| Existing destination preserved during cancellation | PASS |
| No retained `.partial` after cancellation | PASS |
| No false terminal successful progress | PASS |

The restored SHA-256 was
`1c1e6ae1be35f8c6ae2f8548f0d1648715964fb5cf4cacd185edb4a25efe383c`,
exactly matching the original dataset.

## Evidence and limitations

- The approximately 1.041-second compression measurement is
  **OBSERVATIONAL**. The environment was not controlled for benchmarking, and
  the value is not a CI threshold or performance guarantee.
- The 2.4134x ratio and 58.56% reduction describe only this deterministic
  dataset and the selected `raw_zstd` mode. They are not universal compression
  claims.
- This was one manual WSL2 Linux x86_64 run on CPython 3.14.4. Broader
  Linux/Windows and CPython compatibility remains supported by the separate
  hosted distribution matrices.
- The external dataset is not a repository fixture and does not modify the
  protected v1/v2 compatibility corpus.
- Passing this test does not establish production readiness, an external
  security audit, or public package availability.
- Python `KeyboardInterrupt`, CLI Ctrl+C signal integration, and Desktop/Tauri
  remain **DEFERRED**. P5 had not started at the time of this test; later phase
  status is tracked in the Productization report.

## Conclusion

**BETA TEST 002 — PASS.** The externally installed hosted wheel demonstrated
structured progress, explicit cancellation, typed cancellation reporting,
transactional output safety, successful retry, archive validation, exact
restoration, and cross-thread Python cancellation on the recorded workload and
environment. Timing remains **OBSERVATIONAL**. P5 had not started when this
external test was executed.
