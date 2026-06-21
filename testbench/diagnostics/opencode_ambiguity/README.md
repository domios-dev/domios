# OpenCode classifier-ambiguity bundle (for Lead)

Evidence for the opencode strobe + safety-net traps. All frames here are opencode in
its **non-alt-screen inline composer** (`alt_screen=false`), cursor parked in the
bottom region (row 34/40), with **no `status_line`, no active-turn footer, and no
`›`/`❯` composer glyph** — just the box-drawing composer:

```
  ┃
  ┃  Build · DeepSeek V4 Flash Free OpenCode Zen
  ╹▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀…
                                                9.6K (5%)  ctrl+p
```

## The trap (measured)
In `opencode__idle_focus_churn ...idle-matrix` (943 frames) there are **737 such
ambiguous frames**, classified BOTH ways with NO structural difference:
- (working, alt=false): 160
- (idle, alt=false):    577

Because no structural signal fires, the verdict is decided purely by the quiescence
timers (grace / 5000ms safety-net) → the SAME screen flips working↔idle and the
classifier **strobes**. `systems_design opencode` shows 41 strobe segments (<1500ms
WORKING_GRACE), idle-matrix shows the 737-frame ambiguity + 5000ms safety-net idles.

## Fix direction (structural, no prose)
Recognize the opencode inline composer as a ready composer:
- relax `composer_cursor_ready` to accept `alt_screen=false` when the cursor is in the
  bottom region of a `┃`-bordered box with a `╹▀▀▀` bottom rule, OR
- add a structural anchor for that box signature.
Then the 737 frames resolve to a single deterministic Idle → strobes → 0, safety-net → 0.

## Regenerate / validate
```bash
# regenerate timelines against current classifier, then:
python3 testbench/strobe-metric.py <regen-dir> --glob 'opencode__*.json' --bundle <out>
python3 testbench/transition-latency.py <regen-dir> --gate     # opencode safety-net count
```
Acceptance after fix: opencode strobes=0, safety-net=0/N, idle-matrix ambiguous frames
resolve to a single Idle run.

Files: `*.ambiguous.txt` = up to 60 ambiguous-frame screens each; `*_f####.txt` = the
4 specific safety-net transition frames (f177/f572 idle-matrix, f2857/f3130 systems).
