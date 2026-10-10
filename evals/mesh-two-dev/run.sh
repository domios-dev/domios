#!/usr/bin/env bash
# Entry point for the two-DEV mesh eval; the procedure is in RUNBOOK.md.
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$here/../.." && pwd)"
cd "$root"

usage() {
  echo 'Usage: bash evals/mesh-two-dev/run.sh offline' >&2
  echo '       bash evals/mesh-two-dev/run.sh preflight' >&2
  echo '       bash evals/mesh-two-dev/run.sh capture <windows-dev-peer-id> [run-dir]' >&2
  exit 2
}

case "${1:-}" in
  offline)
    node --test evals/mesh-two-dev/*.test.mjs
    node --test evals/mesh-webui/model.test.mjs evals/mesh-webui/terminal-capture.test.mjs
    (cd evals/mesh-webui && python3 -m unittest test_evidence test_native)
    ;;
  preflight)
    : "${CODETERM_INSTANCE:?set CODETERM_INSTANCE to the Mac DEV instance}"
    : "${CODETERM_API_PORT:?set CODETERM_API_PORT to the Mac DEV port}"
    source evals/_lib/lib.sh
    dc config get api_listener
    dc mesh status
    dc mesh peers
    ;;
  capture)
    peer="${2:-}"
    [ -n "$peer" ] || usage
    run="${3:-target-check/evals/mesh-two-dev-$(date +%Y%m%d-%H%M%S)}"
    node evals/mesh-two-dev/driver.mjs prepare "$run"
    bash evals/mesh-two-dev/capture.sh "$peer" "$run"
    ;;
  *)
    usage
    ;;
esac
