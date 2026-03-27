#!/usr/bin/env bash
set -euo pipefail

# Build Rust FFI library for iOS targets
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

echo "Building prxvc-ffi for iOS..."

cd "$PROJECT_ROOT"

# Build for iOS Simulator (arm64)
cargo build -p prxvc-ffi --target aarch64-apple-ios-sim --release

# Build for iOS Device (arm64)
cargo build -p prxvc-ffi --target aarch64-apple-ios --release

echo "Done. Libraries at:"
echo "  Simulator: target/aarch64-apple-ios-sim/release/libprxvc_ffi.a"
echo "  Device:    target/aarch64-apple-ios/release/libprxvc_ffi.a"
