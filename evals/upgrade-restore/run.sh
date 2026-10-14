#!/usr/bin/env bash
set -euo pipefail
ROOT="$(git rev-parse --show-toplevel)"
source "$ROOT/evals/_lib/lib.sh"
case "${1:-}" in
  prepare)
    node "$ROOT/evals/upgrade-restore/prepare.mjs" "${2:?capture required}" "${3:?fixture root required}"
    ;;
  stage)
    node "$ROOT/evals/upgrade-restore/replay.mjs" "${2:?prepared desk required}" "${3:?build SHA required}" "${4:?fixture root required}" 90 stage
    ;;
  verify)
    node "$ROOT/evals/upgrade-restore/replay.mjs" "${2:?prepared desk required}" "${3:?build SHA required}" "${4:?fixture root required}" "${5:-90}"
    ;;
  *)
    echo 'Usage: run.sh prepare CAPTURE ROOT | stage ROOT/desk.json BUILD_SHA ROOT | verify ROOT/desk.json BUILD_SHA ROOT [DEADLINE<=120]' >&2
    exit 2
    ;;
esac
