# Detection deepening loop — round log

Loop Lead / fixer: tab `e6288f83`. Source of truth: `docs/plans/detection-rebuild-deepening-loop.md`
("Live loop runbook — 2026-06-21", rev 5). Baseline commit `3f477246`.

Measurement tool: `testbench/detect-rounds/seg.py <timeline.json>` — segments a replay's
per-frame production-classifier output (`frames[].detector.state`) into state runs, reports
counts, final state, sub-400ms blips (flicker), and last-question end.

---

## Round 1 — establish measurement + baseline; triage the 3 live P0s

### A. Defect picked
Live P0s from Andrey: (1) Codex tab `2bace0d6`/pty `84f81602` visually idle but `/api/tabs`
`agent_state={state:"working"}`; (3) OpenCode `40c87bb3` stayed Working after a normal answer.
No per-frame state measurement existed, so the measurement was built first.

### B. Expected vs observed
- Expected: idle CLI ⇒ `agent_state=idle`; completed normal answer ⇒ idle.
- Observed (build **3c722440**, one commit BEHIND HEAD `3f477246`): dev log
  `~/.codeterm-dev/logs/codeterm.log` shows an **oscillation** driven by the OLD byte-timer in
  `service::output`:
  ```
  14:45:39 [status]    84f81602 Idle -> Working      (byte activity)
  14:45:43 [idle-poll] 84f81602 Idle                 (background poll corrects)
  14:45:52 [status]    84f81602 Idle -> Working      (flips back on len=16 repaint)
  14:46:02 [idle-poll] 84f81602 Idle
  ```
  Codex's idle TUI emits ~len=16 repaints every ~10s; the byte-timer reads any byte as Working,
  the idle-poll corrects it. The API was sampled mid-oscillation ⇒ "stuck Working".

### C. Measurement / counts (HEAD `3f477246`, production classifier replayed per-frame)
`provider_cli_question_probe_v1`, sampled ~13–15fps:

| Provider | frames | idle | working | question | FINAL | blips <400ms |
|----------|-------:|-----:|--------:|---------:|-------|-------------:|
| codex    | 1087   | 621  | 263     | 203      | idle (14325ms) | 1 (225ms idle, q→w gap) |
| opencode | 885    | 521  | 135     | 229      | idle (14325ms) | 0 |
| cursor   | 1047   | 594  | 217     | 236      | idle (14475ms) | 0 |

All three return to **idle** at the end; question state is detected for all three.

Live re-measure after restart to HEAD: `40c87bb3` opencode = **idle**, `5bd7268c` claude = idle,
`2bace0d6` codex = **null** (NOT working). ⇒ the live stuck-Working was the stale 3c72 byte-timer;
HEAD's `classify_state` does not reproduce it for the idle case.

### D. Root cause of the LATENT risk (still present on HEAD)
`offline_state_classifier::classify_state` (line 199) returns **`previous`** when nothing matches.
The only non-grace path OUT of Working is `ready_prompt()` (line 185), which does
`screen_lines.iter().rev().take(4)` looking for a `❯`/`›` prefix. `screen_state::lines_and_tail`
returns the FULL grid with **trailing blank rows kept** as empty strings. So if a provider's TUI
leaves ≥1 blank padding row below its `›` composer, the composer line falls outside the last-4
window ⇒ `ready_prompt`=false ⇒ fall through to sticky `previous`=Working ⇒ **stuck Working**.
Layout-fragile, provider-dependent. Captured codex idle screen
(`round-01-evidence/codex-2bace0d6-idle.screen.txt`) matches the `›` line at exactly rev index 3 —
one extra blank row would break it.

### E. Open gaps for next rounds
- The probe replay is NOT Andrey's real "A/B/C choice" prompt. Must capture real-prompt arcs
  (codex prose-answer; opencode interactive menu) and measure final state (Round 2).
- `state=null` on freshly restored agent tabs is a visibility gap (Round: task #5).
- Codex ChatUI ordering: transcript data IS seq/timestamp-ordered correctly (verified via
  `tab chat --json`), so the bug is in the render layer (`TabSessionChat.tsx`), not detection.

### Status: measurement established, baseline captured, no fix yet (correct — bug not yet
reproduced on HEAD with real prompts). Proceeding to Round 2 (real-prompt corpus).

---

## Round 2 — real-prompt corpus (Dickens/Verne/Twain) confirms/denies stuck-Working on HEAD

### A. Defect picked
Confirm or deny the live stuck-Working / missing-Question P0s on HEAD using Andrey's REAL prompt
(mined from the live tabs), not the synthetic probe (which forces a CLI UI).

### B. Real prompt (generator only, never hardcoded)
All three tabs used the same casual Russian prompt:
`"а ты можешь мне послать допустим в виде опросника A B C варианты как ты умеешь внутри[интерфейса]"`
Scenario: `testbench/detect-scenarios/real_abc_poll_v1.yaml` (3 turns + acks + idle holds + focus
churn). Captured via `testbench/replay-capture.py` (dev 7686), rendered with `render_timeline`.

