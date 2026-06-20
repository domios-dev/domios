# Facet 8 — chat round-trip integrity, state-detection hardening & real agent-driven E2E

> Owner: Dante (orchestrator), from Andrey 2026-06-20 after the OSC4 chat regression.
> Mandate (verbatim intent): do NOT revert — strengthen. Make the state-detection
> mechanism robust and per-plugin; parse what the agent says and let the CHAT react to
> options/variants in-place (never need the console); above all make it TESTABLE and
> understandable — REAL agent-driven tests that send a message from the chat UI and prove
> the agent received it and replied, Playwright with content evaluation, small cases first
> then hard. Strong architecture, no patches, no legacy. I own the design + delegate +
> verify. Branch `plugins-rethink`.

## 1. Diagnosis — what actually broke, and how
Three distinct failures, one common root: **detection (state / session / message echo) is
heuristic, scattered, and never tested end-to-end against a real agent.**

- **OSC4 chat regression (fixed, disabled):** the shell-command timeline merge injected a
  blank "(no command captured)" item (an empty snapshot *boundary* entry with no
  command-line) and interleaved shell items into the agent timeline, displacing/hiding
  agent turns. Unit tests were green; nothing exercised the LIVE chat, so it shipped.
  → Disabled the merge (`useChatSession`), chat renders again. Root lesson: unit-green ≠
  chat-works; we had no agent-driven E2E.
