#!/usr/bin/env bash
# Restore a mem fixture snapshot into the dev instance.
#
# Usage:
#   ./restore.sh clean
#   ./restore.sh loaded
# Set CODETERM_INSTANCE to an isolated dev instance before running.
#
# After restore, dev is running with the snapshot state.

set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
REPO="$(cd "$HERE/../.." && pwd)"
source "$REPO/evals/_lib/lib.sh"
[ -n "${CODETERM_INSTANCE:-}" ] || {
    echo "[restore] CODETERM_INSTANCE must name an isolated dev instance" >&2
    exit 7
}
dev_env || exit 7
DEV_HOME="$(dev_data_dir)" || exit 7
DB="$DEV_HOME/mem/knowledge.db"
BLOBS="$DEV_HOME/mem/blobs"

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
dev_kill_hard

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
seed_empty_storage_root

# Wipe general agent transient state so it re-boots with a fresh GA session
# but keeps the mem snapshot
rm -f "$DEV_HOME/general/soul.md" \
      "$DEV_HOME/general/bootstrap.md"

log "starting dev"
dev_start_bg
dev_wait_ready 120 || {
    echo "[restore] dev didn't come up" >&2
    exit 1
}
log "dev ready"
stats="$(dev_get /api/mem/stats)"
log "restored $target: $stats"
