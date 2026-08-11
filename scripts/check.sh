#!/usr/bin/env bash
set -euo pipefail

cargo fmt --check
cargo check --locked
cargo test --locked
cargo clippy --all-targets --all-features -- -D warnings
cargo build --release --locked
