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
"$RT" "$CAST" "$OUT" >/dev/null 2>&1

python3 - "$OUT" <<'EOF'
import json,sys
fr=json.load(open(sys.argv[1]))['frames']
from collections import Counter
c=Counter(f['detector']['state'] for f in fr)
idle=c.get('idle',0); n=len(fr); final=fr[-1]['detector']['state']
cur=fr[-1].get('cursor'); ds=fr[-1].get('decset')
print(f"permission_box_visible_cursor_v1: states={dict(c)} final={final}")
print(f"  final cursor={cur} decset={ds}")
# PASS = not classified idle (a pending permission must read as Question, never Idle)
if idle==0:
    print("RESULT: PASS (permission box never classified idle)"); sys.exit(0)
elif idle==n:
    print(f"RESULT: FAIL (FALSE-IDLE — all {n} frames idle on a pending permission box)"); sys.exit(1)
else:
    print(f"RESULT: PARTIAL ({idle}/{n} frames idle — permission box intermittently false-idles)"); sys.exit(1)
EOF
