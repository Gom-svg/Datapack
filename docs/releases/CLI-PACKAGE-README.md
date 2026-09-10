# DataPack @VERSION@ CLI candidate

Internal technical evaluation build; not a public release or a production-readiness
claim. Verify this package against the SHA-256 file and source commit in the
associated candidate manifest before using it.

Extract the complete package into a private directory. On Linux invoke `./datapack`;
on Windows PowerShell invoke `.\datapack.exe`. No installer or PATH modification is
required. Use the package matching your operating system and x86_64 architecture.
The Linux CLI uses GNU libc; consult the candidate manifest for its observed libc
symbol requirements and tested runtime. The Python wheel's manylinux baseline is
a separate contract.

The following examples use `datapack`; substitute the invocation above:

```text
datapack --version
datapack --help
datapack compress sample.csv sample.dpack
datapack validate sample.dpack --against sample.csv --json
datapack decompress sample.dpack restored.csv
```

Existing outputs are protected by default. Choose new output names and keep your
original data. Leave integrity verification enabled. Read `SECURITY.md` before
processing untrusted archives and select resource limits appropriate to your host.

This package contains only the executable, this README, `LICENSE-MIT`,
`SECURITY.md`, and `RELEASE-NOTES.md`. There is no automatic update or network
service. See the accompanying candidate bundle for the two platform Python wheels,
checksums, exact source provenance, and certification metadata.
