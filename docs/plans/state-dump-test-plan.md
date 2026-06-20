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

<!-- revision: 1 -->
