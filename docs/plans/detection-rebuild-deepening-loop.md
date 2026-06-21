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

## Live loop runbook — 2026-06-21 active coordination

This section is the active Source of Truth for the Opus improvement loop started after commit
`3f477246 feat: add replay-driven agent state detection`.

### Agents
- Loop Lead / fixer: pane `e6288f83` (`Detection Loop Lead`, Claude Opus).
- Adversarial reviewer: pane `2830a5ee` (`Detection Adversary`, Claude Opus, read-only).
- Visibility/replay researcher: pane `b6ca9166` (`Detection Visibility`, Claude Opus, read-only).
- Human-facing coordinator: Joule / pane `70670bb8`.

### Active known issues
1. **Codex stuck Working after completion.** Live example: pane `2bace0d6`, pty
   `84f81602-2386-4e8f-92f2-fe44f27878e1`; visually idle, backend `/api/panes`
   reported `agent_state: {"state":"working"}`.
2. **Codex ChatUI ordering bug.** Codex messages can render above/out of order relative to
   Andrey's questions. This is not only classifier work; inspect transcript ordering, message
   IDs/timestamps, and DOM order.
3. **OpenCode stuck Working after normal answer.** Andrey asked for a question/choice; OpenCode
   gave a normal prose answer instead of a blocking terminal question, but ChatUI stayed Working.
   If no real blocking question surface exists and the answer completed, state must become Idle.
4. **Question semantics.** For PTY providers, do not render inline option cards in ChatUI.
   Backend may parse/record the existence of a question and diagnostics in replay JSON, but UI
   shows only the state banner: Waiting/Open terminal/Cancel.
5. **Manual cancel semantics.** Stop button, Cancel button, and a single `Esc` typed directly in
   the terminal must force Idle and clear any pending question.

### Real-prompt corpus requirement
Synthetic prompts are not enough. Mine Andrey's real prompts from panes/sessions named
`Dickens`, `Verne`, and `Twain` (or their current matching panes/session history), especially the
questions that produced Cursor/OpenCode/Codex missing-Question or stuck-Working states. Use those
exact prompts only as corpus generators; never hardcode them into detection.

For each real prompt:
1. Record a provider replay with `testbench/replay-capture.py` or a purpose-built full-session
   capture.
2. Run Dump Doctor.
3. Render `timeline.json`.
4. Analyze state counts and exact frame ranges.
5. Compare to live `/api/panes` state and ChatUI DOM when possible.
6. Add any failure as a permanent replay/test case.

### Twenty-to-thirty-round loop protocol
Each round must have measurable visibility. A round without measurement is invalid.

1. Pick one defect/risk from known issues or replay review.
2. If the defect cannot be measured, build the measurement first.
3. Produce evidence before fixing:
   - replay path and frame range, or live pane id / pty id;
   - `/api/panes` `agent_state`;
   - terminal screen text/frame;
   - ChatUI DOM state if applicable;
   - counts: Working frames, Question frames, Idle frames, card frames, flickers/blips.
4. Fix only with structural signals: VT mode, cursor visibility, option-row structure, attributed
   cells, status widget, input anchors. Key-hint text may confirm a structure but is never the
   only signal. No provider prose or option-label hardcoding.
5. Re-run affected replay(s) and at least one negative replay for that provider.
6. Run focused tests and relevant build checks.
7. Reviewer must attack hardcoding, false positives, oracle vacuity, and ChatUI mismatch.
8. Report one round result with before/after numbers and exact artifacts.
9. Continue for 20-30 rounds or until blocked; coordinator re-arms self-delay wakeups.
10. When an agent finishes a round or cannot continue without a coordinator decision, it must
    immediately send a `WAITING_FOR_COORDINATOR` report. The report must include:
    - what was completed;
    - the exact blocker or stopping point;
    - the concrete next decision/action needed from the coordinator.
    Agents must not sit silently after a report. If the runbook implies an obvious next task,
    continue without waiting; otherwise report waiting and let the coordinator choose.

