#!/usr/bin/env bash
# Version-update drill: prove a published new plugin version is picked up, still works,
# and an incompatible version is gated without breaking the daemon.
# Touches ONLY ~/.codeterm-dev (dev), never prod. Restarts dev between versions.
set -uo pipefail
ROOT="$(git rev-parse --show-toplevel)"
source "$ROOT/evals/_lib/lib.sh"

DEVPLUG="$HOME/.codeterm-dev/plugins/transcriber"
LOG="$HOME/.codeterm-dev/logs/codeterm.log"
# Marketplace plugins live in the sibling codeterm-plugins repo now (not this
# repo's plugins/). Allow an override; default to a checkout next to this one.
SRC="${CODETERM_PLUGINS_REPO:-$ROOT/../codeterm-plugins}/transcriber"
if [ ! -f "$SRC/plugin.json" ]; then
  echo "skip: transcriber plugin source not found at $SRC (clone rollacode/codeterm-plugins next to this repo, or set CODETERM_PLUGINS_REPO)" >&2
  exit 0
fi
pass=0 fail=0
ok(){ echo "  ✓ $1"; pass=$((pass+1)); }
bad(){ echo "  ✗ $1"; fail=$((fail+1)); }

restart() { (cd "$ROOT" && ./build.sh kill >/dev/null 2>&1; ./build.sh dev --wait >/dev/null 2>&1); }
install_version() {
  local ver="$1" marker="$2"
  rm -rf "$DEVPLUG"; mkdir -p "$DEVPLUG"
  sed "s/\"version\": \"[^\"]*\"/\"version\": \"$ver\"/" "$SRC/plugin.json" > "$DEVPLUG/plugin.json"
  # marker lets us see which build is live in the logs
  { echo "// build-marker: $marker"; cat "$SRC/plugin.js"; } > "$DEVPLUG/plugin.js"
}

echo "== transcriber version-update drill =="

echo "[1] install v0.1.0"
install_version "0.1.0" "v010"; restart
grep -q "\[js-plugin\] transcriber" "$LOG" && ok "v0.1.0 loaded" || bad "v0.1.0 did not load"

echo "[2] publish + update to v0.1.1"
: > "$LOG" 2>/dev/null || true   # rotate so we read fresh load lines
install_version "0.1.1" "v011"; restart
if grep -q "transcriber registered as transcriber" "$LOG"; then ok "v0.1.1 re-registered after update"; else bad "v0.1.1 not registered"; fi

echo "[3] incompatible version is gated (daemon survives)"
# bump hostApi beyond what this build supports
sed -i.bak 's/"hostApi": [0-9]*/"hostApi": 999/' "$DEVPLUG/plugin.json" && rm -f "$DEVPLUG/plugin.json.bak"
: > "$LOG" 2>/dev/null || true
restart
if dev_alive; then ok "daemon still boots with an incompatible plugin present"; else bad "daemon failed to boot"; fi
if grep -qiE "skip|incompatible|unsupported" "$LOG"; then ok "incompatible version skipped (logged)"; else bad "no skip recorded"; fi

echo "[cleanup] restore a working transcriber"
install_version "0.1.0" "restore"; restart >/dev/null 2>&1

echo "== result: $pass passed, $fail failed =="
[ "$fail" -eq 0 ]
