#!/usr/bin/env bash
# Live plugin-lifecycle checks against dev (:7686). Run: ./build.sh dev --wait first.
# Bench-safe: sources the dev lib; never calls `codeterm` directly.
set -uo pipefail
source "$(git rev-parse --show-toplevel)/evals/_lib/lib.sh"

LOG="$HOME/.codeterm-dev/logs/codeterm.log"
pass=0 fail=0
ok()   { echo "  ✓ $1"; pass=$((pass+1)); }
bad()  { echo "  ✗ $1"; fail=$((fail+1)); }

echo "== plugins eval (dev :7686) =="
dev_alive || { echo "dev not running — ./build.sh dev --wait"; exit 7; }

# 1) capability plugins loaded + registered
echo "[1] capability plugins registered"
if grep -q "transcriber registered as transcriber" "$LOG" 2>/dev/null; then
  ok "transcriber capability registered"; else bad "transcriber not registered (check $LOG)"; fi
if grep -q "bitwarden registered as secret backend" "$LOG" 2>/dev/null; then
  ok "bitwarden secret backend registered"; else bad "bitwarden not registered (check $LOG)"; fi

# 2) transcriber works end-to-end through the daemon (needs a fake daemon configured)
echo "[2] transcriber HTTP path"
FAKE_PORT=7891
if curl -fsS "http://127.0.0.1:${FAKE_PORT}/healthz" >/dev/null 2>&1; then
  body=$(dev_post "/api/transcribe-voice" '{"path":"/tmp/nonexistent.oga"}' 2>/dev/null || true)
  # We can't supply real audio here; we assert the endpoint is wired and returns a marker.
  if echo "$body" | grep -q "codeterm:voice"; then
    ok "/api/transcribe-voice returns a voice marker"
  else
    bad "/api/transcribe-voice did not return a voice marker (got: ${body:0:120})"
  fi
else
  echo "  – skipped: no transcribe daemon on :${FAKE_PORT} (start one + restart dev with CODETERM_TRANSCRIBE_URL to exercise)"
fi

echo "== result: $pass passed, $fail failed =="
[ "$fail" -eq 0 ]