### C. Per-frame counts (production classifier, HEAD 3f477246)

| Provider | replay dir (`testbench/replays/<p>/real_abc_poll_v1-round2-real-abc-<p>`) | frames | idle | work | q | FINAL | card | picker_active |
|----------|------|------:|----:|----:|--:|-------|-----:|--------------:|
| codex    | …-codex    | 1062 | 700 | 155 | 207 | **idle** (13.9s) | 0 | 0 |
| opencode | …-opencode | 1241 | 569 | 439 | 233 | **idle** (14.5s) | 0 | 0 |
| cursor   | …-cursor   | 1542 | 634 | 671 | 237 | **idle** (14.6s) | 0 | 0 |

Provider surface divergence observed (the realistic test):
- **codex** answered in PROSE (`A. … B. … C. …`), no menu → returns to idle. (the stuck-Working case)
- **opencode/cursor** rendered REAL interactive pickers → Question, then returns to idle. Verified
  structural: opencode question frame has `decset.alt_screen+cursor_hidden=true`, numbered `1./2.`,
  footer `↑↓ select  enter submit  esc dismiss` (not prose).

### D. Conclusion
On HEAD, with Andrey's real prompts, **neither codex (prose) nor opencode/cursor (real menu) gets
stuck Working — all return to idle; zero false cards.** ⇒ the live stuck-Working Andrey saw was the
stale dev build `3c722440` byte-timer (Round 1 log evidence), not HEAD.

### E. Latent bug PROVEN at unit level (TDD characterization, fails on HEAD)
`offline_state_classifier::tests`:
- `idle_composer_with_trailing_blank_padding_returns_idle` → FAILS (`left: Working, right: Idle`).
- `long_quiescent_unknown_layout_does_not_stay_working_forever` → FAILS (same).
Root cause: `ready_prompt` does `.rev().take(4)` over RAW rows incl. trailing blanks → composer
glyph can fall outside the window; `classify_state` line 199 then returns sticky `previous`=Working.
This is the latent version of the live bug and IS reproducible on HEAD. → Round 3 fix.

### Status: Round 2 PASS (P0 stuck-Working not reproduced on HEAD with real prompts; corpus added).
Latent fragility proven by 2 failing unit tests. Next: Round 3 structural fix + before/after gates.

> CORRECTION (Round 3): Round 2's "PASS" was measured by `render_timeline.rs`'s OWN memoryless
> `working = status_line.is_some()` + hold-decay overlay — NOT the production `classify_state`.
> The Adversary (P0-1) was right. Round 3 unifies the oracle; the real production numbers below
> SUPERSEDE Round 2's. Round 2's real-prompt CAPTURES remain valid corpus; only its verdicts were wrong.

---

## Round 3 — unify the replay oracle to production `classify_state`; fix stuck-Working

### A. Defect (Adversary P0-1)
`examples/render_timeline.rs::detector_json` computed `detector.state` itself (status-line present ⇒
working, hold-decay `WORKING_HOLD_MS`, no threaded `previous`) — a parallel, MEMORYLESS detector.
Since stuck-Working is a STATEFUL bug (sticky `previous`), the old oracle structurally could not
reproduce it. Any "replay PASS" was vacuous.

### B. Fix 1 — oracle unification (the measurement)
Exposed `classify_signals` (offline adapter: builds `ClassifyInput` from the replayed
`SignalExtractor` + `detect_picker`, delegates to `classify_state`). `render_timeline` now threads
`overlay.previous: TabVerdict` frame-to-frame through `classify_signals` — the SAME stateful
`classify_state` the live `service::output::recompute_agent_state` runs. Removed the hold-decay.
status_line/summary kept as display-only, surfaced per verdict state (mirrors output.rs).

### C. Fix 2 — structural idle resolution in `classify_state`
1. `ready_prompt`: scan last 4 NON-EMPTY rows (was `.rev().take(4)` over raw rows incl. trailing
   blanks) → recognizes codex's `›` composer regardless of padding.
2. Stuck-Working safety net: `previous==Working` + BOTH clocks `>= STUCK_WORKING_IDLE_MS (5000)`
   + no active-work/question/ready-prompt evidence ⇒ Idle (bounded; byte-silence requirement means
   an animating spinner never trips it). Unit tests fail-before/pass-after:
   `idle_composer_with_trailing_blank_padding_returns_idle`, `long_quiescent_unknown_layout_*`.

### D. Before/after through the UNIFIED oracle (the real gate) — exact counts
Renderer: `src-tauri/target/debug/examples/render_timeline`; replays under `testbench/replays/<p>/`.
`tailW` = trailing working-run reaching the final frame (stuck-Working signature).

