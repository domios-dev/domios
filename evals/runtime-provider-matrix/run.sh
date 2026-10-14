#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
source "$ROOT/evals/_lib/lib.sh"
dev_get /api/health >/dev/null
: "${PROVIDER:=codex}"
export PROVIDER
export MATRIX_PAD="${CODETERM_PAD_DIR:?Run in an agent tab with an AgentPad}"
dev_run node "$ROOT/evals/runtime-provider-matrix/matrix.mjs"