- **Elementary "messages arrive wrong" (optimistic bubbles stuck on "sending…"):** the
  backend transcript DOES contain the user messages (verified: `pane chat` shows
  `▸ user йо`, `▸ user где ты` + the agent's replies), yet the UI's optimistic bubbles
  never clear. So `matchedOptimisticEchoes` (`packages/chat-core/src/dedupe.ts`) is failing
  to pair them. Prime suspects: the `ECHO_SKEW_MS = 2000` time-guard vs provider transcript
  timestamp skew (a provider stamps the echo > 2s from client send → guard rejects → never
  matches), and/or the `structured` update timing (the `pane-chat-msg` delivery vs the
  optimistic GC). This is the elementary win to land first — and it is exactly what an E2E
  "send → echo → bubble clears" assertion pins down deterministically.
- **Chat empty while the TUI works (session detection):** `pane.agent_session_id` stays
  empty → `chat_start` returns pending → chat UI empty forever though the agent TUI runs.
  ROG root-caused one instance: cursor's `cursorChatsDir` md5-hashed the FORWARD-slash cwd
  while cursor keys `~/.cursor/chats/<md5>` by the NATIVE-separator path (backslash on
  Windows) → hash miss → session never detected. Andrey's claude pane `Polo` shows
  `session: -` (claude binding miss, same class). This is fragile, per-provider,
  separator-/path-sensitive detection with no fixture gate (issue-class #181).

Common thread: **the seams that decide "which session is this pane", "did my message land",
and "what state is the agent in" are guesswork with no real-agent test harness.** Fixing
that harness + hardening those three seams is the work.

## 2. Architecture — strengthen the seams (no patches, no legacy)
1. **State-detection lives in each PLUGIN, one clear model.** `classify_pane_state`
   (agent-state-spec P2–P4) consumes `StreamSignals` (P1a) → a single per-provider
   `PaneAttentionState`. No scattered heuristics in core; core executes generic actions
   only. Each provider declares its fingerprints as data (manifest) + small `onMatch` for
   real computation. Adding a provider = adding a declaration, not core edits.
2. **Deterministic session detection per provider.** The pane→session binding
   (`chat_start` → `agent_session_id`) must be deterministic + separator/path-safe +
   FIXTURE-GATED: each provider's session-dir/key logic (cursor md5 chats-dir, claude
   `~/.claude/projects` dash-encoding, codex `~/.codex/sessions`) gets a test over real
   directory fixtures so a path/separator bug can NEVER silently empty the chat again.
   Fold in ROG's cursor native-separator fix + the claude binding miss.
3. **Reliable message round-trip.** chat-UI send → delivered to the agent → echoed in the
   transcript → optimistic bubble clears → assistant reply renders. Harden
   `matchedOptimisticEchoes` (timestamp-skew tolerance driven by evidence, not a guessed
   2s) and the `structured` delivery path. The contract: every sent message provably
   arrives and its bubble resolves; no perpetual "sending…".
4. **Chat reacts to state IN-CHAT (the WIN).** When the plugin classifies
   `clarifying_question` / `presented_options`, the chat renders interactive
   question/option cards; selecting one sends the choice back to the agent (keys/text) via
   the existing send path — no console. One `PaneAttentionState` contract drives the status
   pill, the cards, and the (correctly re-introduced) shell decorations.

## 3. Real agent-driven E2E — the backbone (Andrey's core ask)
A test that **sends a message from the chat UI and proves the agent received it and replied**
— with content evaluation, small cases first, then hard, run against a REAL agent.

- **Layer A — Playwright over the live WebView2 UI (ROG/Windows).** The true oracle: type
  into the chat composer, submit, and assert — the message appears as a user bubble, the
  optimistic "sending…" CLEARS, the agent's reply renders as an assistant bubble, the
  status pill transitions working→done, and (hard cases) an options card appears and a
  click sends the choice back. Evaluates *which* message and *how* it arrived.
- **Layer B — macOS-runnable API/headless round-trip** (fast iteration, no UI): drive
  `chat_start`/`chat_send` + read the parsed transcript + the `PaneAttentionState`; assert
  the same round-trip invariants minus pixels. Runs in CI on every change.
- **Case ladder:** (1) elementary — send "hi", agent replies, bubble clears, no stuck
  "sending…" (beats the current bug); (2) rapid multi-send — N quick messages each clear
  exactly once; (3) session-detection — fresh pane binds + transcript appears (no empty
  chat); (4) hard — claude/codex emit an options/clarifying prompt → chat shows a card →
  selecting it advances the agent, all without the console.
- **Hard gate:** no chat/state/session change merges without an agent-driven E2E proving
  the round-trip. This is what was missing and let OSC4 through.

### 3b. Randomized option-interception test (anti-overfit — the headline-WIN gate)
The in-chat option/question interception (CE5) must work for ANY prompt shape, not the one
demo menu in the screenshot. So the test is GENERATED, never canned:
- **Interrogator agent (Sonnet)** — each round it INVENTS a fresh, random prompt that makes
  the target agent present options or ask a clarifying question, in a deliberately VARIED
  form (see dimensions). Never a fixed prompt → the detector can't be tuned to one shape.
  (This is Andrey's "Sonnet asks random 3 things each time so we don't overfit.")
- **Target agent** (claude / codex / cursor / opencode) receives it via the chat send path
  → presents the interactive prompt in its TUI.
- **Oracle (model-judged):** the interrogator compares what the target ACTUALLY presented
  (terminal screen / transcript) against what WE intercepted — the classified
  `presented_options`/`clarifying_question`, the extracted option list, the chat card.
  Did we capture the question + ALL options + the right nav? Scores the round.
- **Close the loop:** the interrogator selects one option via the chat card (send-back) →
  assert the target advances as if that option was chosen.
- **Variety dimensions (randomized per round):** option count (2–9); labels (1/2/3, A/B/C,
  words); question-vs-options; single vs multi-select; RU/EN; a free "type something"
  option present or not; permission-style prompts; secondary/nested menus.
- **Pass bar (hard gate):** over N random rounds × M providers — interception
  precision/recall ≥ threshold; ZERO option-prompts misread as plain "waiting"; ZERO false
  cards on normal prose. Robust across variety, not tuned to the demo menu.
- **Runs:** Layer B (API: classify + extract assertion, model-judged) in CI; Layer A
  (Playwright: card renders + click advances the agent) on ROG.

## 4. Sequencing + tasks (I delegate, re-run the E2E gate myself before every merge)
- **CE1 — agent-driven E2E harness** [FIRST; it is the oracle for all the rest]. Layer B
  (macOS API round-trip) now + the Layer A Playwright spec handed to ROG. Seed the
  elementary + rapid-send + session-detection cases.
- **CE2 — fix the elementary message round-trip** (optimistic-match skew + `structured`
  delivery), gated by CE1's elementary/rapid cases. The immediate "messages arrive wrong"
  win.
- **CE3 — harden session detection per provider** (separator/path-safe, fixture-gated);
  fold ROG's cursor md5 fix + the claude `Polo` binding miss. Gated by CE1 case 3.
- **CE4 — state-detection in plugins** (agent-state-spec P2–P4: `classify_pane_state` →
  `PaneAttentionState`), the robust per-provider model.
- **CE5 — chat reacts to state in-chat** (question/option cards + send-choice-back) +
  re-introduce the shell decorations CORRECTLY (filter empty entries, never displace agent
  turns), gated by CE1's hard cases.
- **CE6 — randomized option-interception eval** (section 3b): the Sonnet interrogator that
  generates fresh varied option/question prompts each round, drives them through the chat,
  and model-judges whether we intercepted question+options correctly + the pick advances
  the agent. This is the anti-overfit gate for CE4/CE5; runs Layer B in CI + Layer A on ROG.
- Verify: orchestrator re-runs the E2E gate (Layer B in CI + Layer A on ROG) before each
  merge; a final live agent-driven pass per provider.

## 5. Guardrails
- No patches/legacy: fix the seams, don't band-aid symptoms. The shell-decoration feature
  returns only behind a passing hard-case E2E.
- Testability is the deliverable: every fix ships with an agent-driven E2E case that would
  have caught the bug.
- Per-plugin state model; core stays generic (asks capability, executes generic actions).
- Cross-platform: Layer A Playwright runs on ROG (WebView2); Layer B on macOS/CI.

<!-- revision: 1 -->
