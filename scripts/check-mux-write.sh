#!/usr/bin/env bash
# Flags MuxState .write() calls that chain directly to a known read-only method.
# These should use .read() — exclusive write locks on hot paths stall concurrent
# PTY readers without cause. The compiler is the hard safety guard (a RwLockReadGuard
# can't call &mut methods), so this script catches only the obvious one-liner pattern.
# Multi-line blocks (let mux = …; mux.read_method()) are verified by the build.
set -euo pipefail

REPO="$(git rev-parse --show-toplevel)"
SRC="${REPO}/src-tauri/src"

# Methods that are &self on MultiplexerState — never need an exclusive write guard.
READ_METHODS="snapshot|get_pane|pane_id_for_pty|resolve_pane_id|get_focused_pane_id|other_session_ids|pane_ids_with_session|list_workspaces|to_session_snapshot"

BAD=$(grep -rn --include="*.rs" \
    -E "\.(mux|mux_state|state)\.write\(\)\.(${READ_METHODS})\b" \
    "${SRC}" 2>/dev/null || true)

if [[ -n "$BAD" ]]; then
    echo "❌ mux .write() chained to read-only method — use .read() instead:"
    echo "$BAD"
    exit 1
fi

echo "✅ check-mux-write: no .write() used for read-only mux access"