| replay | BEFORE fix | AFTER fix |
|--------|-----------|-----------|
| opencode probe   | FINAL=**working** tailW=478  | FINAL=idle tailW=0 |
| opencode real    | FINAL=**working** tailW=586  | FINAL=idle tailW=0 |
| opencode idle_churn ×2 | FINAL=**working** tailW=447/858 | FINAL=idle tailW=0 |
| cursor idle_churn ×2   | FINAL=**working** tailW=1721/1589 | FINAL=idle tailW=0 |
| codex real       | FINAL=**question** (sticky)  | FINAL=idle |
| codex idle_churn ×2 | FINAL=**question** (sticky) | FINAL=idle |
| codex probe      | FINAL=idle                   | FINAL=idle |
| cursor probe/real| FINAL=question               | FINAL=question (UNCHANGED) |

⇒ The unified oracle REPRODUCES the opencode/cursor stuck-Working Andrey reported (FINAL=working,
tails up to 1721 frames). The fix resolves 6 stuck-state replays. All 310 `agent::` unit tests pass;
`build.sh check` clean.

### E. NEW bugs surfaced by the unified oracle (not yet fixed → next rounds)
1. **cursor false/stuck Question** (cursor probe FINAL=question 850/1047 q; cursor real 1115/1542).
   Root cause: `detect_picker` keeps firing on cursor's ANSWERED `AskQuestion Пример опроса` poll
   left rendered on screen — `--native-picker` shows the payload present at the FINAL frame
   t=115575. `classify_state` step 1 (`picker_question`) → Question forever. Fix candidate: gate the
   picker question on `signals.picker_active` (live interactive mode) instead of stale grid match.
2. **cursor idle_churn working-FP** (work=768/936 frames during focus churn) — FINAL=idle now, but
   the idle-HOLD windows still show working; cursor composer not ready_prompt-recognized + churn
   resets byte clock before the 5s net. Negative-corpus gate not yet clean for cursor.
3. **codex near-zero working** (codex real work=2/1062) — known "missing short work" gap; codex
   prose has no persistent status widget for `extract_status_line` and offline motion is Still
   between chunks.

### Status: oracle unified (P0-1 resolved), stuck-Working fixed with replay+unit before/after.
Round 2 verdicts superseded by real numbers. Next: cursor false-Question (detect_picker liveness).

---

## Round 4 — cursor false/stuck Question: oracle-fidelity fix (gate offline picker on `picker_active`)

### A. Defect + root cause (diagnosis)
Round 3's unified oracle showed cursor probe/real FINAL=question (850/1115 q frames), stuck forever.
Root cause is an ORACLE-FIDELITY divergence, NOT a live bug:
- LIVE `recompute_agent_state` takes `picker_question` ONLY from a STRUCTURED handle
  (`output.rs:299-301` hard-restriction; native `detect_picker` is explicitly NOT a fallback).
  Cursor is PTY with no structured prompt ⇒ live `picker_question` is always None.
- OFFLINE `classify_signals` used `detect_picker(grid)` as the stand-in. Cursor's inline
  "AskQuestion" poll stays rendered after it is answered, so `detect_picker` kept matching
  (`--native-picker` payload present at final frame t=115575) ⇒ Question forever.
- Evidence: `signals.picker_active` is **false on every frame** of all 6 captures (cursor/codex/
  opencode) — cursor's poll is inline text+selection, NOT a DECSET interactive picker.

### B. Fix (offline adapter only — does NOT touch the live path)
`classify_signals`: `picker = signals.picker_active.then(|| detect_picker(...)).flatten()`. The
offline picker question now fires ONLY while a picker is structurally ACTIVE (DECSET/alt-screen),
mirroring live's "structured handle only" rule. Real model/permission pickers set `picker_active`
so they are preserved; stale rendered options are ignored.

### C. Before/after (unified oracle), exact counts

| replay | BEFORE gate (R3) | AFTER gate (R4) |
|--------|------------------|-----------------|
| cursor probe | FINAL=**question** q=850/1047 | FINAL=idle q=256 (bounded to active poll) |
| cursor real  | FINAL=**question** q=1115/1542 | FINAL=idle q=256, `last question ends 51000` (clears) |
| opencode probe | q=250 FINAL=idle | q=249 FINAL=idle (PRESERVED) |
| opencode real  | q=254 FINAL=idle | q=254 FINAL=idle (PRESERVED) |
| codex probe | q=402 FINAL=idle | q=203 FINAL=idle (startup menu still Question `75..15225ms`) |

All 14 replays now FINAL=idle, card=0, tailW=0 (no stuck Working, no stuck Question). Cursor question
is bounded to the active-poll window and clears. codex startup onboarding menu still classifies
Question via the structural `classify_terminal_menu_question` (not detect_picker). 310 `agent::`
tests pass; `build.sh check` clean; 6 classifier unit tests green.

### D. Remaining (next rounds)
- cursor idle_churn working-FP (work=768/937 during focus churn) — FINAL=idle but idle-HOLD windows
  still show working; negative-corpus gate not yet clean for cursor.
