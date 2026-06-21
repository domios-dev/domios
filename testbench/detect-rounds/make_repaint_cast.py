#!/usr/bin/env python3
"""Synthesize a post-turn idle-repaint cast (asciicast v2) to prove no indefinite
stuck-Working under live-like repaint. Timeline:
  - a cursor-style WORKING turn (spinner + `→ … ctrl+c to stop` footer),
  - then the IDLE composer (`→ Add a follow-up`, no stop-hint),
  - then a tiny repaint of the SAME idle composer every 2s for ~24s.
Each repaint resets byte_idle to ~0, so byte_idle NEVER reaches the 5s stuck-Working
net. A timeout-only classifier would stay stuck Working forever; the structural
composer signal must keep it Idle. Output: <out.cast>.
"""
import json, sys

COLS, ROWS = 100, 30
out = sys.argv[1] if len(sys.argv) > 1 else "/tmp/repaint.cast"

def home_clear():
    return "\x1b[2J\x1b[H"

def at(row, col, text):  # 1-based
    return f"\x1b[{row};{col}H{text}"

def working_screen():
    s = home_clear()
    s += at(2, 1, "User: do a thing")
    s += at(4, 1, "Assistant: working on it...")
    s += at(27, 1, " ⠋⠋ Composing  42 tokens")
    s += at(28, 1, "→ Add a follow-up" + " " * 50 + "ctrl+c to stop")
    s += at(29, 1, "Composer 2.5 · 7%" + " " * 40 + "Run Everything")
    s += at(30, 1, "/tmp/ct-repaint-proof")
    return s

def idle_screen():
    s = home_clear()
    s += at(2, 1, "User: do a thing")
    s += at(4, 1, "Assistant: done. Here is the result.")
    s += at(28, 1, "→ Add a follow-up")
    s += at(29, 1, "Composer 2.5 · 7%" + " " * 40 + "Run Everything")
    s += at(30, 1, "/tmp/ct-repaint-proof")
    s += "\x1b[28;20H"  # park cursor in composer
    return s

rows = []
def ev(t, data):
    rows.append([round(t, 3), "o", data])

# 0-3s: working
ev(0.0, working_screen())
for t in [0.5, 1.0, 1.5, 2.0, 2.5, 3.0]:
    ev(t, at(27, 2, "⠋"))  # spinner tick (keeps it working)
# 3.5s: turn completes -> idle composer
ev(3.5, idle_screen())
# 5.5s..27.5s: repaint the idle composer every 2s (byte_idle resets, stays < 5s)
t = 5.5
while t <= 27.5:
    ev(t, idle_screen())
    t += 2.0

header = {"version": 2, "width": COLS, "height": ROWS, "timestamp": 0}
with open(out, "w") as f:
    f.write(json.dumps(header) + "\n")
    for r in rows:
        f.write(json.dumps(r) + "\n")
print(out)
