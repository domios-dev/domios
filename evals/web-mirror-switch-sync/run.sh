#!/usr/bin/env bash
# The web-mirror switch-storm reproduction. Dev only.
#
# Every CLI call goes through `dc`; the harness itself talks HTTP to the dev
# port the lib resolved and refuses any origin that is not 7686. Prod on 7685
# is never contacted.
#
#   evals/web-mirror-switch-sync/run.sh                      # local origin
#   WMS_PEER_NAME=air evals/web-mirror-switch-sync/run.sh    # cross-peer origin
#
# Knobs: WMS_TABCOUNT (16), WMS_SWITCHES (200), WMS_CONCURRENCY (1), WMS_RUNS (5),
#        WMS_TSX_LOADER (path to tsx's loader.mjs when this checkout has no
#        node_modules), WMS_KEEP=1 to leave the fixture workspace behind.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
# shellcheck source=/dev/null
source "$ROOT/evals/_lib/lib.sh"

TABCOUNT="${WMS_TABCOUNT:-16}"
WORKSPACE_NAME="wms-$(date +%Y%m%dT%H%M%SZ)-$$"
WORKSPACE_ID=""
TSX_LOADER="${WMS_TSX_LOADER:-$ROOT/node_modules/tsx/dist/loader.mjs}"

cleanup() {
  [ -n "$WORKSPACE_ID" ] || return 0
  [ "${WMS_KEEP:-0}" = 1 ] && return 0
  dc workspace close "$WORKSPACE_ID" --force >/dev/null 2>&1 || true
}
trap cleanup EXIT

native_path() { if is_windows; then cygpath -w "$1"; else printf '%s\n' "$1"; fi; }
file_uri() { python3 -c 'import pathlib,sys; print(pathlib.Path(sys.argv[1]).resolve().as_uri())' "$1"; }

if [ ! -f "$TSX_LOADER" ]; then
  echo "ERROR: tsx loader not found at $TSX_LOADER" >&2
  echo "       run 'npm install' in this checkout, or point WMS_TSX_LOADER at a" >&2
  echo "       loader.mjs from a checkout that has node_modules." >&2
  exit 7
fi

dev_env
echo ">>> dev target: $(dev_api_url)  data_dir=$CODETERM_DEV_DATA_DIR"

WORKSPACE_ID="$(dc workspace create "$WORKSPACE_NAME" --json \
  | python3 -c 'import json,sys; print(json.load(sys.stdin)["id"])')"
echo ">>> workspace $WORKSPACE_ID ($WORKSPACE_NAME)"

TAB_IDS=""
for _ in $(seq 1 "$TABCOUNT"); do
  id="$(dc tab new --workspace "$WORKSPACE_ID" --json \
    | python3 -c 'import json,sys; print(json.load(sys.stdin)["id"])')"
  TAB_IDS="${TAB_IDS:+$TAB_IDS,}$id"
done
echo ">>> $TABCOUNT tabs created"

PEER_ID=""
if [ -n "${WMS_PEER_NAME:-}" ]; then
  PEER_ID="$(dc mesh peers --json | python3 -c '
import json, sys
name = sys.argv[1]
peers = json.load(sys.stdin)
peers = peers.get("peers", peers) if isinstance(peers, dict) else peers
match = [p for p in peers if p.get("instance_name") == name and p.get("online")]
if not match:
    sys.exit(f"peer {name!r} is not online in the dev mesh")
print(match[0]["instance_id"])
' "$WMS_PEER_NAME")"
  echo ">>> cross-peer origin: $WMS_PEER_NAME ($PEER_ID)"
fi

ENTRY="$(native_path "$ROOT/evals/web-mirror-switch-sync/main.ts")"
LOADER_URL="$(file_uri "$(native_path "$TSX_LOADER")")"

cd "$ROOT"
WMS_ORIGIN="$(dev_api_url)" \
WMS_TOKEN="$CODETERM_API_TOKEN" \
WMS_WORKSPACE="$WORKSPACE_ID" \
WMS_TABS="$TAB_IDS" \
WMS_PEER_ID="$PEER_ID" \
MSYS_NO_PATHCONV=1 \
  node --import "$LOADER_URL" "$ENTRY"