- codex near-zero working (work=2/1062 real) — known "missing short work" gap.
- Codex ChatUI same-second ordering (chat-core mergeById timestamp-first sort) — coordinator owns
  ChatUiDefaultPrompt; ordering test still to add against packages/chat-core.
- Live ChatUI/DOM acceptance pass for the fixed states.

### Status: cursor stuck-Question resolved as an oracle-fidelity fix; offline oracle now matches
the live structured-only picker rule. Replay corpus fully FINAL=idle/card=0/tailW=0.

---

## Round 5 — cursor idle_focus_churn "Working FP": NOT a bug (anti-circular correction + gate)

### A. Defect picked (coordinator priority 1)
"cursor idle_focus_churn Working FP during focus churn (768/937 frames)" — from the R3 remaining list.

### B. Measurement vs the EXPECTED phase lane (not assumption)
`phases.json` for `cursor/...idle-focus-v2...100x30` has 3 turns each followed by an `idle_hold`:
expected idle-holds = **37559..57559, 81292..106292, 114579..144579**. Between them are REAL work
turns (the scenario sends prompts: "Create a TODO/checklist… three bullets…", etc.). At the
detector's "working" frames the screen shows `⠰⠰ Composing` + `ctrl+c to stop` — cursor genuinely
composing. So the 768 working frames are REAL WORK, not a focus-churn FP. R3's count conflated work
turns with idle holds.

### C. Idle-hold gate (new permanent artifact): `testbench/detect-rounds/idle_hold_gate.py`
Compares detector vs the expected `idle_hold` events: each hold BODY (hold_start+3000ms .. end)
must be working=0 AND question=0; a ≤3s settle tail right after the turn is tolerated (the work
widget can linger past the scenario's byte-quiet marker). Result across ALL 4 gating fixtures:

| provider | hold bodies | settle-tail working (per hold) |
|----------|-------------|--------------------------------|
| claude   | working=0 question=0 ✓ | 0,0,0 |
| codex    | working=0 question=0 ✓ | 0,0,0 |
| cursor   | working=0 question=0 ✓ | 20,17,25 (range 37575..39000 etc — ≤1.9s, turn-end) |
| opencode | working=0 question=0 ✓ | 25,24,24 (≤1.9s) |

`idle_hold_gate.py` exit=0 (all bodies clean). Working during holds = ONLY the bounded ≤1.9s
settle tail at each turn→hold boundary (real `Composing`/spinner still on screen), never spread
through the focus-churn body.

### D. Conclusion / decision
No detector bug: blinking cursor / focus churn does NOT produce Working on any provider. The settle
tail is correct (the work widget is genuinely on screen for ~1.5s past the scenario marker);
tightening it would risk dropping real work early (flicker), so NO change made. R3's "768-frame FP"
is retracted. The gate is now a permanent negative-corpus guard.

### Status: priority 1 resolved as not-a-bug, with exact frame ranges + a permanent idle-hold gate.
No code change to the detector (correct). Next: codex missing-short-work (coordinator priority 2).

---

## Round 6 — codex missing-short-work: measure user-visibility (coordinator priority 2)

### A. Defect
"codex near-zero working (work=2/1062 in real replay)" from R3 remaining — is codex's working
indicator missing during generation (user sees idle while codex works)?

### B. Measurement vs phase lane (codex real replay)
`phases.json` inputs: turn1 t=8690, turn2 t=34972, turn3 t=61339 ("круто"). Per-turn fine runs:
- turn1 8690..: dominated by `question` (92f) — this window is the codex STARTUP TRUST DIALOG
  ("Do you trust the contents of this directory? 1.Yes 2.No"), correctly Question, codex blocked
  (not generating). Only 2 working frames because there was no generation here.
- turn2 34972..: **working 35550..40200 = solid 4650ms block** (clean, no flicker).
- turn3 61339.. ("круто" short ack): **working 61950..64125 = solid 2175ms block**.

The `work=2/1062` aggregate was misleading — dominated by the startup dialog + the three long idle
holds. During ACTUAL generation, codex working is detected as solid multi-second blocks.

### C. Root signal
Codex renders `• Working (Ns • esc to interrupt)` while generating. `active_turn_footer` matches it
(`has_interrupt_hint && parse_elapsed_ms.is_some()`), and `extract_status_line` surfaces the verbatim
line (`Working (0s..4s • esc to interrupt)` observed). Same path runs live → live codex shows working.

### D. Conclusion / decision
codex missing-short-work does NOT reproduce in the real-prompt corpus — working is detected solidly
(2.2–4.65s blocks) via a structural widget. The documented gap is the narrow "very short FIRST turn
after a long silent startup" edge (sub-second generation), which (a) is not user-visible in normal
turns and (b) whose intended fix (submit-anchored pending-response state) carries a stuck-Working
risk the coordinator explicitly deprioritizes. NO change made. If Andrey wants the sub-second edge
quantified, capture a targeted "startup → 1-word first turn" scenario; otherwise not worth the risk.

### Status: priority 2 measured — not a user-visible bug in the corpus; no fix (avoids stuck-Working
risk). Next: codex ChatUI same-second ordering test (priority 3), Stop/Cancel/Esc→Idle (priority 4).

