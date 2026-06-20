# State-dump test plan — the scenario catalog (which prompts produce which dumps)

> From Andrey 2026-06-20. The concrete prompts/actions we send to each provider to RELIABLY
> drive it into each target state, so `state-capture.sh` records a labeled dynamic dump. Pairs
> with docs/plans/pane-state-engine.md. Reliability tiers: **[S] scriptable** (one prompt/action,
> deterministic), **[C] config-setup** (needs a special spawn — e.g. approval-on, logged-out),
> **[O] opportunistic** (can't force on demand; capture when it happens + one synthetic fixture).

## Coverage matrix (state × subtype × provider)
| target | claude | codex | cursor | opencode |
|---|---|---|---|---|
| WORKING→IDLE | [S] | [S] | [S] | [S] |
| multi-turn (W→I→W→I) | [S] | [S] | [S] | [S] |
| QUESTION model_choice (agent asks) | [S] AskUserQuestion | [S]* often prose | [S] | [S] (SSE question.asked) |
| QUESTION model_picker (/model TUI) | [S] | [S] | [S] | [S] |
| QUESTION permission (approve cmd) | [C] | [C] | [C] | [C]/SSE |
| QUESTION auth (login) | [C] | [C] | [C] | [C] |
| QUESTION rate_limit | [O] | [O] | [O] | [O] |
| QUESTION update | [O] | [O] | — | [O] |
| QUESTION error | [S] | [S] | [S] | [S] |
*codex multi-section is unreliable (invalid JSON) — capture single-choice prose only; skip multi-section.

## The prompts (vary wording to avoid overfit — rotate 3+ per state)

### WORKING → IDLE  [S]
Send a small real task in a scratch project; record WORKING until quiescence, then IDLE.
- "Write a Python function `fib(n)` with a docstring in fib.py."
- "Add input validation and two unit tests to fib.py."
- "Refactor the function in main.py to be iterative; keep behaviour."
Expected timeline: `working` → `idle`. (Rotate the three so the corpus isn't one-task.)

### multi-turn (W→I→W→I)  [S]
Send task 1, WAIT for idle, send task 2, wait for idle, send task 3. The capture script must
gate each send on observed quiescence (not fixed sleeps).
- ["create greet.py with a hello function", "add a --name flag", "write a test for it"]
Expected: `working,idle,working,idle,working,idle` (the real multi-turn the current dump lacks).

### QUESTION : model_choice — the agent ASKS the user to choose  [S]
Make the agent present options (this is the picker we surface as a card).
- "Before implementing, ASK me which caching strategy to use — give me options: in-memory, Redis,
  or file-based — and wait for my choice."
- "Ask me how to handle migrations, as a multiple-choice question with 3 options, then stop."
- claude multi-section wizard: "Set up a new service — ask me about language, style, and focus as
  a multi-step in-TUI questionnaire, then summarize my picks."
Expected: `working` → `question:model_choice` → (answer) → `working` → `idle`. The dump MUST span
the full transition (not just the picker frame).

### QUESTION : model_picker — the /model TUI (config, not agent-asked)  [S]
- action: send `/model` (+ Enter). Record `question:model_picker` while the list is open.
Expected: `idle` → `question:model_picker` → (pick) → `idle`. Reliable, deterministic fixture.

### QUESTION : permission — approve a command/patch  [C]
Needs a NON-auto-approve spawn (our default skips permissions). Spawn with approval-on
(codex `--ask-for-approval on-request`; opencode permission mode; claude without
skip-permissions), then:
- "Run the test suite." / "Delete the file tmp.txt." / "Run `git status`."
Expected: `working` → `question:permission` → (approve) → `working` → `idle`.
opencode: prefer the SSE `permission.asked` handle (console capture blocked by auto-approve).
ACTION ITEM: add an approval-on spawn variant for capture.

### QUESTION : auth — login/OAuth  [C]
Spawn in a logged-out sandbox (cleared creds / fresh profile) → the login prompt.
- action: spawn, no prompt needed; the auth gate appears on launch.
Expected: `question:auth` from launch. Needs a throwaway logged-out profile per provider.

### QUESTION : rate_limit  [O]
Cannot force on demand. Capture opportunistically when a real session hits the limit; keep ONE
synthetic screen fixture (hand-built from a real limit screen) for the classifier. Do not block on it.

### QUESTION : update  [O]
Appears when a new CLI version is available. Capture opportunistically + one synthetic fixture.

### QUESTION : error  [S]
- "Run `nonexistent_command_xyz`." / "Open the file /no/such/path and read it."
Expected: `working` → (error rendered) → the agent either recovers (→working) or parks needing ack.
Classify the parked error state; label the error segment.

## Auto-labeling (how the dump gets its timeline)
The capture script KNOWS the step it sent and the expected state; it records the send instant and
polls the live screen for the transition instant (quiescence settle = W→I; picker-active = →Q;
reply sent = Q→W). Those instants become segment boundaries. Human spot-checks. Rotate prompts.

## What the current (Phase-A) dumps still need
- Make `multi-turn` actually multi (gate sends on idle → 4+ segments).
- Replace the reused thin cursor/opencode/codex picker casts with FULL-session dynamic captures
  spanning working→question→working→idle.
- Add `[C]` permission + auth dumps (approval-on / logged-out spawn variants).
- Add `error` dumps. Keep `rate_limit`/`update` as single synthetic fixtures until they occur.
- Re-run calibration; the low question recall is a DETECTION gap (W1 region work), not a label gap.

## Execution plan & task breakdown (engineering)
The loop: **build dumps (agents) → verify dumps are GOOD (dump-doctor) → calibrate the 3 states on
them → verify scorecard → iterate**. Each task has an explicit acceptance gate; nothing advances on
an unverified artifact.

### Phase 0 — dump-doctor (the quality gate; precedes trusting any dump)  → task `dumpdoctor`
A validator (`testbench/state-doctor.sh` + a Rust check) that a state dump is REAL, not thin/reused:
- `session.cast` is a full session (spans the scenario, not a single picker frame);
- `timeline.json` has the EXPECTED segment shape for its scenario id (model_choice ⇒ a `question`
  segment; multi-turn ⇒ ≥4 alternating working/idle; error ⇒ an error segment);
- each labeled boundary corresponds to a real signal in the cast (re-render at the instant; the
  state-defining signal — quiescence / picker-active — is actually present there);
- flag reused thin casts (row/byte floor + no transitions).
ACCEPTANCE: doctor FLAGS the current shallow Phase-A dumps (multi-turn=2seg, cursor/opencode thin)
as bad and PASSES a known-good one. Runs in CI separate from gating.

### Phase 1 — build proper [S] scriptable dumps via agents  → tasks `dumps-A`, `dumps-B`
Use `state-capture.sh` + the scenario catalog above. Per provider, ≥2 dumps per [S] state, rotating
3 prompt wordings (anti-overfit). Cover: working→idle, multi-turn (sends GATED on observed idle,
not sleeps), model_choice, model_picker (/model), error.
- `dumps-A`: claude + codex. `dumps-B`: cursor + opencode.
ACCEPTANCE: every produced dump passes `dumpdoctor`; report the dump list + each timeline.

### Phase 2 — [C] config-setup dumps  → task `dumps-config` (after Phase 1)
Add an approval-on spawn variant (codex `--ask-for-approval on-request`, opencode permission mode,
claude no-skip-permissions) → permission dumps; a logged-out throwaway profile → auth dumps.
opencode permission via the SSE handle (console blocked). ACCEPTANCE: permission + auth dumps per
applicable provider, doctor-green.

### Phase 3 — calibrate the 3-state engine on the GOOD corpus  → task `calibrate` (re-task Phase-A pane)
Tune WAIT_MS / DONE_MS / ready-prompt (fix late IDLE) + question detection (pairs with W1 region
extraction) on the doctor-green dumps; re-run `state_calibration`.
TARGETS: WORKING P/R ≥0.90/0.90; IDLE ≥0.80/0.80; QUESTION recall ≥0.80; subtype accuracy ≥0.90;
W→I/→Q transition latency ≤1 tick. Iterate prompts/thresholds until met.

### Phase 4 — lock + iterate
Combined gate (`state-doctor` + `state_calibration` green) = the state regression corpus. Add dumps
where a state is weak. THEN wire the classifier into the live status path (pane-state-engine P2).

### Sequencing & parallelism
Wave 1 (now, parallel): `dumpdoctor` (compiles) ‖ `dumps-A` ‖ `dumps-B` (live captures, per-pane,
no daemon restart). Wave 2: `dumps-config` + `calibrate`. Orchestrator gates+merges each; W1's
region fix feeds question recall before `calibrate`.

<!-- revision: 2 -->

