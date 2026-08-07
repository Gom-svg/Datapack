# DataPack Comparison Methodology

This document defines what `datapack compare` measures. It enables a result to
be interpreted and repeated; it does not claim that either competitor is
generally faster or smaller.

## Competitors

The Phase 10 comparison has exactly two competitors:

1. **DataPack** — the current default `.dpack` v1 `PlannerPolicyV1` and
   execution-plan path, including its structured-safety gate and RawZstd
   fallback. Artifact size includes the complete DataPack container.
2. **Standalone zstd** — an ordinary zstd frame produced at compression level
   3, without DataPack framing.

Compare does not retune either competitor per input. It does not add codecs or
activate new column transforms. A DataPack RawZstd result remains a valid
comparison: its container overhead and restoration behavior are part of the
measured artifact.

## Input scope

Quick mode is the default. It prepares an immutable snapshot prefix of at most
64 MiB (67,108,864 bytes), or the positive limit supplied with
`--max-input-mb`. Snapshot creation is outside all timed regions. Both
competitors receive the same prepared file.

Quick is always reported as partial. When the complete source fits within the
bound, `scope.prefix_limited` is false but the conservative Quick scope remains
partial. When the source is larger, the prefix stops exactly at the byte bound
and `scope.prefix_limited` is true. Compare does not normalize newline
boundaries, records, UTF-8, delimiters, or field values when making the prefix.

Full mode snapshots every source byte observed during preparation and rejects
`--max-input-mb`. It is reported as full only after both complete restored byte
sequences have passed length and SHA-256 identity checks against that immutable
snapshot.

The measured input byte count, not the complete source size, is the numerator
for Quick artifact ratios and Quick throughput.

## Preparation and planning

Preparation consists of input preflight and creation of one immutable
compared-byte snapshot in both modes. Quick snapshots only its bounded raw-byte
prefix; Full snapshots the complete source. DataPack planning then runs once
against that snapshot using the current compatibility policy and resource
limits. Its shared analyzer has a 64 MiB planning sample budget plus narrower
record, column, cardinality, and memory bounds. It can therefore derive a safe
plan from a bounded sample without claiming to inspect every snapshot byte.
The plan is reused for all measured DataPack runs so timing samples do not mix
planner variability with codec/file execution variability.

Preparation time is excluded. DataPack planning time is excluded from the
compression duration but reported separately. Standalone zstd has no
corresponding planner stage. This exclusion means the `fastest_compression`
winner is specifically a measured execution-stage result, not an end-to-end
elapsed time from command invocation.

Snapshot hashing, per-run artifact stability hashing, every per-run
restored-output hash, and final report rendering are also outside timed
regions.

The compare-specific structured encoder buffers no more than 64 MiB of input.
If a snapshot with a structured plan exceeds that fixed ceiling, DataPack uses
the streaming RawZstd path and the report includes
`STRUCTURED_COMPARE_MEMORY_LIMIT`. Structured-analysis unavailability or
safety limits and a later encoder refusal likewise use RawZstd and are reported
as `STRUCTURED_ANALYSIS_UNAVAILABLE`, `STRUCTURED_ANALYSIS_LIMITED`, or
`STRUCTURED_ENCODER_FALLBACK`. These codes describe DataPack execution; they do
not shorten Full input scope. The ceiling applies to the structured input
buffer and is not a claim about total RSS or allocator overhead.

## File-to-file timing boundary

Both competitors use the same file-to-file boundary.

For compression, timing starts immediately before opening/reading the prepared
input for that measured operation and ends after the complete artifact has
been written and flushed. For decompression, timing starts immediately before
opening/reading the complete artifact and ends after the complete restored
file has been written and flushed.

The boundary includes the codec or archive implementation, userspace reads and
writes, and operating-system filesystem interaction visible to the process. It
does not issue or time an `fsync`, does not evict filesystem caches, and does
not claim physical-media persistence. It therefore cannot isolate CPU codec
cost from storage, page-cache, scheduler, or virtualization effects.

DataPack and standalone zstd must use the same prepared-input filesystem and
the same temporary-output filesystem within one invocation. A result is not
comparable if one competitor silently uses a different timing boundary.

## Runs, ordering, and aggregation

`--runs` accepts 1 through 25 and defaults to 3. There are no hidden warm-up
runs.

Within each numbered run, both competitors are measured. The competitor that
runs first alternates between runs instead of measuring every DataPack run and
then every standalone-zstd run. Compression and decompression use the same
interleaving principle. Alternation reduces fixed order bias but does not make
the samples independent or eliminate cache and thermal effects.

Immediately after both compression operations in a run, Compare hashes each
complete artifact outside the timed regions. The first digest for a contender
establishes its expected artifact identity; each later run must match it. This
prevents a stable byte length from concealing different artifact contents and
ensures that all compression timing samples describe the same contender
artifact. JSON V1 identifies this protocol as
`methodology.artifact_stability: "sha256_per_run"` without exposing a digest.

