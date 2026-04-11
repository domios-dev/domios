#!/usr/bin/env bash
# Restore a mem fixture snapshot into the dev instance.
#
# Usage:
#   ./restore.sh clean
#   ./restore.sh loaded
#
# After restore, dev is running with the snapshot state.

set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
REPO="$(cd "$HERE/../.." && pwd)"
DEV_HOME="$HOME/.codeterm-dev"
DB="$DEV_HOME/mem/knowledge.db"
BLOBS="$DEV_HOME/mem/blobs"

API="http://127.0.0.1:7686"

target="${1:-}"
if [ -z "$target" ]; then
    echo "usage: $0 <clean|loaded>" >&2
    exit 1
fi

SRC_DB="$HERE/${target}.db"
SRC_BLOBS="$HERE/${target}.blobs"

if [ ! -f "$SRC_DB" ]; then
    echo "snapshot $SRC_DB not found. run ./build.sh first" >&2
    exit 1
fi

log() { printf '[restore] %s\n' "$*"; }

log "killing dev instance"
(cd "$REPO" && ./build.sh kill) >/dev/null 2>&1 || true
sleep 1

log "restoring $target snapshot -> $DB"
mkdir -p "$(dirname "$DB")"
# Wipe any existing WAL files that would conflict with the restored DB
rm -f "$DB" "${DB}-shm" "${DB}-wal"
cp "$SRC_DB" "$DB"

rm -rf "$BLOBS"
if [ -d "$SRC_BLOBS" ]; then
    cp -R "$SRC_BLOBS" "$BLOBS"
else
    mkdir -p "$BLOBS"
fi

# Wipe general agent transient state so it re-boots with a fresh GA session
# but keeps the mem snapshot
rm -f "$DEV_HOME/general/soul.md" \
      "$DEV_HOME/general/bootstrap.md"
# Clear GA claude session so it doesn't resume into stale context
find "$HOME/.claude/projects/" -path "*general*"     -name "*.jsonl" -delete 2>/dev/null || true
find "$HOME/.claude/projects/" -path "*codeterm-dev*" -name "*.jsonl" -delete 2>/dev/null || true
if [ -f "$DEV_HOME/sessions/last.json" ]; then
    sed -i '' 's/"claude_session_id": "[^"]*"/"claude_session_id": null/g' "$DEV_HOME/sessions/last.json"
fi

log "starting dev"
(cd "$REPO" && ./build.sh dev) >/dev/null 2>&1 &

for i in $(seq 1 30); do
    if curl -s "$API/api/health" 2>/dev/null | grep -q ok; then
        log "dev ready"
        item_count=$(sqlite3 "$DB" "SELECT COUNT(*) FROM mem_items")
        ent_count=$(sqlite3 "$DB" "SELECT COUNT(*) FROM entities")
        rel_count=$(sqlite3 "$DB" "SELECT COUNT(*) FROM relations")
        log "restored $target: items=$item_count entities=$ent_count relations=$rel_count"
        exit 0
    fi
    sleep 2
done

echo "[restore] dev didn't come up" >&2
exit 1