---

## Round 7 — codex ChatUI ordering: rule out the sort comparators (priority 3)

### A. Defect
"Codex assistant messages appear above/out of order relative to user questions."

### B. Data-layer evidence (real codex rollout `~/.codex/sessions/.../rollout-…019ee952….jsonl`)
Role+timestamp pairs are CORRECTLY ordered with ms precision (NOT second-floored as merge.ts's
comment claims): `08:35:53.827 user "авава" → 08:35:57.692 assistant`; `08:42:05.097 user →
08:42:06.765 assistant` … user always precedes its reply by 1.5–3.8s.

### C. Comparator analysis (chat-core)
Both `mergeById` (merge.ts) and `mergeTimelineMessages` (timeline.ts) sort timestamp-first, seq, then
arrival. With ms-precision codex data + the optimistic echo (ts = submit `Date.now()`, always < the
assistant's generation ts on the same machine clock), the sort orders user-before-assistant. Existing
tests already cover this (`pending user stays in timeline before later assistant replies`,
`same-second timestamps tiebreak by seq`). Added a REAL-DATA regression
(`merge.test.ts`: "codex real ms-precision rollout…") fed shuffled → passes (7/7).

### D. Conclusion
The chat-core sort layer is SOUND and now regression-locked with verbatim rollout timestamps. The
"assistant above user" symptom is therefore a LIVE-RENDER / streaming-arrival transient, not a
comparator bug — candidates: `matchedOptimisticEchoes` GC'ing the optimistic user bubble in the gap
before the real user JSONL row is tailed (assistant briefly alone), or render-layer interleave in
`TabSessionChat`/the coordinator's `chatUiMode` path. Characterizing it needs a live ChatUI DOM
capture during a codex turn (WebView Playwright). Handing the live-render investigation to the
coordinator (owns chatUiMode/ChatUiDefaultPrompt); did NOT touch those files.

### Status: comparators ruled out + regression-locked (chat-core, my scope). Live-render ordering
needs DOM capture + coordination with the owner. Next: Stop/Cancel/Esc→Idle (priority 4).

---

## Round 8 — Stop/Cancel button must force Idle + clear pending (priority 4, invariant 5)

### A. Defect (code-traced)
`api/tab_routes::stop_tab` (the Stop/Cancel button) sent the provider stop keystroke via
`svc.write_raw(tab_id, …)`. `write_raw` writes straight to the PTY — it BYPASSES the manual-Esc
hook `mark_manual_agent_interrupt` (which only runs on `write_pty`/`write_pty_string_core` and only
when `bytes == b"\x1b"`). So the Stop button never called `mark_agent_interrupted` ⇒ it did NOT
force Idle or clear `pending_prompt`; it just sent the key and waited for the agent to react. Cursor
(stop keystroke = Ctrl+C `\x03`, not Esc) got no force-idle even via the manual hook.

### B. Fix
`stop_tab` now calls `state.svc.mark_agent_interrupted(&pty_id)` (resolved via
`resolve_pty_id_with_fallback`) before sending the keystroke — for ALL providers, keystroke or not.
`mark_agent_interrupted` = `force_agent_idle_for_tab` (sets `agent_state=Idle`, `pending_prompt=None`,
emits) + a 1500ms suppression window so trailing PTY output can't immediately re-flip the verdict.

### C. Live API smoke (dev 7686, build with fix)
Drove codex tab `2bace0d6` to a sustained turn (1500-word essay), tight-polled to `state=working`,
then `POST /api/tabs/2bace0d6/stop`:
```
BEFORE stop:  state=working pending=null
STOP sent ->  {"ok":true,"stopped":true}
AFTER  +0s:   state=idle pending=null
       +0.2s: state=idle pending=null
       +0.7s: state=idle pending=null
```
Working → Idle forced immediately on Stop. (Manual lone-Esc-in-terminal path unchanged and still
forces idle via `mark_manual_agent_interrupt`; this fix adds the same guarantee to the Stop button
and to Ctrl+C providers like cursor. `pending_prompt=None` is set by the same `force_agent_idle_for_tab`.)

### D. Follow-up
Integration/regression test for `stop_tab` (async axum handler) is a follow-up; the handler needs
`AppState`, so it's an http-level test, not a pure unit test. Live smoke is the evidence here.
Did NOT add ChatUI-surface DOM check (coordinator owns the ChatUI Stop button surface).

### Status: priority 4 (Stop→Idle) fixed for ALL providers + API-smoke verified. `build.sh check`
clean; dev restarted on the fixed build; prod untouched; nothing committed.

---

## Round 11 — transition-latency GREEN (all providers), opencode primary-screen, permission guard

### A. Transition-latency hard gate GREEN (authoritative `testbench/transition-latency.py --gate`)
After: reorder (composer_ready AFTER working-grace), `composer_cursor_ready` dropped the alt_screen
requirement (covers opencode primary-screen alt=false), horizontal-button veto.
`[PASS] claude 85/85 · codex 55/55 · cursor 44/44 · opencode 26/26 — safety-net=0`. opencode settles
via post-grace composer anchor (sem_med 1537ms), no longer the 5s timeout. (Gate exits nonzero on RED.)

### B. Reorder + guards (Adversary trap: identical work/idle cursor geometry)
classify_state order is now: picker → working → terminal-menu question → text question → **glyph
ready_prompt (pre-grace, vetoed by options block)** → **working-grace** → **composer_ready (post-grace,
vetoed by options block)** → stuck-net → previous. So composer_ready CANNOT pre-empt grace
(`composer_ready_does_not_preempt_working_grace` test) and a horizontal-button row vetoes both idle
paths (`composer_ready_vetoed_by_options_block`). Motion arm documented dead (both adapters Still).

### C. Permission guard (Visibility r8: synthetic permission_box_visible_cursor_v1 = 121/121 FALSE-IDLE)
A choice/permission box with a VISIBLE cursor + numbered options was missed by
`classify_terminal_menu_question` (requires cursor_hidden) → `composer_ready` false-idled it. Fix:
added `confirm_style_footer` (enter + esc + nav/choose key-hints) — numbered/checkbox options + that
footer classify Question regardless of cursor_hidden (fires at step 3, before composer_ready). Result:
synthetic `permission_box_visible_cursor_v1` 121 idle → **120 question / 1 idle**. Negatives held:
`numbered_prose_without_confirm_footer_is_not_question_visible_cursor` stays None. 14 classifier tests
pass; `build.sh check` clean. (Full-corpus regen verification in progress.)

### D. opencode interrupt + structured-prompt server — ROOT CAUSE found, ARCHITECTURAL (plan-first)
r6: fallback ESC does not halt opencode (+108k bytes after stop). Root cause (coordinator/Adversary):
only orchestrator allocates the structured-prompt server; UI/direct/restart/transport/general paths
pass `structured_prompt_server: None` ⇒ opencode plain TUI, no SSE picker_question, no HTTP abort.
Baseline `testbench/opencode-server-check.sh` (direct spawn): no --port / no LISTEN socket / no
control plane — all FAIL (the broken BEFORE). Scoped plan + C1–C8 in
`docs/plans/structured-prompt-server-centralization.md`. Broad/load-bearing (every spawn path incl.
restart) → **WAITING_FOR_COORDINATOR go/no-go** before editing the shared spawn path. Classifier
permission guard (C) stands as honest defense-in-depth meanwhile.

### Status: latency gate GREEN; permission false-idle fixed in classifier; opencode server-mode
centralization scoped + awaiting approval (architectural). Nothing committed.

---

## Round 12 — structured-prompt-server centralization (opencode out-of-the-box, C1-C9)

### A. Goal / root cause
Only the async `spawn_agent` (REST) allocated+subscribed the opencode structured server; all other
spawn paths passed `structured_prompt_server: None` → opencode via desktop/CLI = plain TUI (no SSE
picker_question, no HTTP abort). The CLI route specifically: `agent.spawn → spawn_agent_sync →
spawn_tab_with_ready → spawn_visible_agent_impl` (sync) bypassed the async block.

### B. Fix — ONE shared capability-gated helper
`structured_prompt::ensure_for_spawn(svc, agent_cmd, tab_id, cwd) -> Option<StructuredPromptServer>`
= gate(`needs_structured_prompt_server`) → `allocate_server` → `register_endpoint` →
`maybe_start_subscriber` → return (B-ORDER: server set into launch params BEFORE the command is
built). No provider-id branch; honest None on not-needed/alloc-fail. Wired into EVERY spawn path via
the one helper: orchestrator async `spawn_agent`, `PtyTransport::launch` (desktop/CLI),
`spawn_visible_agent_impl` + `spawn_hidden_agent_impl`, `start_general_agent`, and `agent_restart`
(C6 fresh-endpoint; some of these landed via the coordinator's concurrent patch — combined diff is
coherent). C5/C6 cleanup pre-existing via `clear_tab` (tab close/exit) so restart re-allocates fresh.

### C. Acceptance — `testbench/opencode-server-check.sh`
- REST Path-A: green (pre-existing).
- **CLI/Path-B (was the gap): now PASS all 4** — (1) `--hostname/--port` in process argv;
  (2) opencode LISTENING on the port; (3) control-plane reachable; (4) C2 daemon→opencode SSE
  ESTABLISHED. `build.sh check` clean. Confirmed opencode TUI with `--port` DOES bind+serve (server
  semantics fine; the gap was purely the missing flag on the sync path).
- C9 structured-first holds by construction: `recompute_agent_state` reads `picker_question` only
  from the structured `pending_prompt` (output.rs hard-restriction); with the SSE subscriber now live
  on all paths, opencode permission/question arrives structured-first, PTY parsing is fallback only.

### Status: opencode structured control plane is now out-of-the-box on every spawn path (single
helper, capability-gated, B-ORDER). Combined with Rounds 9/11 (latency GREEN, permission guard) the
detection rebuild's opencode track is structurally complete. Nothing committed.

