# Detection replay scenarios — realistic conversation corpus

> From Andrey 2026-06-21. Replay is the core testing tool for detection. First build a good
> full-console replay recorder/player, then use realistic multi-turn conversations to learn the
> agent's in-progress/status line and card behavior without overfitting to static frames.

## Goal

Build a corpus of replayable console conversations that behave like accelerated recordings of
real CodeTerm usage. Each scenario is a 15-20 message dialogue, not a one-shot prompt. The replay
player shows the terminal over time plus detector overlays, so we can see false positives,
flicker, missed working states, and stale cards frame by frame.

The first vertical slice is deliberately small:

1. Pick one realistic scripted scenario.
2. Record one full-session replay for one provider.
3. Render a 10-15 fps attributed timeline.
4. Extract the provider's live in-progress/status line from PTY structure.
5. Show the extracted line in the replay player and later in chat UI.
6. Confirm zero false positives and zero flicker.
7. Add replay 2, then 3, then 4, then 5.

## Why long scenarios

Short prompts prove almost nothing. A real agent session includes thinking pauses, streaming,
edits, tests, errors, follow-up corrections, idle periods, and normal prose that contains words
like "approve", "1.", "2.", "proceed", and "Would you like". The detector must survive that
whole arc.

Long scripted dialogues create useful pressure:

- the status-line extractor sees multiple kinds of work, not one spinner frame;
- negative windows appear naturally between turns and inside ordinary prose;
- card detection is tested against realistic text, code, lists, and terminal chrome;
- replay exposes flicker that a final-state scorecard hides.

Existing real sessions are source material: mine the user's Claude/Codex/Cursor/OpenCode sessions
for realistic task shapes, then turn them into reproducible scripts with deterministic prompts and
expected phases.

## Scenario format

Each scenario should be stored as data before capture:

```yaml
id: snake_incremental_build
title: Build and iterate a small Snake game
providers: [claude, opencode, codex, cursor]
workspace_setup:
  files: []
  terminal: { cols: 120, rows: 32 }
turns:
  - user: "Create a minimal browser Snake game in a single HTML file."
    expect:
      phases: [working, idle]
      card: none
  - user: "Add keyboard controls and a visible score."
    expect:
      phases: [working, idle]
      card: none
  - user: "Add pause/resume and a restart button."
    expect:
      phases: [working, idle]
      card: none
  - user: "Make the snake speed up every 5 points."
    expect:
      phases: [working, idle]
      card: none
stop:
  after_final_idle_ms: 3000
oracle:
  forbid_card_on_all_negative_frames: true
  require_status_line_during_working: true
```

The capture harness writes input events into the same replay clock as output events. The replay
player shows input markers on a separate ground-truth lane, never as terminal output.

## Recording quality rules

A good replay must include:

- full arc from initial idle to final idle;
- every user turn and answer/action timestamped in the cast;
- terminal sampled/rendered at 10-15 fps for review;
- raw PTY bytes preserved byte-exactly;
- attributed frames: text, cells, cursor, hidden cursor, DECSET/alt-screen, changed cells;
- detector output per sampled frame: working/idle, status_line, pending_prompt/card/no-card;
- expected phase lane separate from detector output;
- no single-frame-only captures in the gating corpus.

Dump Doctor rejects:

- casts without input anchors;
- casts that do not return to idle;
- casts whose final vt100 render does not match `screen.txt`;
- positive scenarios where the expected phase never appears;
- negative windows with any transient card flash;
- working-status scenarios where the status line cannot be inspected in the player.

## First scenario candidates

### 1. Snake incremental build

Purpose: realistic coding workflow with repeated work/idle transitions and normal prose/code.

Turns, target 15-20:

1. Create a minimal browser Snake game in one HTML file.
2. Add keyboard controls and visible score.
3. Add pause/resume and restart.
4. Make speed increase every 5 points.
5. Add a start screen with difficulty selection.
6. Add wall-wrap mode as an option.
7. Add high score saved to localStorage.
8. Add mobile touch controls.
9. Add sound toggle, but do not use external assets.
10. Refactor game state into clearer functions.
11. Add comments only where the logic is non-obvious.
12. Fix a bug: quick reverse direction should be ignored.
13. Add a small test plan in comments or markdown.
14. Make the canvas responsive.
15. Summarize what changed and how to run it.

Expected detector pressure:

- many working/idle cycles;
- real code blocks and numbered lists;
- likely status widgets during generation;
- no real permission card expected;
- good first negative corpus for false cards.

