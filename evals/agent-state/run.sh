#!/usr/bin/env bash
# RT2 — agent-state replay + score eval. Offline cargo test: parses .cast
# fixtures, replays through the core SignalExtractor (P1a) + provider
# classify_pane_state, runs golden + negative oracles, prints the baseline
# scorecard. Does NOT touch the daemon; we still source the dev-locked lib so
# any future `dc` use can never leak to prod (port 7685).
set -euo pipefail

ROOT="$(git rev-parse --show-toplevel)"
# shellcheck source=/dev/null
source "$ROOT/evals/_lib/lib.sh"   # auto dev_env; never aborts when dev is down

export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-/Users/kandrey/Developer/.psetup-target}"

cd "$ROOT/src-tauri"
exec cargo test --test agent_state_replay -- --nocapture
