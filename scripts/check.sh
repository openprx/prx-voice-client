#!/usr/bin/env bash
set -euo pipefail

echo "=== Format check ==="
cargo fmt --check

echo "=== Clippy ==="
cargo clippy --workspace --all-targets -- -D warnings

echo "=== Tests ==="
cargo test --workspace

echo "=== All checks passed ==="
