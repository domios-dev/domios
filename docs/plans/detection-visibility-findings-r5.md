# Detection visibility — round 5: R9 latency gate, opencode safety-net isolation, /stop verified

Source of truth: `docs/plans/detection-rebuild-deepening-loop.md` (rev 5). Prior: r1–r4.
dev daemon `3f477246`; classifier WIP edits mtime 19:21 (R9 "structural idle recovery").
Rebuilt `render_timeline` against WIP, regenerated **all 46 casts** (`exit 0`) to
scratch; all numbers below are the CURRENT (R9) classifier.

**Protocol ack:** reporting at every pass/waiting point; WAITING_FOR_COORDINATOR when
blocked; separating API/data-layer truth from DOM (DOM stays Windows-only).

## 1. Transition-latency PASS/FAIL gate — `testbench/transition-latency.py --gate`

```
python3 testbench/transition-latency.py <regen-dir> --gate
[PASS]    claude  transitions= 86  composer-anchor= 86  safety-net= 0  sem_med=10ms sem_max=73ms
[PASS]     codex  transitions= 55  composer-anchor= 55  safety-net= 0  sem_med=44ms sem_max=72ms
[PASS]    cursor  transitions= 44  composer-anchor= 44  safety-net= 0  sem_med=27ms sem_max=69ms
[FAIL]  opencode  transitions= 19  composer-anchor= 15  safety-net= 4  sem_med=43ms sem_max=5070ms
        safety-net: t=13200ms  semIdle=5034  turn1  opencode__idle_focus_churn_v1-20260621-idle-matrix-v1-opencode
        safety-net: t=42825ms  semIdle=5044  turn2  opencode__idle_focus_churn_v1-20260621-idle-matrix-v1-opencode
        safety-net: t=214200ms semIdle=5012  turn1  opencode__systems_design_long_chat_idle_v1-20260621-systems-matrix-v1
        safety-net: t=234675ms semIdle=5070  turn2  opencode__systems_design_long_chat_idle_v1-20260621-systems-matrix-v1
GATE: RED  (PASS = working->idle via structural composer anchor, 0 safety-net fallbacks)
```

PASS = a provider's working→idle transitions ALL resolve via a structural composer
anchor (`ready_prompt` glyph or `composer_ready` cursor geometry), never the 5000ms
stuck-Working safety-net.

### R9 vs R4 (the win)
| provider | R4 safety-net | R9 safety-net | R9 sem_med |
|---|---|---|---|
| claude | 0 | 0 | 10ms |
| codex | 0 | 0 | 44ms |
| cursor | **12/12** | **0/44** | 27ms |
| opencode | **12/12** | **4/19** | 43ms |

R9's `composer_ready` (cursor-geometry) **fully fixed cursor** (was 100% safety-net,
now 0) and **mostly fixed opencode** (4 of 19 remain).

## 2. Remaining opencode safety-net — isolated to non-alt-screen inline composer

All 4 remaining opencode safety-net frames share `alt_screen=False`, cursor row 34/40.
Exported frame bundle (bottom screen rows) shows the cause structurally:

```
  ┃
  ┃  Build · DeepSeek V4 Flash Free OpenCode Zen
  ╹▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀…
                                                9.6K (5%
```

opencode's **non-alt-screen inline composer** is a `┃`-bordered box with a `╹▀▀▀`
bottom rule and **no `›`/`❯` glyph on the cursor line**. So:
- `ready_prompt()` misses — there is no composer glyph to match.
- `composer_cursor_ready()` misses — it requires `alt_screen=true` (this is false here).
→ falls through to the 5000ms safety-net.

(When opencode IS in alt-screen, its composer shows `›` → `ready_prompt` matches → idles
in ~10–60ms. The gap is purely the inline/non-alt-screen mode, seen in
`idle_focus_churn` + `systems_design` opencode replays.)

