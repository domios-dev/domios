# Detection rebuild — structural signals + a deepening (negatives-weighted) self-improvement loop

> From Andrey 2026-06-21. The current detection is layered FLAT substring/timer heuristics
> (`has_event_markers` matches ❯ / "1."+"2." / "for N" / "approve"; settle-poll re-parses the raw
> screen; working = byte-timer). It false-positives across all providers on normal output. The
> curated POSITIVE-only dumps overfit (we tuned the parser to pass them; never tested negatives or
> the live pipeline). Fix = move triggering to STRUCTURAL ANSI signals + the agent's own status
> widget, and harden it with a loop that DEEPENS (accumulates ever-harder real negatives and forces
> one rule to satisfy positives AND negatives at once). Branch `plugins-rethink`.

## Principles (what makes it not-говнокод)
1. **Structural, not substring.** A real interactive picker = the terminal entered interactive
   input mode: DECSET alt-screen `?1049h` and/or hidden cursor `?25l`, a footer-anchored option
   block, a selection attribute. Normal output never has all of these. Trigger on THAT, never on
   "the chunk contains ❯ / 1. / approve".
2. **Working = the agent's own live status widget**, not byte flow. Claude/codex/etc render a
   spinner + elapsed + token counter while generating (`✻ Thinking… (3s · ↑1.2k tok · esc to
   interrupt)`). Present ⇒ working (+ parse the progress to SHOW it); absent ⇒ idle. Structured,
   FP-free, and gives the rich indicator Andrey wants.
3. **Negatives are first-class.** The corpus must contain real NORMAL output (prose, code, lists,
   startup chrome, idle-at-composer, trigger-words-in-text). A false card on a negative is the
   worst failure and is weighted hardest.
4. **Test the LIVE pipeline, not the isolated parser.** Replay real `.cast` through the ACTUAL
   trigger→detect→card/state path. The old bench tested the parser alone → missed the trigger FPs.
5. **One rule must satisfy positives AND all negatives at once** → that pressure forces a
   structural rule (a per-frame substring hack that fixes one case breaks a negative).

## Phase 0 — First vertical slice (START HERE, per Andrey)
Don't build the whole corpus first. Prove the mechanism on ONE dump, with a VISUAL replay player,
then expand 2 → 3 → 4 → 5 (one per provider), each addition must stay stable.

1. **Capture ONE full-session dump** (full arc idle → working → … → idle), with input `"i"` rows.
   The recorder is byte-exact + event-driven (already denser than 15fps during animation).
2. **Sample a dense timeline at 10–15 fps** (every ~66–100ms): per sample render the attributed
   frame (text + cells + cursor/DECSET) — dense enough to expose flicker, transient false cards,
   spinner/status changes, and card-clear timing.
3. **Status-line / working extractor** (the first target): from the attributed frames, locate the
   live status line STRUCTURALLY (the transient bottom line with spinner/elapsed/tokens that
   updates) and emit `{working|idle, status_line, elapsed_ms?, tokens?, phase?}`. Parse ANSI/
   hidden/control structure — NOT loose prose regex.
4. **Replay player + overlay** (not just a JSON report): Rust harness emits `timeline.json`
   = `[{t, frame, cursor/DECSET, working, idle, status_line, card}]`; a self-contained HTML player
   loads it and lets Andrey **play / speed up / scrub**, showing the algorithm's verdict
   (working/idle/card/no-card + status line) overlaid against each rendered terminal frame.
5. **Oracle + eyeball check on dump 1**: zero false positives, zero flicker. Then add dumps 2→5
   (claude/opencode/codex/cursor), each must keep the extractor stable before broadening.

### Phase 0 ordering (Andrey, refined)
The recording mechanism comes FIRST, then the extractor, then incremental validation:
1. Build the recorder that produces a GOOD replay (full-session PTY, input anchors, dense timeline).
2. Extract the in-progress/status line from that replay and show that exact line in chat/UI so the
   user plainly sees the agent is working.
3. Validate incrementally — one recording, then two, three; add false positives to the corpus as
   found.

