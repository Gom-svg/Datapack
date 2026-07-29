$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

& cargo fmt -- --check
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

& cargo test
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

& cargo clippy --all-targets -- -D warnings
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

& cargo build --release
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

# If local Windows execution policy or binary execution prevents the native
# checks, run the same commands under WSL from the repository root:
# wsl --cd /mnt/c/Users/gompr/Documents/datapack -- bash scripts/check.sh
