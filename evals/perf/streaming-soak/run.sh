#!/usr/bin/env bash
set -euo pipefail
ROOT="$(git rev-parse --show-toplevel)"
source "$ROOT/evals/_lib/lib.sh"
dev_run node "$ROOT/evals/perf/streaming-soak/run.mjs" "$@"