### Final ChatUI acceptance checks
Every substantial detector change must eventually be checked at the user-facing surface:
- Idle terminal/CLI -> no Working indicator.
- Working terminal/CLI -> Working only, no Question.
- Blocking terminal question -> Question banner only, no inline options/card.
- Stop/Cancel/manual `Esc` -> Idle.
- Plain A/B/C or numbered prose -> not Question.
- Normal answer completion -> Idle for Codex/OpenCode/Cursor/Claude.
- Message ordering in Codex ChatUI matches chronological user/assistant turns.

### Experimental streaming-response track
Investigate whether the currently streaming assistant response can be extracted from PTY before
JSONL/SQL transcript update. Keep it diagnostic-only until replay/player evidence proves it is
stable per provider. First acceptable output is timeline/player JSON diagnostics, not ChatUI
rendering.

### Coordination tick — 2026-06-21 18:15
Reports from Loop Lead, Adversary, and Visibility were reconciled.

Accepted facts:
- Dev now runs build `3f477246`, matching HEAD at the time of this tick.
- The stale-build explanation covers earlier live repros against `3c722440`, but it does not close
  the detector work.
- Round 2 real-prompt captures are useful corpus additions, not final proof.

Current blocker:
- `render_timeline.rs` / replay `detector.state` must not be a parallel approximation of
  production state. The next valid round must route replay verdicts through the same
  `classify_state` semantics used by the live service, or through a shared `classify_timeline`
  helper used by both production and replay. Any replay PASS before this oracle unification is
  provisional.

Next enforced order:
1. Unify replay/timeline state verdicts with the production classifier.
2. Re-run the existing probe + real-prompt replays and show exact state/frame/card counts.
3. Fix structural idle recovery for Cursor/OpenCode/Codex using input/composer anchors or last
   non-empty rows, never provider prose.
4. Add Codex ChatUI same-second message-order coverage.
5. Re-check Stop/Cancel/manual `Esc` -> Idle at API and ChatUI DOM levels.

### Coordination tick — 2026-06-21 18:42
Fresh reports from Rounds 3-4 were reconciled.

Accepted direction:
- The replay oracle blocker was addressed in the right place: `render_timeline` now threads the
  production `classify_state` semantics instead of a memoryless hold-decay approximation.
- The unified oracle reproduced stuck-Working before the fix and showed it resolved after the
  structural idle recovery changes.
- Round 4's `picker_active` gating is provisionally accepted as an offline-fidelity fix, pending
  adversarial review that it does not hide real non-DECSET questions.

Still active:
- Adversary must review the actual R3/R4 diff, especially the 5s stuck-Working safety net and
  picker gating.
- Cursor `idle_focus_churn` still has Working false positives during focus churn and is the next
  detector priority.
- Codex short-work under-detection remains queued.
- Codex same-second ChatUI message ordering needs deterministic unit/data-layer coverage and,
  when available, DOM coverage.
- Stop/Cancel/manual `Esc` -> Idle still needs API and ChatUI-surface verification.

### Coordination tick — 2026-06-21 19:08
Fresh reports from Rounds 5-8 were reconciled.

Accepted:
- `render_timeline` now uses the production classifier semantics; the old oracle-divergence
  blocker is no longer the top issue.
- Cursor `idle_focus_churn` was reclassified correctly: the large aggregate Working count was
  mostly real work-turn frames, not focus-churn idle false positives.
- Codex short-work was measured and is not currently user-visible except for a narrow startup
  edge.
- Stop via `stop_pane` now force-marks the pane Idle at the API level before sending the provider
  stop keystroke. ChatUI-surface verification remains open.
- Codex data-layer ordering tests are useful, but they do not close the observed live-render
  ordering issue without a DOM/live-render capture.

Current blocker:
- Cursor/OpenCode still appear to recover from Working through the 5s stuck-Working safety net
  rather than a structural idle/composer anchor. This creates a visible false-Working window after
  completed turns and can still fail indefinitely if the idle TUI keeps repainting before
  `byte_idle_ms >= 5000`.

Next enforced order:
1. Add a per-transition latency oracle. Final-tail checks are insufficient; each
   working-to-idle transition must report latency and fail timeout-only recovery.
2. Implement structural idle recovery for Cursor/OpenCode using VT/input-region/composer/chrome
   signals, not provider prose and not only `STUCK_WORKING_IDLE_MS`.
3. Resolve live/replay parity for `Motion::Streaming` / `Motion::Animating`, either by feeding
   real motion live or removing/documenting the offline-only trigger.
