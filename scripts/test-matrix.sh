#!/usr/bin/env bash
# Cross-platform test matrix: runs all lanes concurrently.
#   macos          — native host (Rust + TS unit tests)
#   linux          — Docker container (Rust + TS unit tests)
#   windows-cross  — Docker container (cargo check vs x86_64-pc-windows-msvc)
# Full Windows test EXECUTION is delegated to a Windows mesh peer.
# Usage: scripts/test-matrix.sh [lane...]   (default: all lanes)
set -uo pipefail

ROOT="$(git rev-parse --show-toplevel)"
LOGS="${TMPDIR:-/tmp}/codeterm-test-matrix"
mkdir -p "$LOGS"

# No associative arrays: macOS ships bash 3.2 and /usr/bin/env bash may
# resolve to it — keep the script 3.2-compatible.
lane_script() {
  case "$1" in
    macos)         echo "$ROOT/scripts/test-macos-native.sh" ;;
    linux)         echo "$ROOT/scripts/test-linux-docker.sh" ;;
    windows-cross) echo "$ROOT/scripts/test-windows-cross.sh" ;;
    *)             return 1 ;;
  esac
}

if [ $# -gt 0 ]; then LANES=("$@"); else LANES=(macos linux windows-cross); fi

PIDS=()
START=$SECONDS
for lane in "${LANES[@]}"; do
  script="$(lane_script "$lane")" || { echo "unknown lane: $lane" >&2; exit 2; }
  "$script" >"$LOGS/$lane.log" 2>&1 &
  PIDS+=($!)
  echo "started  $lane (pid $!, log $LOGS/$lane.log)"
done

FAIL=0
i=0
for lane in "${LANES[@]}"; do
  if wait "${PIDS[$i]}"; then
    echo "PASS  $lane"
  else
    echo "FAIL  $lane — tail of log:"
    tail -n 20 "$LOGS/$lane.log" | sed 's/^/      /'
    FAIL=1
  fi
  i=$((i + 1))
done

echo "total: $((SECONDS - START))s"
exit $FAIL