---

## Round 13 — C9 live question E2E (deterministic opencode permission gate) — FULL PASS

### A. Gate (HEAD smoke, Option A): `testbench/opencode-permission-gate.sh`
Temp-cwd `opencode.json` with `permission:{bash:"ask"}` (deterministic trigger) → spawn opencode via
the centralized path → forceful exact-bash prompt (retry ×3 for model nondeterminism, honest-skip
if it answers in prose / no auth) → assert through the REAL SSE subscriber + recompute:
1. `permission.asked` → `pending_prompt` non-null + `/api/tabs` **state=question** — PASS.
2. `POST /api/tabs/{id}/prompt/respond {"value":"1"}` → `{ok:true}` → **question cleared** (working) — PASS.
3. opencode **proceeded** — the allowed `echo <marker>` ran, marker in the transcript — PASS.
`RESULT: C9 PASS`. Deterministic CI coverage for the event→prompt mapping stays in the
`opencode_sse` unit tests (`parse_event`/`reply_body`); this gate is the live wire they can't cover.

### B. Two bugs C9 surfaced + fixed
1. `respond_prompt` PANICKED ("Cannot drop a runtime in a context where blocking is not allowed") —
   `send_reply` uses `reqwest::blocking` on the async runtime. Fix (mine): `api/agent_routes.rs`
   wraps `svc.respond_prompt` in `tokio::task::spawn_blocking` (CLAUDE.md async discipline).
