#!/usr/bin/env bash
# Build clean + loaded snapshots for mem fixture testing.
#
# Usage:
#   ./build.sh             # builds both clean.db and loaded.db
#   ./build.sh clean       # only clean
#   ./build.sh loaded      # only loaded (assumes clean already exists)
# Set CODETERM_INSTANCE to an isolated dev instance before running.

set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
REPO="$(cd "$HERE/../.." && pwd)"
source "$REPO/evals/_lib/lib.sh"
[ -n "${CODETERM_INSTANCE:-}" ] || {
    echo "[build] CODETERM_INSTANCE must name an isolated dev instance" >&2
    exit 7
}
dev_env || exit 7
DEV_HOME="$(dev_data_dir)" || exit 7
DB="$DEV_HOME/mem/knowledge.db"
BLOBS="$DEV_HOME/mem/blobs"

target="${1:-both}"

log() { printf '[build] %s\n' "$*"; }

wait_health() {
    dev_wait_ready 120 || {
        echo "[build] dev didn't come up" >&2
        return 1
    }
}

wipe_dev() {
    log "killing dev instance"
    dev_kill_hard
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
    seed_empty_storage_root
}

start_dev() {
    log "starting dev"
    dev_start_bg
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
    log "$name: ${size}"
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
    local api token
    api="$(dev_api_url)"
    token="$(dev_token)" || return 7
    python3 "$HERE/load.py" --api "$api" --token "$token"
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