**Actionable for Lead (structural, no prose):** recognize the opencode inline composer
box — e.g. relax `composer_cursor_ready` to also accept cursor parked in the bottom
region of a `┃`-bordered composer box when `alt_screen=false`, or add a structural
anchor for the `╹▀▀▀` bottom-rule + `┃` side borders. Re-run `--gate` → target opencode
safety-net = 0.

Bundle: `scratchpad/opencode_safetynet_bundle/` (4 frames, full decset+cursor+quiescence).

## 3. /stop ACTUALLY interrupts (not just state flip) — `testbench/stop-verify.sh`

Lead's fallback patch landed: `stop_pane` now sends ESC (`\x1b`) for every provider +
`mark_agent_interrupted` (force-idle + 1500ms suppression). The code comment names the
risk: "the state flip alone is a 1500ms illusion." The oracle measures BOTH axes —
state settle AND PTY output growth in a late window (3500→6000ms, well past suppression).

Disposable dev ws `fd926d6a` (claude/codex/opencode spawned, driven to Working, closed after):

| provider | mode | state path | late-window growth (3500→6000ms) | verdict |
|---|---|---|---|---|
| claude | /stop | idle ≤500ms | +0B | CLEAN INTERRUPT |
| codex | /stop | idle ≤500ms | +0B | CLEAN INTERRUPT |
| opencode | /stop | idle ≤500ms | +0B | CLEAN INTERRUPT |
| claude | manual Esc | idle ≤500ms | +0B | CLEAN INTERRUPT |
| codex | manual Esc | idle ≤500ms | +0B | CLEAN INTERRUPT |
| opencode | manual Esc | **working until ~6000ms** | +0B | **HALTED, STATE LAGGED** |

- **/stop genuinely halts generation on all three** (late-window +0B) AND state stays
  idle past the 1500ms suppression — the illusion is resolved by the fallback ESC.
- **Manual Esc**: generation halts immediately on all three, but **opencode's STATE
  lags ~5s** (no force-idle on the manual path → classifier waits for safety-net) — the
  same §2 inline-composer gap, now shown to cost ~5s of phantom "Working" in the UI
  after the user already interrupted. Fixing §2 fixes this too.

## 4. repaint_proof_v1 doctor status — PASS
Synthetic spinner-repaint cast (`testbench/replays/synthetic/repaint_proof_v1`):
Working only during the spinner status line (250–3000ms), settles Idle by ~3.5–5s,
**0 flicker blips**, ends idle. Committed timeline (sample_ms=250) and a fresh R9 render
(sample_ms=75) agree time-aligned. Minor metadata drift only (sample_ms), classification current.

## 5. Staleness
All committed `testbench/replays/**/timeline.json` predate the 19:21 R9 edit → STALE;
I measured against fresh scratch renders. Committed gate runs need
`testbench/acceptance-suite.sh --regen` first.

## Commands (exact)
```bash
# rebuild + regenerate against WIP classifier
(cd src-tauri && cargo build --example render_timeline)
for c in $(find testbench/replays -name raw.cast); do d=$(dirname "$c"); \
  src-tauri/target/debug/examples/render_timeline "$c" "<scratch>/$(echo $d|sed 's#/#__#g').json" \
  $([ -f "$d/meta.json" ]&&echo --meta "$d/meta.json") $([ -f "$d/phases.json" ]&&echo --phases "$d/phases.json"); done
python3 testbench/transition-latency.py <scratch> --gate          # PASS/FAIL + safety-net list
python3 testbench/transition-latency.py <scratch> --json          # raw rows
testbench/stop-verify.sh --pane <disposable> --mode stop|esc      # interrupt vs state-flip
```

## Tools added/updated
- `testbench/transition-latency.py` — added `--gate` (PASS/FAIL + per-provider safety-net list).
- `testbench/stop-verify.sh` — dual oracle: generation-halt (late-window bytes) vs state-flip; CLEAN INTERRUPT / HALTED-STATE-LAGGED / FALSE-IDLE verdicts.

<!-- revision: 1 -->