4. Find or record a post-turn idle-repaint replay to prove the safety net cannot hide an
   indefinite live stuck-Working state.
5. Continue Stop/Cancel/manual `Esc` API checks and add ChatUI-surface evidence when tooling
   permits.

### Coordination tick — 2026-06-21 19:34
The worker protocol was tightened after the agents finished reports and went idle silently.

New rule:
- Every worker/reviewer must send `WAITING_FOR_COORDINATOR` when it has completed its current
  round and needs the coordinator to decide the next step. The report must say what was done,
  where it stopped, and what action is needed next.
- The coordinator owns the next-step decision and must re-arm wakeups while the loop is active.

### Coordination tick — 2026-06-21 19:40
Fresh reports from Round 9/10 were reconciled.

Accepted:
- Round 10 `/stop` fallback-to-ESC is promising: Lead reports the route now sends a real
  interrupt and verified Codex stayed Idle beyond the 1500ms suppression window with no assistant
  growth. This is provisional until Adversary reviews the actual diff.

Active blocker from Adversary Round 9:
- The transition-latency script exists but is diagnostic-only. It must become a hard acceptance
  gate that fails timeout-only working-to-idle recovery.
- Cursor is structurally improved, but OpenCode still has safety-net transitions in
  `idle_focus_churn` idle-matrix where `alt_screen=false`.
- `composer_ready` must not pre-empt working grace or turn an uncaught real PTY question into
  Idle.
- The repaint proof must cover the unsolved OpenCode primary-screen/no-glyph case, not only a
  glyph composer happy path.
- Motion parity was resolved by forcing offline motion to `Still`; remove or clearly gate any dead
  `Motion::Streaming` / `Motion::Animating` production implication.

Next enforced order:
1. Make transition latency a failing gate.
2. Fix OpenCode primary-screen structural idle recovery.
3. Guard `composer_ready` against false-idle during work/question surfaces.
4. Add an OpenCode no-glyph/primary-screen repaint proof.
5. Have Adversary review Round 10 `/stop` fallback.

### Coordination tick — 2026-06-21 19:45
Adversary reviewed the Round 10 `/stop` fix.

Accepted:
- The code path now falls back to ESC for built-in providers whose `stop_keystroke()` is `None`,
  and it writes real PTY bytes via `write_raw`.
- Non-agent panes are no longer over-cleared by the force-idle path.

Rejected verification:
- The current stop/cancel oracle is vacuous because it checks only that state becomes Idle inside
  the 1500ms force-idle suppression window. It would pass even if no interrupt was sent or the
  agent kept generating.

Next enforced order:
1. Harden the stop/cancel oracle so it waits beyond the suppression window and proves state remains
   Idle.
2. Prove actual generation halts using PTY output/screen-hash/assistant-length stability for a
   post-stop window.
3. For question surfaces, prove the visible terminal question is dismissed or that state honestly
   returns to Question; do not hide a still-blocking question by clearing `pending_prompt`.
4. Continue the Round 9 blockers after stop verification is non-vacuous.

### Coordination tick — 2026-06-21 19:52
Visibility and Adversary reports were reconciled.

Accepted:
- The transition-latency tool now has a `--gate` mode. Current status: Claude/Codex/Cursor pass;
  OpenCode fails with 4 remaining safety-net transitions.
- Remaining OpenCode failures are localized: `alt_screen=false`, visible cursor near the bottom,
  inline box composer, no arrow glyph. This is the next detector fix.
- Adversary retracted the earlier Codex timestamp/comparator root cause. Codex data-layer messages
  use millisecond-precision ISO timestamps and the real fixture is ordered.

Still open:
- OpenCode primary-screen/no-glyph composer needs a structural idle anchor.
- The latency gate must become part of the acceptance path, not only an ad hoc tool.
- Stop/cancel verification must be non-vacuous beyond the force-idle suppression window.
- Codex ordering is now a coverage/surface issue: add an optimistic pending-user merge unit test
  and, when tooling permits, a DOM/jsdom/Windows Playwright order test.

### Coordination tick — 2026-06-21 19:57
Visibility r6 produced a non-vacuous `/stop` oracle.

