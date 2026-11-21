# Shared by the scripts in this directory. Sourced, not run.
# Development helpers for this machine; not part of the product.

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
app_id="org.openprx.prx-voice-client"
data_dir="${PRXVC_DATA_DIR:-$HOME/Library/Application Support/$app_id}"

# Build the helper program (release, native features) unless PRXVC_DEV_BIN names one.
dev_bin() {
  if [ -n "${PRXVC_DEV_BIN:-}" ]; then
    printf '%s\n' "$PRXVC_DEV_BIN"
    return
  fi
  (cd "$root" && cargo build --release -q -p prxvc-desktop --bin prxvc-dev \
    --features "qwentts transcribe apple-translation") >&2
  printf '%s\n' "$root/target/release/prxvc-dev"
}

# Run the helper and drop the native runtimes' own log lines (Metal and model loading).
run_dev() {
  local bin status
  bin="$(dev_bin)"
  set +e
  "$bin" "$@" 2>&1 | grep -v -E '^\[(GGML|Qwen|Pipeline|Perf|SpkExtract|Tokenizer|Codec|Talker|Voice|Model|Load|Metal|ggml)[^]]*\]'
  status=${PIPESTATUS[0]}
  set -e
  return "$status"
}
