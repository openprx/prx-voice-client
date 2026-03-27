#!/usr/bin/env bash
set -euo pipefail

echo "=== PRX Voice Client Statistics ==="
echo ""
echo "Rust source files: $(find crates apps -name '*.rs' | wc -l | tr -d ' ')"
echo "Rust lines:        $(find crates apps -name '*.rs' -exec cat {} + | wc -l | tr -d ' ')"
echo "Swift files:       $(find apps/ios -name '*.swift' 2>/dev/null | wc -l | tr -d ' ')"
echo "Kotlin files:      $(find apps/android -name '*.kt' 2>/dev/null | wc -l | tr -d ' ')"
echo "Total tests:       $(cargo test --workspace 2>&1 | grep '^test result' | awk '{s+=$4}END{print s}')"
echo "Crates:            $(ls crates/ | wc -l | tr -d ' ')"
echo ""
echo "=== Build Check ==="
cargo check --workspace 2>&1 | tail -1
echo ""
echo "=== Format Check ==="
cargo fmt --check 2>&1 && echo "OK" || echo "NEEDS FORMATTING"
