#!/usr/bin/env bash
# Caller: build.sh script-guards; skill build
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

# The stable X: path shares cargo intermediates across worktrees; only gateway-held jobs may map it.
awk '
  /^[a-z_]+\(\) \{/ { fn = $1; gated = 0 }
  /^  *enter_(cargo|build)_gateway / { gated = 1 }
  /enter_stable_build_path/ && !/^enter_stable_build_path\(\)/ && !/^ *#/ && fn != "enter_build_gateway()" && !gated {
    printf "enter_stable_build_path outside the build gateway in %s (line %d)\n", fn, NR; bad = 1
  }
  END { exit bad }
' build.sh || exit 1

echo "check-queue gateway boundary: PASS"
