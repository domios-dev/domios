# Detection visibility — round 7: strobe metric + opencode ambiguity bundle

Source of truth: `docs/plans/detection-rebuild-deepening-loop.md` (rev 5). Prior: r1–r6.
Adversary follow-up: the opencode "strobe" trap.

## Strobe metric — `testbench/strobe-metric.py`
A **strobe** = a state segment shorter than `WORKING_GRACE_MS` (1500ms) sandwiched
between two segments of the opposite state (working→idle→working, etc.) — visible UI
flicker. The tool also exports **ambiguous frames**: no `status_line`, cursor visible in
the bottom region (`composer_cursor_ready` band) — the frames whose verdict is decided by
timers, not structure.

```
python3 testbench/strobe-metric.py <regen-dir> --glob 'opencode__*.json' --bundle <out>
```

### Results (R9 regen)
| replay | strobes | ambiguous frames |
|---|---|---|
| opencode systems_design | **41** (working↔question, 0–675ms blips) | 660 |
| opencode idle-matrix | 2 | **737** |
| opencode idle-focus-rest | 1 (working 1200ms in idle) | 0 |
| claude idle-focus (×2) | 1 | 72 |
| codex idle-focus (×2) | 3 | 411 |
| cursor idle-focus (×2) | 0 | 411 |

## Root cause (bundled): structurally-identical frames classified both ways
The 737 ambiguous idle-matrix frames are ALL opencode's **non-alt-screen inline
composer**: `alt_screen=false`, cursor row 34/40, no `status_line`, no active footer,
no `›`/`❯` glyph — just `┃` box borders + a `╹▀▀▀` bottom rule + the model footer.

They split **(working, alt=false): 160 / (idle, alt=false): 577** — i.e. the SAME screen
is classified both ways. With no structural signal, the verdict falls to the quiescence
timers (grace / 5000ms safety-net), so the same composer screen flips working↔idle as the
clocks cross → strobe. Same root cause as the 4 safety-net transition frames (r5).

## Fix direction for Lead (structural, no prose)
Recognize the opencode inline composer box as a ready composer:
- relax `composer_cursor_ready` to accept `alt_screen=false` when the cursor sits in the
  bottom region of a `┃`-bordered box with a `╹▀▀▀` bottom rule, or add a structural
  anchor for that signature.

→ the 737 frames resolve to one deterministic Idle: strobes→0, safety-net→0.

## Acceptance after Lead's fix (before/after evidence I will run)
1. `strobe-metric.py … --glob 'opencode__*'` → strobes = 0 (was 44 total across opencode).
2. `transition-latency.py … --gate` → opencode safety-net 4/19 → 0/19.
3. `stop-oracle.sh --pane <disposable> --mode stop` → opencode TRULY HALTED (non-vacuous).
4. idle-matrix ambiguous frames collapse to a single Idle run.

## Bundle (persisted for Lead — scratchpad is session-isolated)
`testbench/diagnostics/opencode_ambiguity/` — README + `*.ambiguous.txt` (≤60 frame
screens each) + the 4 safety-net frame dumps (`_f177/_f572/_f2857/_f3130`).

## Tools added/updated
- `testbench/strobe-metric.py` — strobe detection (sub-grace flicker) + ambiguous-frame
  bundle export.

<!-- revision: 1 -->
