#!/usr/bin/env bash
# Smoke: a passing single gate must exit 0 and print QA_RUN (prod-tab env).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
out="$(CODETERM_API_PORT=7685 "$ROOT/scripts/qa-gate.sh" run shell-guards --allow-dirty --timeout 120 2>&1)"
echo "$out" | grep -q '^QA_RUN status=pass' || {
  echo "qa-gate-smoke: missing QA_RUN pass summary" >&2
  echo "$out" >&2
  exit 1
}
echo "qa-gate-smoke: ok"
