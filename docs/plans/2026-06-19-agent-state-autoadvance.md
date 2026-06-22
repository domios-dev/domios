# Agent State Classification + Auto-Advance (plugin-level, voting-gated)

> Owner: this orchestrator (Dante). Brief from Andrey 2026-06-19 with two live
> screenshots. NOT started — design + plan only. Branch off `plugins-rethink`
> when scheduled. This is a NEW subsystem, adjacent to the orchestration
> de-burring (agent liveness) but distinct.

**Thesis:** an agent pane frequently parks on a prompt the orchestrator/user
could handle automatically — a CLI self-update prompt, a rate-limit menu, a
clarifying question. Today a human must notice and press a key. We want the
PROVIDER PLUGIN to (1) detect the parked state from structural fingerprints,
(2) classify it with a voting/confidence gate so we never act on a false
positive, and (3) take the correct per-prompt action — auto-advance past noise,
auto-schedule a resume after a rate-limit, or surface a real question to the
user. All provider-specific logic lives in the PLUGIN; core stays generic.

## The two concrete cases (real fixtures)

**Case A — Codex self-update prompt (on pane start).** Structural fingerprint:
```
✨ Update available! 0.139.0 -> 0.141.0
Release notes: https://github.com/openai/codex/releases/latest
> 1. Update now (runs `... install.ps1 | iex`)
  2. Skip
  3. Skip until next version
Press enter to continue
```
Default highlight is **"Update now"** → a blind Enter would trigger a mid-session
update. Correct action: navigate to **Skip** (or "Skip until next version"), then
Enter, so we land INSIDE the real Codex. Only when this is NOT an auth/login
prompt. (Per-prompt navigation, not a blanket Enter — exactly why it's plugin-side.)

**Case B — Claude session-limit menu.** Fingerprint:
```
You've hit your session limit · resets 4:20pm (Europe/Riga)
/rate-limit-options
What do you want to do?
> 1. Stop and wait for limit to reset
  2. Upgrade your plan
  3. Upgrade to Team plan
Enter to confirm · Esc to cancel
```
Pane status shows `waiting`. Default highlight is **"Stop and wait"** → Enter
confirms directly. Then: parse the reset time + timezone ("4:20pm (Europe/Riga)"),
and schedule an automatic resume — a delayed `continue your work` send + Enter at
reset-time + a small buffer — via the T7 reminder/delayed-send machinery.

**Generalization (same mechanism):** clarifying / network question prompts (agent
asks the user something) → extract the question(s), surface them in chat + notify
the user (do NOT auto-answer). Auth/login/API-key prompts → NEVER auto-act,
surface to the user. "Should be working but isn't" (idle while a turn is owed) →
detect + nudge or notify.

## Hard constraints (from Andrey)

1. **Plugin-level, no core hardcode.** The fingerprints, the per-prompt action,
   the reset-time parse — all in the provider plugin (codex / claude). Core only
   offers a generic capability + a generic reactor that executes the plugin's
   recommendation. No `if provider == "codex"` / literal-string match in core.
2. **No false positives — voting + strategic confidence.** The classifier is an
   ENSEMBLE of independent matchers; an actionable verdict requires agreement
   above a confidence threshold. A single weak signal → "no action". Auto-pressing
   keys on a misread screen is the cardinal sin (could confirm a destructive menu,
   answer an auth prompt, accept an update we didn't want).
3. **Testable.** Golden-fixture corpus: positive fixtures (these two + more) must
   classify correctly with confidence ≥ threshold and yield the correct action;
   a NEGATIVE corpus of normal agent output must yield "no action" (the
   no-false-positive gate). Per-provider, and native+JS parity.
4. **Auth is sacred.** Any login / OAuth / API-key / "paste your token" prompt is
   classified `WaitingOnAuth` and is NEVER auto-advanced — surfaced to the user.

## Architecture

**Provider capability (plugin side).** New optional capability on the agent
provider, e.g.:
```
classifyPaneState(snapshot) -> {
  state: Working | WaitingOnAuth | UpdatePrompt | RateLimited | ClarifyingQuestion | IdleStuck | Unknown,
  confidence: 0..1,                 // ensemble vote score
  data?: { resetAt?: ISO, questions?: string[], ... },
  action?: { keys?: ["down","enter"], delayedResume?: {at: ISO, text: "continue your work", thenKeys:["enter"]}, surface?: {kind, payload} }
}
```
- `snapshot` = the pane's recent rendered screen + parsed transcript tail (reuse
  the existing screen_state / pane_chat snapshot).
- The plugin runs its ensemble of matchers, computes a vote, and only emits a
  non-`Unknown` state above its internal threshold. It chooses the action
  (Skip-not-Update for Case A; confirm-wait + schedule-resume for Case B).

**Generic core reactor.** A poller/reactor gated on the existing
`PaneStatusTracker` — only runs when a pane is `idle`/`waiting` (so it adds no
hot-path cost while an agent streams). On that edge it asks the provider to
classify; if `confidence >= GLOBAL_THRESHOLD` AND `state != WaitingOnAuth` AND an
`action` is present, core EXECUTES the action through existing seams:
- key injection → the same path as `codeterm send --actions "down,enter"`;
- delayed resume → the T7 reminder/delayed-send envelope (`continue your work` +
  Enter at `resetAt + buffer`);
- surface → push a chat card (the questions / the limit notice) + notify the user
  (Tauri notification / Telegram bridge).
Core branches ONLY on the capability + the returned generic state/action — never
on a provider id or literal string.

**State model / introspection.** Formalize the pane agent lifecycle the tracker
exposes: `Working | WaitingOnUserPrompt(classified) | IdleStuck(turn-owed) |
RateLimited(resetAt) | Dead`. Gives the UI + orchestrator a truthful "is it
running / waiting / should-be-working-but-stuck" signal (ties into the
de-burring liveness theme: watchdog = death, this = parked/stuck).

