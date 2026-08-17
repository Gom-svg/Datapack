# Performance regression methodology

DataPack's Phase 16 suite separates deterministic correctness regression from
observational performance. A correctness failure exits unsuccessfully. A slow
measurement does not.

## Run the suite

Use the optimized Cargo bench profile:

```bash
source ~/.bashrc
export CARGO_TARGET_DIR="$HOME/.cache/datapack-modernization"

cargo bench --locked --bench performance_regression -- \
  --preset smoke --runs 2 \
  --output /tmp/datapack-performance-smoke.json
```

For a larger but still repository-controlled observation:

```bash
cargo bench --locked --bench performance_regression -- \
  --preset representative --runs 3 \
  --output /tmp/datapack-performance-representative.json
```

Use `--work-dir PARENT` to choose the filesystem containing generated sources,
archives, and restored files. The harness creates and removes an isolated
child directory. `--output` is optional; the complete report is always printed
to standard output.

## Evidence that can fail

The harness fails before emitting a successful report if any of these facts
are false:

- the recorded smoke source size or SHA-256 changed;
- the archive version or intentionally fixed selected mode changed;
- repeated same-build runs emitted different archive sizes or SHA-256 values;
- decompression did not restore the exact source length, SHA-256, and bytes;
- validation did not match the original source;
- a required format-specific structural check did not have its expected
  status; or
- v2 did not contain the expected number of multiple chunks.

No elapsed duration or throughput participates in those decisions.

## Observational fields

For compression and decompression, the JSON records every elapsed sample, the
median, and median MiB/s. It also records archive size and source-to-archive
ratio. These facts apply only to the recorded dataset, DataPack/zstd build,
configuration, machine, filesystem placement, cache state, and concurrent
load.

The timing boundary is the complete synchronous Rust Application API
file-to-file call. Hashing, validation, and byte comparison run after the timed
operation. No `fsync` or cache eviction is performed.

## Environment interpretation

The report identifies the Git commit and dirty state, Rust/Cargo versions,
optimized build state, OS/kernel/architecture, CPU/logical CPUs, available
Linux memory facts, WSL state, and relevant path classes.

At minimum, treat these placements as separate populations:

- WSL plus `/mnt/c` or another Windows-drive mount;
- WSL plus a Linux-native path such as `/tmp` or the distribution filesystem;
- native Linux; and
- native Windows.

Do not merge them into a single baseline. A synced directory, antivirus or
indexing activity, storage device, network mount, CPU power state, thermal
state, or unrelated system load can also change results even inside one path
class.

Optional `DATAPACK_PERF_GPU_INFO` text is inventory only. DataPack currently
uses the CPU, and the report records `gpu_used_by_datapack: false`. The local
developer workstation's AMD Radeon RX 6800 XT does not contribute to current
compression or decompression measurements.

## Comparison policy

A useful comparison requires the same:

- dataset bytes and SHA-256;
- scenario seed, rows, archive version, backend, and chunk controls;
- DataPack commit and dependency lockfile;
- Rust toolchain and optimized build profile;
- operating-system/virtualization class;
- input and temporary-output filesystem class;
- run count, warm-up policy, and cache policy; and
- materially similar load controls.

Even when these match, a small run count is an observation rather than a
universal claim. The suite deliberately contains no checked-in “must be faster
than” threshold.

## Relationship to other tools

- `datapack compare` remains the defined v1-versus-standalone-zstd factual
  comparison.
- `datapack benchmark` remains its configurable legacy workflow.
- `datapack tune` remains an experimental v2 configuration grid.
- `performance_regression` exercises representative Application API flows,
  enforces stable correctness, and adds environment-complete observations.

These interfaces are complementary and are not normalized into one hidden
methodology.

## Result retention

Write reports outside the repository by default. Retain a result only with its
environment facts and an explicit evidence label. Never commit generated
source files, `.dpack` artifacts, restored outputs, or large external data.

See [RFC-006](../rfcs/RFC-006-performance-regression-suite.md) for the complete
architecture and scenario catalog.