### Scenario set (realistic multi-turn, not toy one-liners)
Good scenarios are **15–20 message dialogues** where the provider really works: writes code, runs
it, hits errors, fixes, asks/answers, streams, pauses, shows progress. Source material can be mined
from real sessions (`codeterm sessions search`) and turned into reproducible scripts. Each scenario
must traverse the phases we test: idle, working-streaming, working-THINKING (status line w/
elapsed/tokens — the extractor's main target), tool-call/exec, turn-complete, a POSITIVE picker
(card + clear), and NEGATIVES (prose question, error-in-prose, marker-words-as-text).

Canonical **Sc-A "Snake, iteratively complicated"** (first scenario, ~18 turns): create snake.py →
run (error in prose = negative) → fix → add score → increasing speed → `/model` switch (positive
picker → answer → card clears) → pause on space → refactor into modules (multi tool-call, long
working) → "make it nicer" (agent asks clarifying question in prose = negative) → answer → approval
picker if provider prompts (positive) → persist high-score → write tests → run tests → wall-wrap
flag → "think hard about the snake body data structure" (sustained THINKING status line) → idle.
Other scenarios (later, for the matrix): debugging-a-failing-suite (exec/error-heavy), startup-idle
baseline, marker-words-in-prose (pure negative), long-planning (thinking stress test).

**First pick: claude + Sc-A** — claude's `✻ Thinking… (Ns · ↑N tok · esc to interrupt)` widget is
exactly the status line Andrey wants surfaced, and it's the provider he watches most. Then opencode
(cross-check its SSE structured card vs PTY), codex, cursor.

### Recorder / scenario-runner upgrade (the literal first task)
- Add input capture: `CastWriter::write_input` → `"i"` rows (or `inputs.json` sidecar as bridge).
- Scenario runner: spawn provider → start capture at IDLE (before first input) → send scripted
  turns, pacing on a RECORDING-ONLY quiescence gate (byte-quiet N s OR timeout — a pacing device,
  never a correctness signal) → run the FULL arc → stop capture only after final return-to-idle →
  write artifacts: `raw.cast` (+ `"i"` rows), `meta.json` (provider, scenario id+version, CLI
  version, cols/rows), `screen.txt` (final frame), `phases.json` (expected phase/card/working
  windows, authored from input anchors + player review).

Phase 0 is NOT throwaway: its status-line extractor becomes Phase 1's "working" source, and its
timeline/player becomes the Dump Doctor's renderer (Phase 2).

## Detection Replay Player (the PRIMARY review tool — anti-overfit centerpiece)
Not a side utility. An accelerated console recording WITH algorithm overlays — this is what shows
whether the console behaves correctly OVER TIME (JSON scorecards only confirm after the fact, and
hide flicker). Data contract: the Rust harness emits `timeline.json` (per-frame rendered + attributed
state + detector outputs + input anchors + expected phases); a self-contained HTML player consumes it.

1. **Time controls** — play/pause, speed 0.25/1/2/5/10×, frame-step, scrub by timeline. Sample/render
   at ~10–15 fps so transient flicker is visible.
2. **Terminal view** — the vt100-rendered frame exactly as the engine sees it; toggle an attributed
   view: cursor position, hidden-cursor, alt-screen/DECSET state, highlighted rows/cells, and
   cells-changed-since-previous-frame.
3. **Algorithm overlay** — beside/over the terminal, per frame: working/idle, extracted
   `status_line`, `pending_prompt`/card/no-card, prompt fields, phase label, confidence/reason.
4. **Flicker / FP timeline** — every state transition and card transition as colored bands; a
   one-frame working→idle→working blink or card flash is obvious. Summary counts: working flickers,
   card flashes, false-positives on negative windows, missed positives on positive windows.
5. **Ground-truth lanes** — input events as vertical markers (prompt sent, answer sent, key actions);
   scenario EXPECTED phases shown on a SEPARATE lane from detector output → prevents circular
   validation (truth ≠ what the detector said).
6. **Diff / debug panes** — when detector output changes, show WHY: the raw bytes chunk, decoded
   ANSI events, DECSET changes, candidate status-line rows, selected attributed cells. Fix STRUCTURAL
   rules from evidence, not guesses on flattened text.
7. **Export / share** — a failing frame range exports as a small bundle (cast slice + frames + oracle
   + detector timeline) = the bug repro.
8. **Incremental workflow** — one dump → inspect in player → write the simplest status-line rule →
   replay → confirm no flicker → add dump 2/3/4/5, watching regression/flicker directly.

## Phase 1 — Stabilize NOW (kill the FP flood, low risk)
- Replace `has_event_markers` substring trigger with a STRUCTURAL trigger: only when the chunk/
  screen shows DECSET alt-screen (`?1049h`) or hidden-cursor (`?25l`) [the picker entered
  interactive mode], OR a structured event arrived (opencode SSE). Drop ❯/"1."+"2."/"for N"/
  "approve" matching.
- Remove the raw-screen parse from `schedule_prompt_settle_poll` (make it structured-only, or
  delete it — native detect_picker is no longer a card source).
- Card (`pending_prompt`) = structured handle (opencode SSE) OR the DECSET-gated attributed-grid
  detector (real picker only). Nothing from plain text.
- Working/idle: stop using the byte-timer. Interim: WORKING only when the status-widget is present
  (Phase 2 parses it properly); else IDLE. (Or disable the indicator until Phase 2 if needed.)
- Gate: cargo build/test/fmt/clippy + run.sh + state-doctor; rebuild dev; LIVE smoke (drive normal
  Q&A on each provider → NO card, correct working/idle).

## Phase 2 — The deepening loop (the centerpiece)
A corpus of real, labeled captures grows each round; the detector is fixed to satisfy ALL of it;
harder negatives are added until it converges. The unit under test is the LIVE pipeline.

### Corpus shape
`testbench/detect-corpus/<provider>/<kind>/<case>/` each = { raw.cast, screen.txt, meta.json,
oracle.json }, where `kind ∈ {picker, working, negative}`. meta carries the ground-truth, and
`oracle.json` is a FRAME-LEVEL timeline (see Dump Doctor): per sampled frame the expected
`{card: null|{question,options,selected}, state: working|idle|question[subtype]}`. The cast MUST be
a FULL recorded session (idle → trigger → picker-active → answered → card-cleared → idle), not a
single steady-state frame.
- POSITIVES: real pickers (model/permission/multi-step/auth) + real working (mid-generation).
- NEGATIVES (the new, weighted half): normal Q&A, code output, numbered lists, startup banners/
  chrome, idle-at-composer, prose containing "approve/error/sign in/1. 2.", multi-turn.

### Why today's dumps are too shallow (the gap this loop closes)
Verified in `testbench/rust/harness.rs`: the replay engine (`replay()`) IS a real time-aware
recording player (events fed into `SignalExtractor` with TICK_MS idle interpolation) — but only the
agent-state cases use it. Concrete gaps, all fixed below:
1. **Card path collapses time.** `provider_screen_attr_actual_from_cast` folds ALL cast events into
   ONE final frame and runs `detect_picker` on it — no timeline, no mid-stream check.
2. **Captures are single-frame.** Picker casts (`model-picker-r1`) hold 1 output event (a full
   redraw of the settled picker), so there is nothing temporal to replay.
3. **Production pipeline never runs.** Harness calls `detect_picker`/`parse_prompt`/
   `classify_pane_state` directly; the real `process_pty_output → has_event_markers → structured_
   pending_prompt → schedule_prompt_settle_poll → pending_prompt` path (where the false-card bug
   lives) is untested. Green dumps coexisted with prod false-positives → the overfit.
4. **Assertion is final-state-only.** Even the 92-frame streams assert just the end state, so a
   transient false card mid-stream is invisible.
5. **Real captures are non-gating** (`gating: false`) → they print baselines but never fail the build.
6. **No UI/card oracle** proving what the user actually sees.

### One round = these stages, in order
- **Stage 0 — Harvest** (codex workers, live via dc; never rebuild dev): drive each provider into
  BOTH positive and negative surfaces (rotate wording), capturing the FULL session as a lossless
  raw.cast — the whole arc idle → trigger → picker-active → answered → card-cleared → idle, NOT a
  single settled frame. Tag kind + expectation. Goal each round: add NEW negatives (especially ones
  that resemble pickers but aren't).
- **Stage 0.5 — Dump Doctor (replay-fidelity gate)**: a dump cannot become a GATING fixture until it
  passes the doctor. Acceptance:
  1. Re-render raw.cast through the SAME vt100 renderer and verify the final frame matches
     `screen.txt` (faithful recording, no capture corruption).
  2. Build a timeline sampled every N ms: per frame `{screen, attributed cells, cursor/DECSET flags,
     pending_prompt, agent_state}`.
  3. Verify the scenario's REQUIRED PHASES exist in that timeline. Picker example: idle/no-card →
     trigger output → picker-active/card → answered → card-cleared → idle. A single-frame cast that
     can't exhibit the phases is REJECTED (forces real recordings).
  4. The replay must go through the REAL production output pipeline (Stage 1), not the isolated
     classifier.
  5. Frame-level compare vs `oracle.json`: card/no-card, state, prompt fields, and clear-timing at
     each phase.
  Only after the doctor passes is the fixture promoted to `gating: true`. (Fixes gaps 1–6 above.)
- **Stage 1 — Replay through the PRODUCTION pipeline**: run each capture through the ACTUAL prod
  path — `process_pty_output → has_event_markers → structured_pending_prompt →
  schedule_prompt_settle_poll → pending_prompt` + `classify_state` — sampled per frame, recording
  `{pending_prompt, agent_state}` at every sample. NOT `detect_picker` called directly. (Offline,
  fast, deterministic — but the SAME code path prod runs, so the settle-poll false-card path is now
  under test.) Assertion is PER-FRAME against the oracle (no card on ANY negative frame), not
  final-state-only.
- **Stage 2 — Judge** (opus oracle): given the RAW screen + the detector output, rule per case:
  `{correct, kind: ok|false_positive|false_negative|wrong_fields, corrected_expectation}`. A card on
  a negative = false_positive (hard fail). Missing a real picker = false_negative. Garbled options =
  wrong_fields.
- **Stage 3 — Triage + grow**: every failure becomes a permanent doctor-validated corpus case;
  false_positives weighted highest. Dedup vs seen.
- **Stage 4 — Fix** (opus, structural-only constraint): change the detector so it satisfies the
  WHOLE corpus at once — positives parse cleanly, negatives produce nothing. NOT allowed: new
  substring/glyph hacks. Allowed: DECSET/cursor/attributed-grid/footer-structure/status-widget
  rules. If a fix can't satisfy a negative without breaking a positive, that tension is the signal
  to deepen the structural rule.
- **Stage 5 — Gate + converge**: re-run the whole corpus through the live pipeline + cargo/tests +
  state-doctor. Pass = zero false_positive on negatives, true_positive on positives, garble-free
  fields. Converged after K (e.g. 3) consecutive rounds with no new failures. Each round adds harder
  negatives ⇒ the rule deepens, never flattens.

### Metrics per round (so we SEE progress)
- false_positive count on negatives (must trend to 0 — the headline), false_negative on positives,
  field-garble count, per-provider breakdown, corpus size. Logged each round.

### Working/idle sub-track (Andrey's spinner idea)
Parse the agent's status widget structurally: detect the spinner/elapsed/token line per provider
(its STRUCTURE — a transient bottom line with a timer/counter that updates), expose
`{working, elapsed_ms, tokens, phase}` → the UI shows the live churn + progress; widget gone ⇒ idle.
Harvested + judged the same way (negatives: normal output must read idle, not working).

## Converged design (brainstormed with Joule, rev 3)

### A. Corpus artifact contract (final)
- **Cast = byte-exact PTY output (as today) + native input rows in the SAME clock domain.** Add
  `CastWriter::write_input` → an `"i"` row `{t, payload:{text|actions, source}}` teed off the PTY
  input path. Trigger/answer timestamps share the output clock ⇒ the artifact is self-describing;
  a moved/copied cast never loses *why* the screen changed. `inputs.json` sidecar is allowed ONLY
  as a migration bridge for old captures, never the final contract.
- **Input rows are anchors, NOT VT output.** The replay runner treats `"i"` rows as phase markers
  (and, in interactive replay, as bytes that *would* be sent to the PTY) — it never writes them
  into the VT screen.
- **Attributed dumps, not just flattened text.** Each sampled frame stores the attributed grid
  (selection highlight, color, focus, cursor/DECSET flags), because flattened `screen.txt` equality
  can pass while the very evidence `detect_picker` keys on (selection/color/focus) was lost.

### B. The pure decision core (the real fix for determinism)
- Extract the PRODUCTION decision logic into a pure core:
  `step(input, now_ms, state) -> (emitted_decisions, next_state)` where
  `input ∈ {Output(bytes), Resize, Input(payload), Tick}` and `emitted_decisions` = card set/clear +
  agent_state changes. The core OWNS all time-dependent state: VT/screen state, SignalExtractor,
  provider classify, pending_prompt, agent_state.
- **Settle-poll becomes virtual scheduled callbacks**: on a trigger the core enqueues checkpoints at
  `now+120/240/400/700/1000ms`; a `Tick` (or any input) at/after a checkpoint fires it. No thread,
  no wall-clock sleep, no races.
- **`service/output.rs` becomes orchestration only**: feed PTY chunks + real `now` to the core,
  apply returned decisions (set/clear `pending_prompt`, emit UI events), drive `Tick` on a real
  timer. The harness calls the SAME core with the SAME provider registry, only with virtual time.
  The core must be the production core — NOT a duplicate detector (no second source of truth).
- **Boundary caveat (Joule):** the core emits INTENTS/events and owns only terminal-derived,
  time-dependent state (VT/screen, SignalExtractor signals, provider classify, pending_prompt,
  agent_state, scheduled checkpoints). Storage/broadcast side effects (DB writes, socket emits) stay
  OUTSIDE the core — service applies them from the emitted intents. Keeps the core pure + replayable.

### C. Migration of the existing corpus
- Existing single-frame attr cases (no input rows, no full arc) CANNOT pass the new Doctor. Demote
  them to **non-gating micro-tests** of `detect_picker` on a static attributed frame (still a cheap
  regression check). The NEW full-session corpus is the gating layer. Don't retro-fit old casts.

### D. Scenario matrix (scripted, ≥5 full-session per provider)
1. startup idle (negative — no card), 2. short turn-complete, 3. long streaming working/progress,
4. marker-words-in-prose (negative — "approve/error/sign in/1. 2." as text), 5. real interactive
positive (picker, full arc idle→trigger→active→answered→cleared→idle). **claude also gets a
thinking-widget/progress capture** for the spinner sub-track.

### E. Phase detection (anchored, never circular)
Phases derive from input-row timestamps + structural transitions (DECSET alt-screen, cursor
hide/show, resize, attributed-grid change). NEVER from `detect_picker`/`pending_prompt` — those are
the output under test, not truth.

### Gating gate (restated)
A dump becomes `gating: true` ONLY after Dump Doctor verifies: full arc present, input anchors
present, objective phases derived, frame-level oracle matched, AND replay went through the
production core (B), comparing attributed frames (not just text).

## Execution & sequencing
- Orchestrator (me) drives the rounds: harvest (codex) + judge/fix (opus), FIXED to (a) include
  negatives, (b) replay the PRODUCTION core, (c) weight FPs, (d) forbid substring hacks, (e) reject
  dumps that fail the Doctor. Gate + merge each round myself.
- **Sequencing decision for Andrey** (the one strategic fork): the (3a) core extraction is a real
  refactor of `service/output.rs`, bigger than the original "Phase 1 stabilize". Two paths:
  - **(i) Stabilize-first**: ship the minimal FP kill now (structural trigger + drop settle-poll
    raw-parse + working off the byte-timer), THEN do the core extraction as Phase 2's foundation.
    Immediate clean UI, low risk; the structural trigger is reused by the core (not wasted).
  - **(ii) Core-first**: go straight to the pure-core extraction, no interim stabilize.
  Recommend (i) — relief now, right architecture next. Andrey decides.

<!-- revision: 4 -->
