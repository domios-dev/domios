# Detection visibility findings — round 1 (pane b6ca9166, Visibility)

Source of truth: `docs/plans/detection-rebuild-deepening-loop.md` (rev 5).
All `detector` verdicts below come from `src-tauri/examples/render_timeline.rs`,
which runs the SAME `offline_state_classifier` the live path uses → a green
offline gate is the proof gate for a classifier change before live re-check.

## 0. P0-CRITICAL caveat: the running daemon is NOT HEAD

| | commit | classifier |
|---|---|---|
| `git HEAD` (baseline) | `3f477246` | reworked (+602 lines in `offline_state_classifier.rs`) |
| **running dev daemon** | `3c722440` | OLD logic — exhibits the P0s |

`curl :7686/api/health` → `build: 3c722440`. The live stuck-Working evidence
was produced by the OLD build. **First action for any P0 re-check: restart dev to
HEAD** (`./build.sh kill && ./build.sh dev`), or you are debugging a bug HEAD may
already fix. (Andrey was live in these panes, so I did not restart.)

## 1. Codex stuck-Working — root cause (exact, before/after)

Live capture, pane `2bace0d6` / pty `84f81602` (before its PTY ended):
- Rendered screen tail = **idle composer**: `› Explain this codebase` (placeholder)
  + footer `gpt-5.5 medium · ~/.codeterm-dev/general`; **no** "Esc to interrupt".
- OSC0 title stream: `⠹⠸⠼⠴⠦⠧⠇ general` then bare `general` (spinner already stopped).
- `/api/panes` `agent_state` = `{"state":"working"}` — wrong.
- Later the pane's PTY ended → `agent_state` cleared to `null`. The stuck state was
  bound to that PTY.

Mechanism, `classify_state` OLD (`3c722440`) vs HEAD (`3f477246`):

| | OLD (running) | HEAD |
|---|---|---|
| Working trigger | `byte_idle_ms < WAIT_MS` (any byte) | structural: `extract_status_line() \|\| active_turn_footer() \|\| motion∈{Streaming,Animating}` |
| Idle reset | `semantic_idle_ms >= DONE_MS && ready_prompt()` | `ready_prompt()` (gate dropped) |

OLD fails exactly as the scenario comment says: codex composer redraws / cursor
blink emit bytes → `byte_idle_ms` keeps resetting under `WAIT_MS` → perpetual
Working; even when bytes stop, the idle reset needs `semantic_idle ≥ 800ms` which
the same churn prevents. HEAD switches the work signal from "bytes flowing" to
"status widget present" (structural) and ungates the ready-prompt reset.

Offline proof on HEAD — `codex_prompt_idle_regression_v1` (purpose-built for this
failure), 955 frames:
- per-turn idle-hold segments **working_pct = 0.0** (turns 1/2/3), `short_idle_blips=0`
- final frames f950–955 = `idle`; `tail3s_working = 0`.

## 2. OpenCode stuck-Working — real prompt + state

Live pane `40c87bb3` ("Dickens", opencode). Andrey's real prompt (corpus generator):
`а ты можешь мне послать допустим в виде опросника A B C варианты как ты умеешь внутри`
→ opencode answered in **prose** ("Да без проблем:") + marker `→ Asked 1 question`
+ `▣ Build · DeepSeek V4 Flash Free · interrupted`. Live state now `idle` (settled).
Same OLD-build `byte_idle` class of bug. HEAD question-probe opencode: working
f652–693 → **idle f694–885** (returns idle correctly); `tail3s_working=0`.

## 3. Question semantics — no inline cards (runbook #4)

`card_frames = 0` and `native_picker_frames = 0` in **every** timeline analyzed
(25 files). Real terminal questions DO get `Question` state with zero cards:
`terminal_questions_choice_v1` → codex question=203, opencode question=405, both
`card=0`. ✅ Question banner only, no inline options/card.

## 4. Provider divergence on the same probe prompt

`provider_cli_question_probe_v1` ("send a blocking A/B/C poll or say you can't"):
- **codex** → prose answer; question frames after the real prompt = 7 (transient).
  The Twain real prompt confirms codex gives **prose A/B/C/D** ("Ты отвечаешь: A, B,
  C или D", "в текущем режиме надёжнее текстовый A/B/C-формат"). → negative-prose case.
- **opencode** → real structured question (229 q-frames). **cursor** → real terminal
  question UI (236 q-frames).
So acceptance checks must be **per-provider with the matched replay**, not one prompt.

Question-probe state counts (HEAD), all end idle:
- codex  1087f: idle621/q203/w263, ends idle f896–1087
- opencode 885f: idle521/w135/q229, ends idle f694–885
- cursor 1047f: idle594/w217/q236, ends idle f854–1047

## 5. Measurable acceptance gate — `testbench/acceptance-gate.py`

Operationalizes the runbook's ChatUI acceptance checks over the corpus. On HEAD:

```
[PASS] idle-cli-no-working          (8 idle_focus_churn replays, tail3s_working=0, card=0)
[PASS] question-banner-no-card      (5 terminal_questions replays, card=0, native_picker=0)
[PASS] normal-answer-ends-idle      (8 replays incl. codex regression, final=idle, tail3s_working=0)
[PASS] working-only-no-question     (5 snake replays, mid_build_question=0)
GATE: GREEN
```

Oracles: `tail_working(last 3s)==0` (honest stuck-Working test — excludes the final
turn's legit working), `card_frames==0` (hard rule), `mid_run_questions==0` (excludes
startup splash). Run: `python3 testbench/acceptance-gate.py [--json]`.

LIVE-ONLY (not in replay corpus yet — need live daemon + ChatUI DOM oracle):
- Stop / Cancel / single terminal `Esc` → Idle.
- Codex ChatUI message ordering (P0 #2) — **frontend/transcript**, not classifier.
  HEAD touched `PaneSessionChat.tsx` (+57), `App.tsx`, `parser.ts` — inspect message
  id/timestamp ordering + DOM order, not `offline_state_classifier`.

## 6. Experimental streaming-response track

`render_timeline.rs` already emits the **volatile band** + `status_line` per frame
(`detector.status_line`, `volatile_rows`). That IS the streaming-response diagnostic
surface — keep it timeline/player-only until a per-provider stability bench exists;
do not route to ChatUI.

## Next measurements (highest value)
1. Restart dev to HEAD, re-check codex/opencode/cursor live `/api/panes` + ChatUI DOM.
2. Same-cast before/after: render `codex_prompt_idle_regression_v1/raw.cast` through the
   OLD classifier in a `git worktree @3c722440` → quantify stuck-Working frame delta.
3. Capture real-prompt replays from Twain(codex)/Dickens(opencode)/Verne(cursor) prompts;
   add as permanent cases; re-run `acceptance-gate.py`.
4. Build a live Stop/Cancel/Esc → Idle oracle (key inject + `/api/panes` poll).

<!-- revision: 1 -->