Recommended first capture: `claude/snake_incremental_build_01`, because Claude's status widget is
the most important one to learn first and the user explicitly wants that progress line in chat UI.

### 2. CLI todo app with tests

Purpose: terminal-heavy coding flow with file edits, tests, failures, fixes.

Turns:

1. Create a small Python CLI todo app.
2. Add add/list/done commands.
3. Persist todos to JSON.
4. Add input validation.
5. Add pytest tests.
6. Run tests and fix failures.
7. Add filter by status.
8. Refactor storage.
9. Add help text.
10. Add sample usage.
11. Introduce a deliberate edge case: empty todo text.
12. Fix the edge case.
13. Add test coverage for it.
14. Summarize files changed.
15. Explain how to run.

Expected pressure:

- tool/test activity if the provider is allowed to run commands;
- possible permission prompt depending on spawn mode;
- rich terminal output that should not become cards.

### 3. Refactor existing CodeTerm helper

Purpose: realistic repo-local work, but higher risk and should not be first.

Turns:

1. Inspect a small helper module.
2. Explain its responsibilities.
3. Add a focused unit test.
4. Make a narrow refactor.
5. Run the targeted test.
6. Fix compile/test failures.
7. Add one negative test.
8. Summarize behavior and risks.

Expected pressure:

- closest to actual Andrey usage;
- more likely to run commands and hit permission/tool states;
- should be used after the recorder/player is already proven.

### 4. Marker-words prose drill

Purpose: deliberate false-positive attack inside a normal conversation.

Turns:

1. Explain approval workflows using numbered lists.
2. Include the words approve, proceed, "for 3 seconds", "Would you like", and "Esc to cancel" as
   normal prose.
3. Write a code sample that includes menu-like strings.
4. Add a markdown checklist.
5. Add a fake terminal transcript in a code block.
6. Summarize tradeoffs in a table.
7. Finish idle.

Expected pressure:

- zero cards on every frame;
- status may be working only while the agent is generating;
- this becomes a permanent negative once the player can show no flashes.

## First implementation slice

Do not start by implementing the whole scenario matrix. Start with:

1. Add scenario data for `snake_incremental_build`.
2. Capture one provider, one run.
3. Generate `timeline.json` at 10-15 fps.
4. Build the first HTML player view if none exists yet.
5. Inspect the terminal frames and identify candidate status-line rows.
6. Implement the smallest status-line extractor against the attributed frames.
7. Replay the same capture and verify no flicker.

Only then expand:

1. same scenario on provider 2;
2. same scenario on provider 3;
3. same scenario on provider 4;
4. second scenario on the weakest provider;
5. add marker-words negative once the extractor starts to generalize.

## Current Corpus Status

Captured and promoted for idle false-positive gating on 2026-06-21:

| Provider | Scenario | Geometry | Replay dir | Gate notes |
| --- | --- | --- | --- | --- |
| claude | `idle_focus_churn_v1` | real resize `100x30` | `testbench/replays/claude/idle_focus_churn_v1-20260621-idle-focus-v2-claude-claude-idle_focus_churn_v1-100x30` | all 3 idle holds represented; `working=0` during idle holds; `card=0`; `native_picker=0`; `picker_active=0` |
| opencode | `idle_focus_churn_v1` | real resize `100x30` | `testbench/replays/opencode/idle_focus_churn_v1-20260621-idle-focus-v2-rest-opencode-idle_focus_churn_v1-100x30` | all 3 idle holds represented; `working=0` during idle holds; `card=0`; `native_picker=0`; `picker_active=0` |
| codex | `idle_focus_churn_v1` | real resize `100x30` | `testbench/replays/codex/idle_focus_churn_v1-20260621-idle-focus-v2-rest-codex-idle_focus_churn_v1-100x30` | all 3 idle holds represented; `working=0` during idle holds; `card=0`; `native_picker=0`; `picker_active=0` |
| cursor | `idle_focus_churn_v1` | real resize `100x30` | `testbench/replays/cursor/idle_focus_churn_v1-20260621-idle-focus-v2-rest-cursor-idle_focus_churn_v1-100x30` | all 3 idle holds represented; `working=0` during idle holds; `card=0`; `native_picker=0`; `picker_active=0` |

These are the current trusted fixtures for "idle must stay idle, including focus
churn and blinking cursor" regressions.

Geometry smoke status on 2026-06-21:

