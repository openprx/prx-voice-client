#!/usr/bin/env bash
set -euo pipefail

echo "=== Format check ==="
cargo fmt --check

echo "=== Clippy ==="
cargo clippy --workspace --all-targets -- -D warnings

echo "=== Tests ==="
cargo test --workspace

if [ "$(uname -s)" = Darwin ]; then
  echo "=== Virtual microphone driver (host-side load test) ==="
  scripts/check-driver.sh
fi

echo "=== All checks passed ==="
