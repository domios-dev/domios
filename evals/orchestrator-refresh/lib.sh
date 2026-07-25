#!/usr/bin/env bash
# Shared helpers for the orchestrator-refresh drills. Sources the dev-locked
# bench lib (dc / dev_get / dev_env) — NEVER invoke `codeterm` directly here.
set -u
source "$(git rev-parse --show-toplevel)/evals/_lib/lib.sh"

DEV_LOG="${DEV_LOG:-$HOME/.codeterm-dev/logs/codeterm.log}"

# Byte offset of the dev log right now — used to scope grep to THIS run so a
# prior drill's markers never contaminate the ordering assertion.
log_offset() { wc -c < "$DEV_LOG" 2>/dev/null || echo 0; }

# Emit only the log tail written since $1 bytes.
log_since() {
  local off="$1"
  tail -c "+$((off + 1))" "$DEV_LOG" 2>/dev/null
}

# First line number (1-based, within the scoped tail) matching a phase marker,
# or empty if absent. Marker grep is STRUCTURAL on the `[refresh] ... phase=<tag>`
# and `compact mode=` lines the driver emits.
marker_line() {
  local tail_text="$1" pat="$2"
  printf '%s\n' "$tail_text" | grep -nF "$pat" | head -1 | cut -d: -f1
}

# Assert marker A appears strictly before marker B in the scoped tail.
assert_order() {
  local tail_text="$1" a="$2" b="$3"
  local la lb
  la=$(marker_line "$tail_text" "$a")
  lb=$(marker_line "$tail_text" "$b")
  if [ -z "$la" ]; then echo "FAIL: marker missing: $a" >&2; return 1; fi
  if [ -z "$lb" ]; then echo "FAIL: marker missing: $b" >&2; return 1; fi
  if [ "$la" -ge "$lb" ]; then
    echo "FAIL: '$a' (line $la) must precede '$b' (line $lb)" >&2; return 1
  fi
  echo "ok: '$a' ($la) < '$b' ($lb)"
}

# Poll until a marker appears in the scoped tail or timeout (secs) elapses.
wait_for_marker() {
  local off="$1" pat="$2" timeout="${3:-200}"
  local start=$SECONDS
  while [ $((SECONDS - start)) -lt "$timeout" ]; do
    if log_since "$off" | grep -qF "$pat"; then return 0; fi
    sleep 2
  done
  echo "TIMEOUT waiting for marker: $pat" >&2
  return 1
}