- `geometry_smoke_v2` at `100x30` and `160x40` was re-rendered for all four
  providers.
- Fast replay doctor passes for seven of eight geometry runs. The exception is
  `opencode/geometry_smoke_v1-20260621-geometry-smoke-v2-opencode-geometry_smoke_v1-160x40`:
  the capture itself timed out in two phases, so it is a stress/debug replay,
  not a promoted gate.
- The valid geometry runs have `card=0` and `native_picker=0`. Claude/Codex
  startup terminal menus classify as `state=question`, `card=null`; Cursor and
  OpenCode have no terminal-question frames in the current geometry corpus.
- OpenCode wide layout is useful for status extraction: it falls back to the
  structural `▣ <status>` row when the richer spinner row is absent.

Known exclusions:

- `idle-matrix-v1` dirs for codex/cursor: missing `capture_resize`, wrong row
  geometry for working-state checks. Keep only as historical debugging context.
- Early/partial geometry smoke dirs without `meta.json`, `phases.json`, or
  `capture.log.json`: not gating.
- Claude `geometry_smoke_v2` with the startup safety dialog: useful as a native
  picker/card false-positive repro, not a clean idle corpus fixture.
- Existing opencode/codex `systems_design_long_chat_idle_v1` replays: valid
  negative long-text/no-card fixtures, but weak positive working/status fixtures
  because the current structural detector sees them as mostly idle.

Current open quality gap:

- Claude/Codex can miss very short first-turn work after a long silent startup.
  Do not fix this by treating user typing as work. The intended fix is a
  submit-anchored pending-response state, bounded by ready prompt / completion
  summary and validated on replay windows that include both typing and submitted
  turns.

Current terminal-menu question policy:

- Some providers show blocking terminal menus that are structurally questions,
  but are not clean enough to render as a Chat UI card yet. Example: startup
  trust menus with a hidden cursor, one selected numbered option, and at least
  one additional numbered option.
- These frames should classify as `state=question` with `card=null`. That tells
  the UI and replay player "the agent is waiting for user input" without
  fabricating a broken option card.
- The detector must be structural: cursor/visibility state plus menu row shape
  (`selected numbered option + sibling numbered option`), not vendor copy such
  as "trust this folder" or "press enter".
- Terminal-menu question detection runs only after active work/status/footer
  checks. A streaming response that happens to contain a `› 1.`-shaped numbered
  list must stay `working`, not become `question`.
- A question-state fixture is not promoted until the same rule is checked
  against negative prose with numbered lists, code blocks, markdown lists, and
  idle/focus churn.

Current terminal-menu fixtures:

| Provider | Replay | Evidence | Expected |
| --- | --- | --- | --- |
| codex | `testbench/replays/codex/idle_focus_churn_v1-20260621-idle-focus-v2-rest-codex-idle_focus_churn_v1-100x30` | startup hidden-cursor numbered menu; frame `141/1461` is the tracked example | `state=question`, `card=null`, `subtype=unknown` |
| claude | `testbench/replays/claude/geometry_smoke_v1-20260621-geometry-smoke-v2-claude-geometry_smoke_v1-100x30` | startup safety menu with `❯ 1.` selected row | `state=question`, `card=null`, `subtype=unknown` |

Open fixture gap: no confirmed OpenCode/Cursor parked terminal-question replay
yet. Capture one per provider before increasing question-state confidence beyond
startup trust dialogs.

## Status line extraction target

The output should be plain and UI-ready:

```json
{
  "state": "working",
  "status_line": "Thinking... 12s · 1.2k tokens · esc to interrupt",
  "phase": "thinking",
  "elapsed_ms": 12000,
  "tokens": 1200
}
```

Rules:

- prefer structural evidence: bottom/status row, volatile row, spinner cell, changed cells,
  cursor-hidden/DECSET, provider-specific styling;
- do not mark normal prose as status just because it contains "thinking" or "working";
- a one-frame disappearance is flicker unless the expected phase lane says the turn ended;
- extracted text must be stable enough to show in chat UI without jitter.

## Open decisions

- Exact scenario file format and directory: likely `testbench/detect-scenarios/*.yaml`.
- Whether the first capture is Claude or Codex. Recommendation: Claude first for the status-widget
  line, then Codex, OpenCode, Cursor.
- Whether the first player is a static HTML artifact under `testbench/player/` or a small dev-only
  route. Recommendation: static self-contained HTML consuming `timeline.json`.

<!-- revision: 5 -->
