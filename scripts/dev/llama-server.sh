#!/usr/bin/env bash
# Start and stop the translation server exactly as the default provider expects: Gemma 4 E4B
# (the installed model file), served as `gemma-4-e4b-it-qat-q4-0` on http://127.0.0.1:8080/v1,
# two parallel slots (one per direction), 8192 tokens of context, all layers on the GPU. Thinking
# is switched off by the provider in every request (chat_template_kwargs), so no server flag is
# needed for it.
#
# Usage: scripts/dev/llama-server.sh start|stop|status
#   start   starts the server in the background, writes a PID file and waits (at most 120 s)
#           until /health answers; does nothing when the server of this script already runs and
#           refuses when something else answers on the port
#   stop    stops only the server this script started (the PID file's process, after checking
#           that it is a llama-server), waits up to 20 s, then kills that one process
#   status  says whether the server answers and whether this script started it
# Environment: LLAMA_SERVER (default /opt/homebrew/bin/llama-server), LLAMA_PORT (default 8080),
#   LLAMA_MODEL (default: the model in the app's model store, else the file under code/prx-voice).
set -euo pipefail
. "$(dirname "$0")/_common.sh"

llama="${LLAMA_SERVER:-/opt/homebrew/bin/llama-server}"
port="${LLAMA_PORT:-8080}"
alias_name="gemma-4-e4b-it-qat-q4-0"
dev_dir="$data_dir/dev"
pid_file="$dev_dir/llama-server.pid"
log_file="$dev_dir/llama-server.log"

model_file() {
  if [ -n "${LLAMA_MODEL:-}" ]; then printf '%s\n' "$LLAMA_MODEL"; return; fi
  local f
  for f in "$data_dir"/models/store/$alias_name/*/gemma-4-E4B_q4_0-it.gguf \
           "$root/../prx-voice/models/llm/gemma-4-E4B_q4_0-it.gguf"; do
    [ -f "$f" ] && { printf '%s\n' "$f"; return; }
  done
  return 1
}

healthy() { curl -sf --max-time 2 "http://127.0.0.1:$port/health" >/dev/null 2>&1; }

# The PID of the server this script started, if it still runs and is a llama-server.
ours() {
  [ -f "$pid_file" ] || return 1
  local pid; pid="$(cat "$pid_file")"
  [ -n "$pid" ] && kill -0 "$pid" 2>/dev/null || return 1
  ps -p "$pid" -o command= 2>/dev/null | grep -q 'llama-server' || return 1
  printf '%s\n' "$pid"
}

case "${1:-}" in
  start)
    if pid="$(ours)"; then echo "already running (pid $pid), http://127.0.0.1:$port/v1"; exit 0; fi
    rm -f "$pid_file"
    if healthy; then echo "something else already answers on port $port; not started" >&2; exit 1; fi
    [ -x "$llama" ] || { echo "llama-server not found at $llama" >&2; exit 1; }
    model="$(model_file)" || { echo "the Gemma model file was not found (scripts/dev/adopt-models.sh)" >&2; exit 1; }
    mkdir -p "$dev_dir"
    nohup "$llama" -m "$model" --alias "$alias_name" --host 127.0.0.1 --port "$port" \
      --parallel 2 -c 8192 -ngl 99 >"$log_file" 2>&1 &
    echo $! >"$pid_file"
    for _ in $(seq 1 120); do
      if healthy; then echo "started (pid $(cat "$pid_file")), http://127.0.0.1:$port/v1, log $log_file"; exit 0; fi
      ours >/dev/null || { echo "the server exited; see $log_file" >&2; rm -f "$pid_file"; exit 1; }
      sleep 1
    done
    echo "the server did not answer within 120 s; stopping it; see $log_file" >&2
    "$0" stop || true
    exit 1
    ;;
  stop)
    if ! pid="$(ours)"; then
      rm -f "$pid_file"
      echo "no server of this script is running"
      exit 0
    fi
    kill "$pid"
    for _ in $(seq 1 20); do kill -0 "$pid" 2>/dev/null || break; sleep 1; done
    if kill -0 "$pid" 2>/dev/null; then kill -9 "$pid" 2>/dev/null || true; fi
    rm -f "$pid_file"
    echo "stopped (pid $pid)"
    ;;
  status)
    if pid="$(ours)"; then
      healthy && echo "running and answering (pid $pid, started by this script)" \
              || echo "running (pid $pid, started by this script) but not answering yet"
    elif healthy; then
      echo "answering on port $port, not started by this script"
    else
      echo "not running"
      exit 1
    fi
    ;;
  *)
    echo "usage: $0 start|stop|status" >&2
    exit 2
    ;;
esac
