#!/usr/bin/env bash
set -euo pipefail
ROOT="$(git rev-parse --show-toplevel)"
source "$ROOT/evals/_lib/lib.sh"
if [ "${1:-}" = --dc ]; then
  shift
  dc "$@"
  exit
fi
dev_run python3 "$ROOT/evals/delivery-confirmation/drill.py" "$@"