Accepted:
- The new stop oracle avoids the previous vacuity by requiring a changing pre-stop baseline,
  then checking raw monotonic output growth, screen hash, state, and transcript length after the
  1500ms force-idle suppression window.
- Claude and Codex `/stop` are truly halted by fallback ESC in the non-vacuous oracle.

New blocker:
- OpenCode `/stop` is not interrupted by fallback ESC. In repeated non-vacuous trials, raw output
  kept growing after stop, the progress meter advanced, and state honestly reverted to Working.
  This is not a state-machine bug; OpenCode needs a working interrupt key/path.

Next enforced order update:
1. Investigate and implement the OpenCode interrupt path via provider capability
   (`stop_keystroke` or equivalent), not by masking state.
2. Re-run the non-vacuous stop oracle: OpenCode must show no raw output growth beyond suppression
   and remain Idle because generation really halted.
3. Keep the OpenCode primary-screen/no-glyph composer latency fix active.

### Coordination tick — 2026-06-21 20:03
Adversary reviewed the landed OpenCode composer/`alt_screen` fix.

Accepted:
- The fix made real progress: the OpenCode idle-matrix safety-net cases now resolve
  structurally, no provider-id branch was introduced, and working-grace moved before
  composer readiness so known mid-work footer gaps are protected.

Still open:
- The option/question guard is incomplete. It misses OpenCode box-button permission surfaces such
  as `Allow once / Allow always / Reject`, so composer readiness can still false-Idle a real PTY
  permission if the SSE/structured handle is absent.
- Transition latency is still not enforced in the acceptance path, and current regenerated data
  still has residual safety-net transitions.
- The no-glyph repaint proof still does not cover the OpenCode primary-screen/no-arrow case.
- OpenCode stop remains an interrupt-delivery bug: ESC is the declared key, but a programmatic
  lone ESC does not halt active generation in the non-vacuous oracle. Fix this at the capability
  or delivery layer, not via provider-id hardcoding or state masking.

Next enforced order:
1. Widen the structural question guard for box-button permission surfaces without provider prose.
2. Make transition latency a hard acceptance gate and account for all residual safety-net
   transitions.
3. Add OpenCode primary-screen/no-glyph repaint proof.
4. Fix OpenCode interrupt delivery/API capability so `/stop` truly halts generation.

### Coordination tick — 2026-06-21 20:08
Adversary investigated the OpenCode interrupt failure.

Accepted:
- OpenCode's declared stop key is ESC, from its provider manifest. The problem is not the key
  value; it is PTY delivery. A lone programmatic ESC byte does not register as an interrupt in
  OpenCode's TUI.

Preferred direction:
- Use OpenCode's existing HTTP/control-plane integration for interrupt/abort. The host already
  has the bound server base, session id, SSE subscription, and HTTP reply calls for structured
  prompts. Verify the exact abort endpoint against the live bound server and implement this as a
  capability-level interrupt path.

Constraints:
- No provider-id branch in core.
- No Ctrl+C guess and no double-ESC hack unless represented by a provider capability/manifest
  delivery hint.
- If the control plane is unreachable, fallback must be honest: do not force-idle while generation
  continues.
- Acceptance is the non-vacuous stop oracle: OpenCode output must cease and state must remain Idle
  beyond the suppression window.

### Coordination tick — 2026-06-21 20:14
Visibility r8 reconciled the OpenCode composer and stop tracks.

Accepted:
- OpenCode primary-screen composer latency is fixed on the available regenerated timelines:
  safety-net transitions went from `4/19` to `0/26`; Cursor/Codex/Claude also pass the current
  transition-latency gate.

Active detector blocker:
- A visible-cursor permission/choice box can still false-Idle. Synthetic
  `permission_box_visible_cursor_v1` reproduces the bug: visible bottom cursor, `alt_screen=false`,
  numbered/choice surface, and `121/121` frames classified Idle. Composer readiness must be
  vetoed by an on-screen option/choice box even when `cursor_hidden=false`.

OpenCode stop clarification:
- Current CodeTerm launches OpenCode as a plain TUI, not `opencode serve`; there is no bound
  local control-plane socket/port for HTTP abort today. The HTTP abort direction requires a
  server-mode/attach architecture change. Until then, OpenCode `/stop` remains a known unresolved
  interrupt-delivery issue, not a classifier/state issue.

