# Frozen DataPack compatibility baseline

These files are the pre-refactor compatibility baseline for RFC-001B0. They are
not claimed to come from a previously published release; from this point on,
their bytes are immutable compatibility inputs.

## Provenance

- Audited source baseline: `7a51af4`.
- Generated from worktree revision `ebcc2a6`; its only changes relative to the
  audited production code are the isolated rustfmt correction `fbcc6a9` and
  documentation.
- Toolchain: `rustc 1.97.0 (2d8144b78 2026-07-07)` and Cargo lockfile as
  committed.
- Tests open these archives in place. They never regenerate or overwrite them.

## Generation options

- `v1_raw_zstd.dpack`: `datapack compress v1_raw_zstd_source.csv
  v1_raw_zstd.dpack --mode fast`; defaults include `sample_mb=64`. The legacy
  planner selected `RawZstd` and the archive writer emitted format v1.
- `v1_csv_columnar.dpack`: `datapack compress v1_csv_columnar_source.csv
  v1_csv_columnar.dpack --mode fast`; defaults include `sample_mb=64`. The
  legacy planner selected `CsvColumnarDictionary` and the archive writer emitted
  format v1. The source uses LF terminators and a final LF.
- `v2_chunked_multichunk.dpack`: one call to
  `encode_raw_zstd_chunked_file` with `chunk_size_bytes=64`, `threads=1`,
  `max_in_flight_chunks=1`, backend `chunked-raw-zstd`,
  `adaptive_level=false`, and `profile=false`. This produced 10 chunks in
  format v2. The library entry point keeps the committed source small while
  exercising the same v2 reader; the CLI accepts chunk sizes only in whole MiB.

## Files

| File | Format / strategy | Bytes | SHA-256 | Purpose |
|---|---|---:|---|---|
| `v1_raw_zstd_source.csv` | source, LF | 810 | `81f5cc20f74dd9bea672f7bdbb5ecc1bf5fdb5379223c4af8c2cb650f54fb994` | Exact expected bytes for v1 RawZstd. |
| `v1_raw_zstd.dpack` | v1 / `RawZstd` | 331 | `7d467f55a0927c0fafc0a75c1fdf7be5f7ad2db430a8bee65dc9312c71ddadab` | Freezes the v1 header, bincode metadata graph, zstd payload, reader, and restore path. |
| `v1_csv_columnar_source.csv` | source, LF | 1269 | `320c19793eea868aa109cb798887f92394de114879a3d5df3ed55fb1f49d16bf` | Exact CSV with quoted commas, escaped quotes, empty fields, leading zeros, and a final LF. |
| `v1_csv_columnar.dpack` | v1 / `CsvColumnarDictionary` | 265 | `0210e1b969e124731cbe374fb030fc75e391949e588195b676b2a7a2b36070a5` | Freezes v1 metadata plus the byte-exact DCSV01 columnar payload. |
| `v2_chunked_source.bin` | source, LF | 588 | `f41051e6c9fcbf543208b67f26116e0f11188d997d52815f7ee24444f643f8f3` | Exact expected bytes spanning ten 64-byte chunks. |
| `v2_chunked_multichunk.dpack` | v2 / chunked `RawZstd` | 1542 | `a0644b941c86dac0c0d0ccd37e8cd6f07fdd25d45f38672a0d0c626faa5f8cbd` | Freezes the v2 header, table, per-chunk hashes, global hash, reader, and restore path. |

## Maintenance rule

A hash change is a compatibility event, not routine snapshot maintenance. Do
not regenerate an archive to make a failing decoder test pass. If a future
format is intentional, add a newly named fixture with explicit provenance.