## Testability (the heart of "no false positives")

- **Golden fixtures**: capture real rendered screens as fixtures (the two
  attached + a growing corpus). Each positive fixture asserts (state, ≥threshold
  confidence, exact action). Store under `tests/fixtures/pane-state/<provider>/`.
- **Negative corpus**: a set of NORMAL screens (mid-stream output, a tool-call
  list, a diff, a shell prompt) must ALL classify as `Working`/`Unknown` with no
  action. This is the regression wall against false positives — grow it whenever
  a misfire is found.
- **Voting unit tests**: each matcher tested in isolation; the ensemble tested for
  threshold behavior (one weak match → no action; agreement → action).
- **Native+JS parity**: per the plugin-boundary discipline, a `parity_test.rs`
  case asserts the native and JS classifiers agree on the shared fixtures.

## Case C — provider CLI not installed → install + auto-launch

When a provider's CLI (claude / opencode / codex / …) isn't installed, launching
the pane fails. Goal: detect "missing", install it (with consent), then relaunch
automatically so the agent just starts.

**Detection — by launch-error ground truth, NOT a PATH pre-check (this is the
false-positive answer).** A `which`/PATH probe is unreliable: CLIs installed via
npm-global under a custom prefix, nvm-managed node, Homebrew, a shell alias/
function, or a non-PATH dir all read as "not installed" → a pre-check would
auto-install a DUPLICATE (real harm). Instead, attempt the REAL launch command
and classify the failure:
- `command not found` / exit 127 / ENOENT on the launch binary → **`Missing`**
  (high confidence: it's the exact command we'd run, so however it was installed,
  if it runs it's installed; only a true not-found means missing).
- any other failure (auth, crash, non-zero with output) → NOT missing — do not
  install; classify separately (auth → surface; crash → surface).
A config override (point us at a custom binary/path) short-circuits this so a
user's non-standard install is honored, never clobbered.

**Install is an outward action → confirm, don't silent-auto.** It runs an
installer (network, OS-specific). Flow: launch → `Missing` → show the EXACT
per-OS install command + a confirm ("install <provider> CLI?") → run it in a
visible pane → verify (re-probe / version) → **relaunch the agent
automatically**. A config knob `agents.autoInstall = ask | yes | no` (default
`ask`) gates the confirm. The PLUGIN owns the per-OS install recipe + the
version-verify; core only orchestrates launch→classify→confirm→install→relaunch
generically.

**HARD RULE — NO sudo, ever.** Agent CLI installs are **user-space only**: no
`sudo`, no privilege escalation, no system-wide writes, and the confirm flow
NEVER asks for / runs sudo. Recipes use user-scoped methods only (e.g. npm/pnpm
global under a user prefix, the official user-space install script that writes to
`~/.local`/`~/.codeterm/bin`/`~/bin`, Homebrew on macOS which is already
user-owned). If the ONLY available install path for a provider would require
sudo, do NOT attempt it — surface to the user with the command to run themselves.
A recipe that contains `sudo` is invalid and must be rejected at the seam.

Realistic? Yes — with the launch-error signal it's low-false-positive; the only
care items are (a) confirm before modifying the system, (b) honor a config
override, (c) classify the error precisely (not-found vs auth vs crash).

## Proposed tracks

- **AS1** — capability + generic state enum/action types + the core reactor
  (gated on PaneStatusTracker idle/waiting; confidence threshold; auth-guard;
  executes via send-keys / T7 delayed-send / notify seams). No provider logic.
- **AS2** — Codex plugin: `UpdatePrompt` classifier (ensemble) + Skip-not-Update
  action + the on-start gate (only before first real entry; never on an auth flow)
  + golden fixtures (Case A) + negatives.
- **AS3** — Claude plugin: `RateLimited` classifier + reset-time/timezone parse +
  confirm-wait + schedule-resume (T7) + golden fixtures (Case B) + negatives.
- **AS4** — `ClarifyingQuestion` + `WaitingOnAuth`: extract questions → chat card
  + user notification; auth → surface, never act. Fixtures + negatives.
- **AS5** — state-model/introspection surfacing (UI + ledger): truthful
  running/waiting/idle-stuck/rate-limited, "should-be-working-but-isn't" detector.
- **AS6** — no-false-positive corpus + voting-threshold harness + native/JS parity
  test; a CI gate that the negative corpus stays action-free.
- **AS7** — provider-CLI-not-installed (Case C): classify launch failure as
  `Missing` (not-found ground truth, not a PATH pre-check), plugin per-OS install
  recipe + version-verify, generic confirm (`agents.autoInstall = ask|yes|no`) +
  auto-relaunch, config override for custom installs. **NO sudo / user-space
  only** — reject any recipe containing `sudo`, surface-to-user instead. Fixtures:
  a not-found launch error → `Missing`; an auth/crash launch error → NOT `Missing`
  (negative); a sudo-containing recipe → rejected at the seam.

## Risks / guardrails

- **Auto-pressing the wrong key** → mitigated by: voting + confidence threshold,
  auth-guard, Skip-not-Update (never auto-accept a destructive/installing option),
  and acting ONLY on the idle/waiting edge.
- **Reset-time parse** (am/pm, timezone, "tomorrow") → parse in the plugin,
  schedule with a buffer, and verify against the T7 envelope; if parse confidence
  is low, surface to the user instead of auto-scheduling.
- **Mid-work Codex update** → we deliberately choose Skip, never "Update now";
  updating is a separate, explicit user action.
- **Overhead** → gated on the status tracker; never classifies while streaming.

<!-- revision: 1 -->
