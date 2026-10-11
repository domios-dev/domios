#!/usr/bin/env bash
set -euo pipefail
source "$(git rev-parse --show-toplevel)/evals/_lib/lib.sh"
dc tab status --tab "${CHAT_QA_TAB:-91a91705}"
dev_run node scripts/qa/chat-live-scroll.mjs