For each competitor and operation, elapsed durations are sorted from shortest
to longest. The JSON aggregation token is `median`:

- with an odd count, the median is the single middle duration; and
- with an even count, the median is the arithmetic mean of the two central
  durations, computed with checked duration arithmetic.

Every elapsed sample and the resulting median are included in JSON V1. The
odd default of three avoids the even-count distinction. This rule is kept
explicit so a consumer does not substitute a minimum, maximum, mean of every
sample, or best-of-run value.

## Derived metrics

For the exact compared byte count `I`, artifact size `A`, and aggregate
duration `T`:

```text
compression_ratio = I / A
throughput_mib_per_second = (I / 1,048,576) / T_seconds
```

The ratio is original-to-artifact, so for the same nonempty input a larger
number indicates a smaller artifact. Artifact size always means the complete
emitted file, including DataPack framing where applicable. Empty input gives
both contenders a ratio of zero; its storage-ratio winner is a tie even when
the empty container sizes differ.

Rates are null when a finite value cannot be represented truthfully, including
an applicable zero-byte or zero-duration case. JSON never emits NaN or
infinity. Text output uses an explicit unavailable marker for the same case.

## Validation

After creating the snapshot and outside the timed regions, Compare records its
exact length and SHA-256. It also requires each contender's complete artifact
digest to remain identical across every compression run. Every timed
decompression output is then checked immediately after its timed region: its
complete length and SHA-256 must equal the snapshot identity. JSON V1 names
this rule `methodology.validation: "sha256_roundtrip_per_run"`.

Each decompression sample is therefore validated directly. Because every
compression sample produced the same complete artifact bytes, these per-run
restoration checks also validate every compression sample transitively.
Compare does not infer either relationship from equal file sizes.

Quick therefore validates the exact prefix it measured, while its report
remains `partially_validated` with respect to the complete source. Full
validates the complete snapshot and reports `validated`.

A mismatch is not a benchmark datum. It is a corruption-risk error: the
command emits no winners or successful JSON report and exits unsuccessfully.
This includes a per-run artifact-identity change as well as any per-run
restored-output length or SHA-256 mismatch. Snapshot, artifact, and
restored-output digest values are never included in text or JSON output.

## Winner rules

The report identifies exactly three factual categories:

- `best_storage_ratio`: for nonempty input, the smaller complete artifact; for
  empty input, a tie because both ratios are zero;
- `fastest_compression`: the shorter median compression duration; and
- `fastest_decompression`: the shorter median decompression duration.

Exact equality produces a tie. Empty input is the explicit storage exception:
it produces a storage tie regardless of differing artifact sizes. Winner
selection otherwise uses the underlying values before presentation rounding.
There is no tolerance band, weighting, overall winner, composite score,
confidence percentage, statistical-significance claim, or recommendation.

The words “faster” and “smaller” apply only to the recorded values from that
invocation. Quick winners apply only to the measured prefix. Neither Quick nor
Full establishes performance on another input, machine, filesystem, run
count, codec level, or DataPack version.

## Temporary artifacts and cleanup

Comparison creates an immutable measured-input snapshot in both modes, two
compressed artifacts, and restored outputs. Paths are collision-resistant and
files are created without clobbering an existing entry. No temporary path is
included in the report.

Cleanup is explicitly completed before a successful report is written; success
therefore means that all comparison artifacts were removed. On an operation
failure, the workspace guard attempts best-effort cleanup during unwinding. If
the filesystem itself rejects removal, artifacts can remain and the command
does not claim cleanup succeeded. Compare intentionally has no `--keep-temp`
switch, so a successful invocation does not leave benchmark litter.

Temporary-filesystem placement affects file-to-file timings. Reports shared
for review must record relevant placement outside the privacy-safe JSON when
it materially affects interpretation.

## WSL and `/mnt/c`

When the input or temporary artifacts are under `/mnt/c` in WSL, the measured
file operations include WSL/Windows filesystem translation and NTFS behavior.
Such numbers must be described as WSL/NTFS-path measurements. They must not be
presented as native-Linux filesystem throughput or used to claim a codec-only
speedup.

Likewise, moving the input or temporary directory between `/mnt/c`, the WSL
Linux filesystem, a network mount, and another device changes the environment.
Results from those placements are not directly interchangeable without that
context.

## Reproducibility checklist

When publishing a comparison, record at least:

- DataPack commit or release;
- mode, exact compared byte count, and sample limit when Quick;
- run count and `median` aggregation;
- actual DataPack archive mode and standalone zstd level 3;
- operating system and architecture;
- Rust/build profile when comparing development builds;
- input and temporary filesystem classes, including WSL `/mnt/c`; and
- material concurrent load or resource constraints.

These facts support reproduction. They do not turn a small run count into a
statistical performance guarantee.
