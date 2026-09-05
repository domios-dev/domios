#!/usr/bin/env bash
set -euo pipefail

ROOT="$(git rev-parse --show-toplevel)"
source "$ROOT/evals/_lib/lib.sh"
if [[ " ${*:-} " != *" --self-test "* ]]; then
  dev_alive || { echo "ERROR: dev is not running on port 7686" >&2; exit 1; }
fi

exec dev_run python "$ROOT/evals/chat-resume-latency/run.py" "$@"
