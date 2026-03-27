#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

echo "Building prxvc-ffi for Android targets..."

# Requires Android NDK and cargo-ndk
# Install: cargo install cargo-ndk
# Set ANDROID_NDK_HOME environment variable

cargo ndk -t arm64-v8a -t armeabi-v7a -t x86_64 -o "$SCRIPT_DIR/app/src/main/jniLibs" build -p prxvc-ffi --release

echo "Done. Libraries at: app/src/main/jniLibs/"