Next enforced order:
1. Fix the visible-cursor permission/choice-box guard.
2. Finish full timeline regeneration and report final latency/strobe counts.
3. Keep OpenCode `/stop` as an architectural follow-up unless server mode/abort is actually wired.

### Coordination tick — 2026-06-21 20:20
Adversary and Visibility identified a shared OpenCode root cause.

Accepted:
- OpenCode's structured prompt server allocation is currently present only in the orchestrator
  spawn path. Other spawn paths pass `structured_prompt_server: None`, so OpenCode launches as a
  plain TUI with no SSE/structured prompt channel and no local HTTP control plane.
- This explains two OpenCode problems at once:
  1. Permission surfaces fall back to raw PTY, so `picker_question` is absent and composer
     readiness can false-Idle a visible-cursor permission box.
  2. `/stop` cannot use a control-plane abort because no OpenCode server is bound.

Primary direction:
- Centralize structured-prompt-server allocation in the shared spawn/launch path for every provider
  that declares the capability. This must be capability-gated, not provider-id gated.
- Validate normal UI/direct/restart/general spawn paths, not only orchestrator spawns.

Secondary direction:
- Keep the classifier permission/choice-box guard as defense-in-depth for genuinely server-less or
  degraded cases, but do not treat it as the main OpenCode fix.

Acceptance:
- A normally spawned OpenCode pane gets a structured prompt server/base registered.
- Structured prompts arrive via SSE/API, so permission surfaces become `Question` without inline
  cards.
- OpenCode stop can target the control-plane abort path, or at minimum the server base/session is
  available for that next step.
- Existing latency/strobe/permission synthetic replays remain green.

### Coordination tick — 2026-06-21 20:25
Adversary and Visibility prepared validation for the shared structured-server fix.

Accepted:
- Visibility added `opencode-server-check.sh`. Baseline for direct-spawned OpenCode currently
  fails all checks: no `--port` in launch command, no listening socket, no reachable control plane.
- Adversary identified that allocation alone is insufficient: subscriber startup is also currently
  orchestrator-only. Registering an endpoint without starting the subscriber will not populate
  `picker_question`.

Review criteria for the shared fix:
1. Single shared allocation chokepoint, not copied into every spawn path.
2. Subscriber start parity across direct/UI/restart/general/orchestrator paths.
3. Capability gate only; no provider-id branch.
4. Port allocation race/leak considered.
5. Endpoint/subscriber cleanup on close, exit, restart, and spawn failure.
6. Restart gets a fresh endpoint and subscriber.
7. Degraded fallback is honest if allocation/bind fails.
8. Direct-spawned OpenCode shows live structured prompts and exposes the control plane needed for
   future abort/stop.
9. Structured-first semantics: for providers declaring structured prompt/control-plane capability,
   SSE/control-plane events are the source of truth for questions and permissions. PTY screen
   parsing is fallback/diagnostic only for degraded or genuinely server-less cases.

### Coordination tick — 2026-06-21 20:35
Coordinator decision: GO for shared structured-prompt-server centralization.

Lead is authorized to implement the shared helper/chokepoint under these constraints:
- allocation, endpoint registration, and subscriber startup must move together;
- capability gate only (`needs_structured_prompt_server` / provider capability), no provider-id branch;
- direct/UI/restart/general/orchestrator spawn paths must all go through the same path;
- structured-capable providers must use the structured stream as the primary question/permission path;
- no prod kill; dev restart only when needed for validation;
- if port allocation, cleanup, or restart freshness requires a large unrelated refactor, stop and report
  `WAITING_FOR_COORDINATOR` with the exact blocker.

Acceptance order:
1. `testbench/opencode-server-check.sh` passes all checks, including daemon SSE subscriber connection.
2. A normal OpenCode permission prompt reaches the structured `Question` path without inline card rendering.
3. `testbench/stop-oracle.sh --mode http-abort` passes if base/session is available.
4. `testbench/transition-latency.py --gate`, `testbench/strobe-metric.py`, and
   `testbench/permission-guard-check.sh` remain green.
5. `./build.sh check` remains green.

