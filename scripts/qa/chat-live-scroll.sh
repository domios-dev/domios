#!/usr/bin/env bash
set -euo pipefail
if [[ "${1:-}" == "--check-report" ]]; then
  shift
  exec node scripts/qa/chat-scroll-report.mjs "$@"
fi
source "$(git rev-parse --show-toplevel)/evals/_lib/lib.sh"
dc tab status --tab "${CHAT_QA_TAB:-91a91705}"
dev_run node scripts/qa/chat-live-scroll.mjs
