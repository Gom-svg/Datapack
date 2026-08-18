# Phase 16 representative performance observation

## Evidence classification

- Status: **OBSERVATIONAL**, with **CERTIFIED CORRECTNESS GATES** inside the run
- Timestamp: `2026-08-17T23:46:55Z`
- DataPack commit: `5b174c397042eaf8cba2b8116370bb903cf3067f`
- Git state during run: clean
- Harness report SHA-256:
  `15c5936f018a296d3a52e09a65c7dcd48d7616021d57b9f1712036da2073b4ea`
- Harness report location during certification:
  `/tmp/datapack-phase16-representative.json` (not tracked)

The run used the optimized Cargo bench profile and three runs per scenario. It
contained no warm-up, cache eviction, CPU pinning, thermal control, or
system-load isolation. Timing values are observations from this invocation,
not regression thresholds, release guarantees, or evidence of universal
performance.

## Power-recovery re-certification

After an unexpected workstation shutdown, the committed harness and this
untracked report were recovered without rewriting either one. The original
JSON report referenced above was no longer present under `/tmp`, so its hash
cannot be independently recomputed from the recovered filesystem.

The representative suite was therefore run again in the same optimized bench
profile, with three runs per scenario and generated artifacts on WSL's
Linux-native `/tmp` storage:

- Timestamp: `2026-08-18T03:04:08Z`
- DataPack commit: `5b174c397042eaf8cba2b8116370bb903cf3067f`
- Git state during re-certification: dirty only because this recovered report
  was still untracked; the implementation at the recorded commit was unchanged
- Re-certification JSON SHA-256:
  `c7849eea5460ef5e6ca4c91e1bbf706e6f4195c5d50cfdad56088c2467476b88`
- Re-certification JSON location:
  `/tmp/datapack-phase16-recovery-representative.json` (not tracked)

All source sizes and SHA-256 values, selected versions and modes, archive sizes
and SHA-256 values, and the v2 12-chunk structure reproduced exactly. Every
archive was stable across all three runs, every restored file was byte-exact,
and every required validation check passed. Fresh timing samples differed, as
expected for observational wall-clock measurements, and are retained only in
the re-certification JSON rather than substituted into the original table.

## Environment

| Fact | Recorded value |
| --- | --- |
| Rust | `rustc 1.85.0 (4d91de4e4 2025-02-17)` |
| Cargo | `cargo 1.85.0 (d73d2caf9 2024-12-31)` |
| Build | optimized Cargo `bench` profile |
| OS/kernel | Linux `6.18.33.2-microsoft-standard-WSL2` |
| WSL | yes, Ubuntu |
| Architecture | `x86_64` |
| CPU | AMD Ryzen 7 5700 |
| Logical CPUs visible | 16 |
| Memory visible | 8,256,122,880 bytes total |
| Work/input/output path class | WSL Linux-native filesystem under `/tmp` |
| Report path class | WSL Linux-native filesystem under `/tmp` |
| GPU inventory | AMD Radeon RX 6800 XT, 16 GB VRAM |
| GPU used by DataPack | no |

This checkout resides under `/mnt/c`, but measured source, archive, and restored
files were created under `/tmp`. These observations therefore must not be
classified as WSL `/mnt/c`/NTFS file-to-file results. Conversely, they must not
be silently pooled with native Linux or native Windows observations.

## Results

Compression and decompression columns are medians of three complete Rust
Application API file-to-file calls. MiB/s uses 1,048,576 bytes. Ratios are
source bytes divided by complete archive bytes.

| Scenario | Source bytes | Archive | Archive bytes | Ratio | Chunks | Compress ms / MiB/s | Decompress ms / MiB/s |
| --- | ---:| --- | ---:| ---:| ---:| ---:| ---:|
| repetitive structured | 2,721,159 | v1 structured | 138,179 | 19.6930x | n/a | 103.137 / 25.162 | 8.298 / 312.721 |
| realistic structured | 3,868,893 | v1 structured | 445,816 | 8.6782x | n/a | 114.084 / 32.342 | 8.155 / 452.451 |
| high-cardinality | 4,511,021 | v1 RawZstd | 2,264,590 | 1.9920x | n/a | 45.031 / 95.536 | 6.487 / 663.162 |
| random/low-redundancy | 3,422,884 | v1 RawZstd | 2,600,636 | 1.3162x | n/a | 37.160 / 87.844 | 6.872 / 475.050 |
| realistic v2 multi-chunk | 11,609,824 | v2 chunked RawZstd | 2,233,926 | 5.1970x | 12 | 17.715 / 624.993 | 32.990 / 335.616 |

The v2 scenario used 1 MiB chunks, four CPU workers, eight maximum in-flight
chunks, the `chunked-raw-zstd` backend, and adaptive level disabled. The GPU
did not contribute.

## Dataset identities

All inputs came from the deterministic repository generator.

| Scenario | Rows | Seed | Source SHA-256 |
| --- | ---:| ---:| --- |
| repetitive structured | 50,000 | 42 | `b56f8e42c11d3210e74d5b7cfd122a1c60858fa871fca504d7239b0048f2ee42` |
| realistic structured | 25,000 | 2,026 | `d60738c81711d84c3cabf6958cd20fd00112b130fdb708e70a0a65bd3a02812c` |
| high-cardinality | 15,000 | 7 | `7ce3503d4b740783534c3454f49b3a3484f36674aec5f66b8c2bff3cdb841048` |
| random/low-redundancy | 25,000 | 99 | `163edffe7d2ad538704b11b0ee91c3c9e9b4c1825d97a04c1438bb512f4e4e9a` |
| realistic v2 multi-chunk | 75,000 | 20,260,316 | `0cf579c11cbf21c0bc6970d9daa3ff3cef109d0fe519ffa9a643c8ac22a77fa0` |

No generated input, archive, or restoration is stored in the repository.

## Correctness evidence

All five scenarios passed:

- expected archive version and intentionally fixed selected mode;
- stable complete archive size and SHA-256 across all three same-build runs;
- restored source length and SHA-256;
- streaming byte-for-byte comparison;
- archive validation against the original source;
- header, metadata, payload-structure, decompression, and restored-length
  checks; and
- format-specific integrity checks.

The v2 scenario additionally passed its 12-entry chunk-table structure,
per-chunk SHA-256, global SHA-256, and trailing-data checks. V1 accurately
reported its unavailable stored-hash and trailing-data capabilities rather
than fabricating them.

This correctness evidence is eligible for deterministic regression gating.
The observed milliseconds and MiB/s are not.

## Interpretation and limitations

The observed ratios illustrate the expected workload shape, not a universal
ranking: structured repetitive data is DataPack's strongest niche, while
high-cardinality and random data reduce the advantage and select the honest
RawZstd fallback. V1 structured and v2 chunked RawZstd optimize different
properties; their rows are not a controlled head-to-head format comparison.

The relatively small inputs can amplify setup, cache, scheduler, and timing
resolution effects. Later runs may have benefited from the page cache. The
run does not establish performance on `/mnt/c`, NTFS, OneDrive, native Linux,
native Windows, another CPU, another zstd build, or production-size data.

External Python Beta Test 001 is not merged into this report. Its 1.59 GiB v2
round trip is strong separate functional/integrity evidence, but its dev wheel,
WSL `/mnt/c`, NTFS, and OneDrive conditions make its timings unsuitable for
comparison with this optimized `/tmp` observation.
