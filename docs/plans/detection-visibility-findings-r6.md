# Detection visibility — round 6: non-vacuous /stop oracle (Adversary follow-up)

Source of truth: `docs/plans/detection-rebuild-deepening-loop.md` (rev 5). Prior: r1–r5.
Addresses the Adversary "stop verification vacuous" critique.

## Why v1 was vacuous, and the fix
v1 (`stop-verify.sh`) judged halt by output-byte delta in a fixed window. It read
"+0B = halted" even when the agent had simply **finished the short essay before /stop
landed** — there was nothing to interrupt. Non-vacuity requires proving the turn was
STILL generating at the moment of /stop.

`testbench/stop-oracle.sh` fixes this with:
1. **Long prompt** (default ~2000-word essay) so the turn is still streaming at /stop.
2. **Pre-stop baseline guard** — samples signals twice while working; if nothing is
   changing, exits `VACUOUS (retest)` instead of reporting a pass.
3. **Four signals**, gated on the strongest:
   - raw monotonic output length (`/output?full=true`) — PRIMARY; grows with real
     output, not fooled by in-place repaint (which leaves length equal but changes the
     screen hash).
   - rendered screen hash (md5 of strip_ansi screen) — catches repaint/settling.
   - `agent_state.state` — must stay idle, not revert to working.
   - assistant transcript length (`/chat`) — WEAK: only opencode populates it, and only
     at turn END (lags streaming); reported, not gating.
4. Verdict over the window 2000–6000ms post-stop (well past the 1500ms force-idle
   suppression): TRULY HALTED / NOT-INTERRUPTED-STILL-GENERATING / REMAINS-QUESTION / VACUOUS.

## Results (disposable dev panes, spawned + driven + cleaned up)

| provider | pre-stop baseline | raw growth 2–6s post-stop | state | verdict |
|---|---|---|---|---|
| claude   | changing=YES | **+0B** | idle stable | **TRULY HALTED** |
| codex    | changing=YES | **+0B** | idle stable | **TRULY HALTED** |
| opencode (run2) | changing=YES | **+108413B** | reverted to working | **NOT INTERRUPTED — STILL GENERATING** |
| opencode (run3) | changing=YES | still growing | reverted to working | **NOT INTERRUPTED** |
| opencode (run1) | changing=NO | n/a | — | VACUOUS (turn already done) |

### Headline
- **claude & codex `/stop` truly halt generation** — non-vacuous (baseline changing),
  output frozen +0B for 4s past suppression, state stays idle.
- **opencode `/stop` does NOT reliably interrupt active generation.** In 2/2 non-vacuous
  trials, output kept growing (+108KB) and state correctly reverted to working after the
  1500ms force-idle. The fallback ESC (`\x1b` via `write_raw`) halts claude/codex but not
  opencode's live turn. Earlier "halted" opencode results were the vacuous case (short
  turn already finished). The on-screen `■■⬝⬝ esc interrupt` meter kept advancing.

**Actionable for Lead:** opencode needs a working interrupt path — raw ESC via
`write_raw` does not stop its active generation. (Likely related to the same opencode
TUI input-mode quirk behind the alt_screen=false composer gap.) The state machine is
HONEST here (it reverts to working because the agent really is working) — the bug is the
INTERRUPT not landing, not the classifier.

## Carried / pending
- **opencode safety-net bundle** kept for Lead: `scratchpad/opencode_safetynet_bundle/`
  (4 frames, alt_screen=false inline composer, `┃` box + `╹▀▀▀` rule, no `›` glyph).
- **After Lead fixes opencode primary-screen / composer**: rerun
  `transition-latency.py --gate` → expect opencode safety-net 0/19 (currently 4/19).
  Before/after safety-net counts are the acceptance evidence. (WAITING on that fix.)
- **Codex ordering**: data layer ordered (`chat-order-gate.py` GREEN). Remaining gap is
  frontend-only — the optimistic-merge render path + a jsdom unit / Windows Playwright DOM
  order surface. Not measurable from macOS API; stays a frontend/Lead item.

## Commands (exact)
```bash
testbench/stop-oracle.sh --pane <disposable> --mode stop      # non-vacuous halt oracle
testbench/stop-oracle.sh --pane <disposable> --mode esc       # manual-Esc path
python3 testbench/transition-latency.py <regen-dir> --gate    # safety-net PASS/FAIL
```

## Tools added/updated
- `testbench/stop-oracle.sh` — non-vacuous /stop oracle (raw-growth-gated, pre-stop
  baseline, vacuous-run guard, 4 signals, settling-aware window).

<!-- revision: 1 -->
