# DataPack @VERSION@ internal candidate notes

Status: internal release-engineering candidate. The product version remains a
development value. This is not a public release, an RC version/tag, or a claim of
production readiness.

DataPack preserves the certified V1/V2 archive formats, exact-byte behavior,
progress, cancellation, and public error/API contracts. This candidate pipeline
packages the existing engine; it does not introduce a new compression mode.

The candidate includes Linux GNU x86_64 and Windows MSVC x86_64 CLI packages and
standard `datapack-engine` CPython `cp39-abi3` wheels. The Python import is
`datapack`. GIL-enabled CPython 3.9 through 3.14 is the established compatibility
target; candidate jobs certify their exact wheel on CPython 3.14 and reuse the
existing isolated SDK and P6 CI profile. The broader interpreter matrix remains
part of the separate CI workflow.

Before use, verify `datapack-@VERSION@-SHA256SUMS.txt` and inspect
`datapack-@VERSION@-manifest.json`. The manifest identifies the source commit,
lockfiles, toolchain, workflow run, artifact sizes/hashes, and completed platform
smokes. CLI package READMEs contain extraction and safe-use instructions. Install
only a matching, reviewed wheel with `python -m pip install /path/to/wheel.whl`;
registry installation is not provided by this candidate.

Checksums establish byte identity, not publisher authentication. There is no code
signing, notarization, supply-chain level certification, or external security audit.
The accepted `RUSTSEC-2025-0141` bincode maintenance advisory remains documented in
the security policy. No wire migration is required.

Actions artifact retention is 14 days and is not permanent distribution. macOS,
ARM, musl, free-threaded Python, PyPy, installers, publication, and automatic updates
are outside this candidate contract. P8 Desktop Foundation is a later phase;
Adaptive Compute and V3 remain separate future programs. No performance claim or
large-dataset certification is made by this release-artifact smoke test.
