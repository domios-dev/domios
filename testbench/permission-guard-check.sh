#!/usr/bin/env bash
# Permission-box false-idle guard check.
#
# Regression for the guard gap exposed by Lead's relaxed composer_ready (idle when
# the cursor is visible in the bottom region, alt_screen no longer required): a
# numbered-choice PERMISSION box that keeps its cursor VISIBLE must NOT be classified
# Idle. classify_terminal_menu_question is gated on cursor_hidden, so a visible-cursor
# choice box skips question detection and composer_ready false-idles it.
#
# Renders the synthetic cast through the CURRENT classifier and asserts state != idle.
#   BEFORE Lead's guard fix: 121/121 frames idle  -> FAIL (the bug)
#   AFTER  Lead's guard fix: frames should be question (or at least not idle) -> PASS
set -uo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"; cd "$ROOT"
CAST="testbench/replays/synthetic/permission_box_visible_cursor_v1/raw.cast"
RT="src-tauri/target/debug/examples/render_timeline"
OUT="${TMPDIR:-/tmp}/permission_guard.$$.json"
[ -x "$RT" ] || (cd src-tauri && cargo build --example render_timeline >/dev/null 2>&1)
# Synthetic cast: no provider, so the band is stated fixture data.
"$RT" "$CAST" "$OUT" --status-bar-rows 0 >/dev/null 2>&1

python3 - "$OUT" <<'EOF'
import json,sys
fr=json.load(open(sys.argv[1]))['frames']
from collections import Counter
# Count idle only on frames where the box is visibly painted (its numbered allow/deny
# choice rows present). render_timeline's first frame is a blank pre-paint seed (empty
# screen, cursor 0,0) emitted before the cast's first output event — no box exists on it
# yet, so its idle classification is honest, not a false-idle. testbench-only gate.
visible=[
    f for f in fr
    if any(('allow' in row.lower() or 'deny' in row.lower()) and any(ch.isdigit() for ch in row)
           for row in (f.get('screen') or []))
]
c=Counter(f['detector']['state'] for f in visible)
idle=c.get('idle',0); n=len(visible); final=fr[-1]['detector']['state']
cur=fr[-1].get('cursor'); ds=fr[-1].get('decset')
print(f"permission_box_visible_cursor_v1: visible_frames={n}/{len(fr)} states={dict(c)} final={final}")
print(f"  final cursor={cur} decset={ds}")
# PASS = not classified idle (a pending permission must read as Question, never Idle)
if n == 0:
    print("RESULT: FAIL (fixture did not render a visible permission box)"); sys.exit(1)
elif idle==0:
    print("RESULT: PASS (permission box never classified idle)"); sys.exit(0)
elif idle==n:
    print(f"RESULT: FAIL (FALSE-IDLE — all {n} frames idle on a pending permission box)"); sys.exit(1)
else:
    print(f"RESULT: PARTIAL ({idle}/{n} frames idle — permission box intermittently false-idles)"); sys.exit(1)
EOF
