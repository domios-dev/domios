#!/usr/bin/env bash
# Verifies wake-up scoring honors workspace context for pinned items.
#
# Setup: pin a fact under workspace W1.
# Positive: `mem wake-up --workspace W1 --no-l0` must include the title.
# Negative: `mem wake-up --workspace W2 --no-l0` must NOT include it
# (other-workspace pinned items get gated, not boosted, by Task 5 scoring).

set -uo pipefail

source "$(git rev-parse --show-toplevel)/evals/_lib/lib.sh"

if ! dev_get /api/health > /dev/null 2>&1; then
  echo "FAIL: dev daemon not reachable on port 7686. Run ./build.sh dev first." >&2
  exit 7
fi

TEST_TITLE="bench-pin-ws1-$(date +%s)-$$"

SAVE_OUT=$(dc mem save --kind fact --workspace W1 --pin --title "$TEST_TITLE" "load-bearing fact body for eval")
ITEM_ID=$(echo "$SAVE_OUT" | python -c "import sys, json
try:
    print(json.loads(sys.stdin.read()).get('id', ''))
except Exception:
    pass" 2>/dev/null || true)

cleanup() {
  if [[ -n "${ITEM_ID:-}" ]]; then
    dc mem delete "$ITEM_ID" > /dev/null 2>&1 || true
  fi
}
trap cleanup EXIT

if [[ -z "${ITEM_ID:-}" ]]; then
  echo "FAIL: could not extract id from save output" >&2
  echo "$SAVE_OUT" >&2
  exit 1
fi

WAKEUP_W1=$(dc mem wake-up --workspace W1 --no-l0)
if echo "$WAKEUP_W1" | grep -q "$TEST_TITLE"; then
  echo "PASS: wake-up in W1 included the pinned item ($TEST_TITLE)"
else
  echo "FAIL: wake-up in W1 did not include $TEST_TITLE" >&2
  echo "--- W1 wake-up output (head) ---" >&2
  echo "$WAKEUP_W1" | head -20 >&2
  exit 1
fi

WAKEUP_W2=$(dc mem wake-up --workspace W2 --no-l0)
if echo "$WAKEUP_W2" | grep -q "$TEST_TITLE"; then
  echo "FAIL: wake-up in W2 leaked workspace W1 item" >&2
  echo "--- W2 wake-up output (head) ---" >&2
  echo "$WAKEUP_W2" | head -20 >&2
  exit 1
else
  echo "PASS: wake-up in W2 correctly excluded the workspace W1 item"
fi

echo "SUCCESS: workspace-scoped pinned items honored"