Coordination protocol update: any worker that reaches `WAITING_FOR_COORDINATOR` must both run
`codeterm agent report` and send a short direct pane message to coordinator pane `70670bb8` with
what changed, the exact blocker, and the proposed next action.

### Coordination tick — 2026-06-21 20:42
Adversary added and coordinator accepted C9: **structured-first**.

Implications:
- zero manual setup remains required; capability is the only gate;
- no config/env/per-pane toggle for normal OpenCode structured behavior;
- no provider-id hacks, guessed Ctrl+C, double-ESC, or allocation-without-subscriber;
- for OpenCode and any future structured-capable provider, PTY grid parsing must not be the primary
  question/permission detector once the structured server/subscriber is available.

### Coordination tick — 2026-06-21 20:45
Adversary found an urgent orchestration blocker: the backend worker worktree for centralization was
created on the wrong base (`ed3cf911`, master-lineage), not on `plugins-rethink` / baseline
`3f477246`. That worktree does not contain `src-tauri/src/service/structured_prompt/`, so any
centralization diff from it is invalid and must not be reviewed or applied.

Required recovery:
- discard/abort the wrong-base worker output for this task;
- re-create or rebase the worker worktree on current `plugins-rethink` / baseline `3f477246`;
- before implementation, the worker must report:
  - `git rev-parse HEAD`;
  - `git merge-base --is-ancestor 3f477246 HEAD` result;
  - presence of `src-tauri/src/service/structured_prompt/mod.rs`;
- only then continue the `ensure_for_spawn` centralization and C1-C9 review.

### Coordination tick — 2026-06-21 20:46
Visibility reconciled the spawn architecture and narrowed the real gap:

- Path A: web/REST `svc.spawn_agent` already goes through the orchestrator allocation block and has
  allocation, endpoint registration, and subscriber startup. This path was live-verified green,
  including C2.
- Path B: desktop/Tauri `svc.spawn_pane_atomic` uses the `AgentLaunchParams` family and currently
  passes `structured_prompt_server: None` without allocation/subscriber. This is the main
  out-of-the-box user gap.
- Path C: CLI spawn also follows a non-orchestrator launch path and currently lacks the structured
  server/control plane.

Implementation focus:
- add `structured_prompt::ensure_for_spawn(svc, agent_cmd, pane_id, cwd)` or equivalent;
- keep side-effect order explicit: capability gate -> allocate -> register endpoint -> start
  subscriber -> return server;
- replace the orchestrator inline block with the shared helper;
- wire the `AgentLaunchParams` family (`transport.rs` / `spawn_pane_atomic`, `spawn.rs`,
  `multiplexer/commands.rs`, `agent_restart.rs`, and any equivalent non-orchestrator path);
- do not spend effort "fixing" the already-green web/REST path except to route it through the same
  helper for one-source ownership.

Lead is now actively editing in the main checkout on the correct base.

### Coordination tick — 2026-06-21 20:51

Lead reported the centralization diff ready in the main checkout. The shared helper now exists:

`structured_prompt::ensure_for_spawn(svc, agent_cmd, pane_id, cwd)`

Required review focus:
- C1/C2: helper owns allocation, endpoint registration, and SSE subscriber startup together;
- C8/B-ORDER: Path B must pass `StructuredPromptServer` into `AgentLaunchParams` before
  `build_launch_command`, so OpenCode launches with `--hostname/--port`;
- remaining `structured_prompt_server: None` sites must be classified, not ignored.

Coordinator added the missing C6 restart coverage after Lead's initial report:
- `agent_restart.rs` now uses `ensure_for_spawn` for fresh relaunch;
- for providers that need a structured server and have a valid session, restart-resume uses
  `build_launch_command(AgentLaunchParams { session_id, structured_prompt_server, ... })` instead
  of the old `build_resume_command` path;
- `plugins/provider-opencode/src/plugin.ts` and rebuilt `plugin.js` now add `--session` when
  `LaunchParams.sessionId` is present, so resume can still bind `--hostname/--port` through the
  normal launch-params contract.

Validation order remains:
1. Adversary C1-C9 review against current `git diff b14c1418`.
2. Visibility behavior gates after review GO: `opencode-server-check --via cli` (Path B/C proxy)
   and `--via rest`, structured prompt check, stop oracle, latency/strobe/permission gates.

<!-- revision: 23 -->
