#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
if [ "${1:-}" = --help ]; then
  echo 'Usage: bash evals/startup-matrix/run.sh [--expect-build SHA] [--out DIR] [--dependencies CHECKOUT] [--preserve-input TAB_IDS] [--resume UNSENT_RUN_DIR | --attach CRASHED_RUN_DIR]'
  echo 'After ARMED, the Orchestrator restarts canonical DEV. See README.md. Spends provider tokens.'
  exit 0
fi
# This suite intentionally adopts the owner's default DEV, never an inherited prod connection.
unset CODETERM_INSTANCE CODETERM_API_PORT CODETERM_CLI_PATH
source "$ROOT/evals/_lib/lib.sh"
if [ "${1:-}" = --dc ]; then
  shift
  dc "$@"
  exit $?
fi
dev_run node "$ROOT/evals/startup-matrix/driver.mjs" "$@"
