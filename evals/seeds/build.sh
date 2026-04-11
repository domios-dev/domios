#!/usr/bin/env bash
# Build clean + loaded snapshots for mem fixture testing.
#
# Usage:
#   ./build.sh             # builds both clean.db and loaded.db
#   ./build.sh clean       # only clean
#   ./build.sh loaded      # only loaded (assumes clean already exists)

set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
REPO="$(cd "$HERE/../.." && pwd)"
DEV_HOME="$HOME/.codeterm-dev"
DB="$DEV_HOME/mem/knowledge.db"
BLOBS="$DEV_HOME/mem/blobs"

API="http://127.0.0.1:7686"

target="${1:-both}"

log() { printf '[build] %s\n' "$*"; }

wait_health() {
    for i in $(seq 1 30); do
        if curl -s "$API/api/health" 2>/dev/null | grep -q ok; then
            return 0
        fi
        sleep 2
    done
    echo "[build] dev didn't come up" >&2
    return 1
}

wipe_dev() {
    log "killing dev instance"
    (cd "$REPO" && ./build.sh kill) >/dev/null 2>&1 || true
    sleep 1
    log "wiping dev state"
    rm -f  "$DB" "${DB}-shm" "${DB}-wal" \
           "$DEV_HOME/mem/workers.json" \
           "$DEV_HOME/mem/.bundled-seeded-v1"
    rm -rf "$BLOBS"
    rm -f  "$DEV_HOME/general/identity.md" \
           "$DEV_HOME/general/soul.md" \
           "$DEV_HOME/general/bootstrap.md" \
           "$DEV_HOME/general/AGENT.md" \
           "$DEV_HOME/general/CLAUDE.md" \
           "$DEV_HOME/general/AGENTS.md"
    # Clear GA claude session so it doesn't resume
    find "$HOME/.claude/projects/" -path "*general*"     -name "*.jsonl" -delete 2>/dev/null || true
    find "$HOME/.claude/projects/" -path "*codeterm-dev*" -name "*.jsonl" -delete 2>/dev/null || true
    if [ -f "$DEV_HOME/sessions/last.json" ]; then
        sed -i '' 's/"claude_session_id": "[^"]*"/"claude_session_id": null/g' "$DEV_HOME/sessions/last.json"
    fi
}

start_dev() {
    log "starting dev"
    (cd "$REPO" && ./build.sh dev) >/dev/null 2>&1 &
    wait_health
    log "dev ready"
    # Give bootstrap extraction workers a moment
    sleep 8
}

snapshot() {
    local name="$1"
    local out_db="$HERE/${name}.db"
    local out_blobs="$HERE/${name}.blobs"
    log "snapshotting to $out_db"
    # Use sqlite3 .backup for atomic WAL-safe copy
    sqlite3 "$DB" ".backup '$out_db'"
    # Copy blobs dir
    rm -rf "$out_blobs"
    if [ -d "$BLOBS" ]; then
        cp -R "$BLOBS" "$out_blobs"
    else
        mkdir -p "$out_blobs"
    fi
    local size=$(du -sh "$out_db" | cut -f1)
    local item_count=$(sqlite3 "$out_db" "SELECT COUNT(*) FROM mem_items")
    local ent_count=$(sqlite3 "$out_db" "SELECT COUNT(*) FROM entities")
    local rel_count=$(sqlite3 "$out_db" "SELECT COUNT(*) FROM relations")
    local vec_count=$(sqlite3 "$out_db" "SELECT COUNT(*) FROM mem_vectors" 2>/dev/null || echo "?")
    log "$name: ${size} items=${item_count} entities=${ent_count} relations=${rel_count} vectors=${vec_count}"
}

build_clean() {
    log "=== BUILDING CLEAN ==="
    wipe_dev
    start_dev
    snapshot clean
}

build_loaded() {
    log "=== BUILDING LOADED ==="
    # Assume we just built clean, dev still running with empty mem
    log "loading 189 items via load.py"
    python3 "$HERE/load.py"
    snapshot loaded
}

case "$target" in
    clean)
        build_clean
        ;;
    loaded)
        # Restore clean first if it exists; otherwise build from scratch
        if [ -f "$HERE/clean.db" ]; then
            log "restoring clean to start loaded build"
            "$HERE/restore.sh" clean
        else
            build_clean
        fi
        build_loaded
        ;;
    both|"")
        build_clean
        build_loaded
        ;;
    *)
        echo "usage: $0 [clean|loaded|both]" >&2
        exit 1
        ;;
esac

log "done"