2. `reply_endpoint` permission route 404 — was `/api/session/{}/permission/{}/reply`; now
   `/session/{}/permissions/{}` (matches opencode SDK `postSessionIdPermissionsPermissionId`; no `/api`
   prefix, consistent with `/event` at root). Fixed by the coordinator concurrently.

### C. Open
- The QUESTION reply route (`/api/session/{}/question/{}/reply`) is untested here (gate exercised
  PERMISSION) — may need the same de-`/api`/pluralize treatment; flagged to the opencode-API owner.

### Status: opencode structured question/permission is answerable end-to-end (raise → Question →
structured respond → clear → proceed), live + deterministic. C1–C9 complete for opencode. Nothing committed.

---

## Round 9 — STRUCTURAL idle settle for cursor/opencode (Adversary R3/R4 blocker)

### A. Defect (Adversary, accepted)
R3's 5s stuck-Working safety net is a TIMEOUT, not structural: cursor/opencode (composers not
matched by `ready_prompt`) only settled via it ⇒ (i) up-to-5s visible false-Working window after
every turn, (ii) indefinite-stuck risk if idle repaints keep `byte_idle < 5000`.

### B. Per-transition idle-latency oracle (new): `testbench/detect-rounds/idle_latency_oracle.py`
Reports `byte_idle/semantic_idle` at every working→idle transition; FAILs if it only settles at the
~5000ms net. BEFORE (R3/R4 build): cursor real 5040/5055ms, cursor probe 5037/5008ms, opencode real
5000/5063ms, opencode probe 5063/5049ms → ALL timeout-settles. codex 2–26ms (already structural).

### C. Structural fix (evidence-derived, not prose)
Compared work vs post-turn frames: cursor work footer `→ … ctrl+c to stop` / opencode `… esc interrupt`
(both caught by `active_turn_footer`); idle = same composer minus the stop-hint. Two structural idle
signals added, both gated AFTER the working check:
1. `ready_prompt` also matches `→` (cursor's composer input arrow) alongside `❯`/`›`.
2. `composer_cursor_ready(alt_screen, cursor_visible, cursor_row, rows)` — a visible VT cursor in the
   bottom input region of an alt-screen full-screen TUI (opencode). New `ClassifyInput.composer_ready`,
   computed by BOTH adapters: offline from `signals.cursor`/`input_mode`; live from a new
   `ScreenState::cursor_input_state` (alt_screen + !hide_cursor + cursor row + rows). Parity preserved.

### D. AFTER (unified oracle, same replays) — latency collapses, no timeout settles
cursor real 15/30ms · cursor probe 12/58ms · opencode real 50/38ms · opencode probe 38/24ms · codex
unchanged. **Every transition structural (≤58ms), zero timeout-settles.** All 14 replays still
FINAL=idle/card=0/tailW=0; questions preserved (cursor q=256, opencode 249/254). `idle_hold_gate`:
all 4 providers' idle-hold bodies working=0/question=0 AND settle-tails now 0 (were 17–25 for
cursor/opencode). 9 classifier + 313 agent + 22 screen_state tests pass.

