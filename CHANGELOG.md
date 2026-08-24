# Changelog

All notable DataPack product changes are recorded in this file. The format is
based on Keep a Changelog, and package versions follow Semantic Versioning as
defined by the DataPack release policy.

DataPack has not made a public product release. The current `0.1.0` value is a
development version and does not imply PyPI, crates.io, binary, or production
release availability.

## [Unreleased]

### Added

- Productization P0 readiness audit and evidence ledger.
- Productization P1 release/versioning, release-note, artifact, checksum, and
  supported-platform policies.
- Automated product-identity and cross-language version consistency checks.

### Changed

- The authorized future PyPI distribution identity is `datapack-engine`; the
  Python import remains `datapack`.
- The internal Rust package is explicitly non-publishable during the current
  Productization Foundation program.

### Security

- No `.dpack` v1/v2 wire behavior, protected fixture, integrity control,
  resource limit, or transactional-output guarantee changed in P0 or P1.
