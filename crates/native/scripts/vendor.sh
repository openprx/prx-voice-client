#!/usr/bin/env bash
# Refresh a vendored native runtime from a local git checkout at its pinned commit.
#
# Usage: crates/native/scripts/vendor.sh <qwentts|transcribe> <path-to-upstream-checkout>
#
# The checkout must be at the commit recorded in the crate's VENDOR.toml (the
# script refuses otherwise). For a new pin: clone upstream, check out the new
# commit (with submodules), edit VENDOR.toml, then run this script. Only the
# files a macOS (CPU + Metal) build needs are copied; nothing is edited.
# Needs: git, rsync.
set -euo pipefail

runtime="${1:?runtime: qwentts or transcribe}"
src="${2:?path to the upstream checkout}"
root="$(cd "$(dirname "$0")/.." && pwd)"

case "$runtime" in
  qwentts)   crate="$root/qwentts-sys";   name="qwentts.cpp" ;;
  transcribe) crate="$root/transcribe-sys"; name="transcribe.cpp" ;;
  *) echo "unknown runtime: $runtime" >&2; exit 2 ;;
esac

pin="$(sed -n 's/^commit *= *"\(.*\)"/\1/p' "$crate/VENDOR.toml" | head -1)"
head="$(git -C "$src" rev-parse HEAD)"
if [ "$pin" != "$head" ]; then
  echo "checkout is at $head but VENDOR.toml pins $pin" >&2
  exit 1
fi

dest="$crate/vendor/$name"
rm -rf "$dest"
mkdir -p "$dest"

# ggml backends this project never builds (CUDA, Vulkan, OpenCL, ...) and ggml's
# own examples, tests and docs are left out; CPU, Metal and BLAS stay.
ggml_filter=(
  --exclude='/ggml/examples' --exclude='/ggml/tests' --exclude='/ggml/docs'
  --exclude='/ggml/ci' --exclude='/ggml/scripts' --exclude='/ggml/.git*' --exclude='/ggml/.pi'
  --exclude='/ggml/src/ggml-cann' --exclude='/ggml/src/ggml-cuda'
  --exclude='/ggml/src/ggml-et' --exclude='/ggml/src/ggml-hexagon'
  --exclude='/ggml/src/ggml-hip' --exclude='/ggml/src/ggml-musa'
  --exclude='/ggml/src/ggml-opencl' --exclude='/ggml/src/ggml-openvino'
  --exclude='/ggml/src/ggml-rpc' --exclude='/ggml/src/ggml-sycl'
  --exclude='/ggml/src/ggml-virtgpu' --exclude='/ggml/src/ggml-vulkan'
  --exclude='/ggml/src/ggml-webgpu' --exclude='/ggml/src/ggml-zdnn'
  --exclude='/ggml/src/ggml-zendnn'
)

case "$runtime" in
  qwentts)
    rsync -a "${ggml_filter[@]}" \
      --include='/LICENSE' --include='/src/***' --include='/ggml/***' --exclude='*' \
      "$src/" "$dest/"
    ;;
  transcribe)
    rsync -a "${ggml_filter[@]}" \
      --include='/LICENSE' --include='/THIRD-PARTY-LICENSES.md' --include='/CMakeLists.txt' \
      --include='/include/***' --include='/cmake/***' --include='/src/***' \
      --include='/ggml/***' --exclude='*' \
      "$src/" "$dest/"
    ;;
esac
find "$dest" -name .DS_Store -delete
echo "vendored $name at $head into $dest"
find "$dest" -type f | wc -l
du -sk "$dest"