### E. Motion parity (coordinator priority 3)
Live passes `Motion::Still`; offline passed real `signals.quiescence.kind`. Experiment: forcing
offline to `Motion::Still` left ALL replay verdicts byte-identical ⇒ motion was redundant
(status_line/active_turn_footer/composer_ready cover every working frame). Made offline use
`Motion::Still` → replay==live, no offline-only Streaming reliance. (Documented in `classify_signals`.)

### F. Repaint-proof (coordinator priority 4): `testbench/replays/synthetic/repaint_proof_v1/`
Synthetic cast (`make_repaint_cast.py`): cursor turn → idle composer → repaint the idle composer
every 2s for 24s. Post-turn: `max_byte_idle=2000ms` (never reaches the 5000ms net), states={idle},
working_frames=0, FINAL=idle. A timeout-only classifier would be stuck Working forever here; the
structural composer signal keeps it Idle. Proves no indefinite stuck under live-like repaint.

### G. Live smoke
opencode tab `c3ada6ce`: sent a 1-sentence prompt, observed ~7.25s working, then settled to idle
within ~250ms (one poll) — NOT a 5s timeout. Structural settle confirmed on the live path.

### Status: Adversary blocker RESOLVED. cursor/opencode settle structurally (≤58ms replay /
~250ms live), not via timeout; no indefinite stuck under repaint; replay==live motion. The 5s net
remains only as a last-resort backstop for genuinely unknown layouts. Nothing committed.

---

## Round 10 — /stop must actually INTERRUPT the agent, not just flip state (Adversary)

### A. Defect (Adversary, confirmed)
R8 made `stop_tab` force-idle, but `bytes = provider.stop_keystroke()` is `None` for EVERY built-in
provider (trait default `stop_keystroke()` returns None; only JS drop-ins set `stop_key`). So the
REST/CLI stop sent NO interrupt — the agent kept generating and the state flip was a 1500ms illusion
(would re-flip to Working after the suppression window). R8's smoke only checked +0.7s (inside the
window), so it missed this.

### B. Fix
`stop_tab`: when the tab has an agent, the interrupt key falls back to ESC (`\x1b`) when the
provider declares no `stop_keystroke()` — mirroring the UI `interrupt_agent` path (which sends `\x1b`
for every provider). Then write_raw the key + `mark_agent_interrupted` (force idle + clear pending).
UI `interrupt_agent` path left unchanged. No agent on the tab ⇒ `{stopped:false}`, no-op.

### C. Live smoke — agent ACTUALLY stops, checked BEYOND the 1500ms window (codex `2bace0d6`)
```
sent a 3000-word essay task; BEFORE stop:   state=working assistant_len=238
POST /api/tabs/2bace0d6/stop -> {"ok":true,"stopped":true}
AFTER +3s:  state=idle assistant_len=238
AFTER +5s:  state=idle assistant_len=238   (delta=0 chars => generation HALTED)
screen tail: `› Напиши очень длинное эссе…` + `gpt-5.5 medium · …`  (idle composer, NO spinner)
```
State stays idle past suppression AND assistant output frozen at 238 chars AND idle composer on
screen ⇒ the ESC reached codex and cancelled the turn. (Contrast R8: without a key, output would
keep growing and Working would return after 1500ms.)
Follow-up: an http-handler integration test for `stop_tab` needs an AppState harness (none exists;
current tab_routes tests are pure body-parsing) — deferred; live smoke is the accepted evidence.

### Round 9 evidence bundle (for Adversary)
- Transition-latency oracle: `testbench/detect-rounds/idle_latency_oracle.py`. BEFORE cursor
  5040/5055, 5037/5008ms; opencode 5000/5063, 5063/5049ms (TIMEOUT). AFTER cursor 15/30, 12/58ms;
  opencode 50/38, 38/24ms; codex 2–26ms (all STRUCTURAL, 0 timeout-settles).
- Replays: `testbench/replays/{cursor,opencode,codex}/{real_abc_poll_v1-round2-*,provider_cli_question_probe_v1-*}/timeline.json`.
- Synthetic repaint-proof: `testbench/replays/synthetic/repaint_proof_v1/` (max_byte_idle=2000ms,
  working_frames=0, FINAL=idle); generator `testbench/detect-rounds/make_repaint_cast.py`.
- idle-hold gate: `testbench/detect-rounds/idle_hold_gate.py` — all 4 providers bodies+tails 0.
- Live opencode smoke: tab `c3ada6ce`, ~7.25s working → idle within ~250ms.

### Coordinator protocol ACK
Loop constitution (visibility-first, production-truth oracle, structural-only, negatives-weighted,
user-facing truth, before/after counts, no timeout-masking when a structural transition is possible)
acknowledged and being followed. Will report after each round and emit WAITING_FOR_COORDINATOR (with
what/where/decision) only when truly blocked; otherwise continue on the next runbook task.

### Status: /stop now sends a real ESC interrupt + force-idle; agent verified to actually halt past
the suppression window. build.sh check clean; dev restarted; prod untouched; nothing committed.
