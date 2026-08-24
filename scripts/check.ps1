$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

& cargo fmt --all -- --check
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

& cargo check --locked
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

& cargo test --locked
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

& cargo clippy --all-targets --all-features --locked -- -D warnings
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

& cargo build --release --locked
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

# The same checks can be run from WSL by changing to this checkout and invoking
# `bash scripts/check.sh`; no user-specific checkout path is assumed.
