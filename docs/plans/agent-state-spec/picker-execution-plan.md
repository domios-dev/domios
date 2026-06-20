# Universal picker interception — detailed execution plan (all providers)

> Owner: Dante (orchestrator), from Andrey 2026-06-20. The headline WIN: ANY interactive
> picker/question an agent shows (model picker, permission, options, multi-step wizard,
> multi-select) is intercepted and rendered as a selectable CARD in the chat; clicking
> sends the choice back — never touch the console. Across claude, codex, cursor, opencode.
> Builds on facet9 (generic picker_active signal + the existing prompt pipeline) + the
> PendingPromptCard (CE5). Andrey: execute the tracks with CODEX agents. Branch
> `plugins-rethink`. NO MCP.

## Architecture (settled — facet9)
PTY → core detects `picker_active` (DECSET cursor-hidden/alt-screen + quiescence) → the
PLUGIN parses the picker off the screen → `pane.pending_prompt {question, options[], …}` →
the chat renders `PendingPromptCard` (buttons) → click → `respond_prompt` →
plugin `build_prompt_response` → keys to the agent. Core stays generic; each plugin owns
its picker shapes + send-back. The CARD must MIRROR the picker's control surface.

## The matrix: (provider × picker-shape) — what exists, what's missing
Picker shapes observed live:
- **S1 single-select numbered list** — `1. … 2. …`, ↑↓/number + Enter. (codex, claude, opencode)
- **S2 single-select non-numbered list** — `→`-selector rows + metadata (cursor `/model`).
- **S3 question-with-options** — header + numbered options + "Type your own" (opencode "Выбери…").
- **S4 multi-step wizard** — top tabs (Язык/Стиль/Фокус + Submit), ←/→ between sections,
  each section its own question/options. (claude)
- **S5 multi-select** — checkboxes `[ ]`, Space toggles, Enter submit. (claude wizard,
  cursor multi-question)
- **S6 free-text option** — "Other: (type to answer)" / "Type your own" → text input.

| Provider | extraction | send-back | status |
|---|---|---|---|
| claude | S1 ✓ (/model live), S4/S5 wizard ✗, S6 ✗ | down-nav ✓ | /model WIN live; wizard/multi-select MISSING |
| cursor | S2 ✓ (/model list, p9x), S5 multi-q ✗ | **WRONG** (highlight starts on current, not top) | extraction ✓; send-back fix in flight (csb) |
| codex | S1 ✓ but FLAKY (slow render vs poll) | down-nav (universal) | reliability + send-back todo |
| opencode | S3 ✗ (vertical numbered question-menu) | ↑↓+Enter ✗ (only horizontal row done) | in flight (oc, codex worker) |

Cross-cutting:
- **Chat ORDERING** (prereq): transcript misorders rapid msgs (sort by coarse timestamp,
  no source-sequence tiebreaker). In flight (ord). Clean chat underpins everything.
- **Card generalization**: PendingPromptCard today = single question + buttons. Needs
  multi-select (checkboxes + Submit), wizard (sections/tabs + Submit), free-text input.
- **prose options (F3)**: agent writes "reply A/B/C" in its message text (not a TUI picker)
  → optional later: parse the assistant message for offered options → buttons.

## Tracks (each a worker; CODEX agents per Andrey)
IN FLIGHT:
- **ord** (chat ordering stable-sort) — claude worker, running.
- **csb** (cursor S2 send-back: highlight-relative / type-to-filter) — claude worker, running.
- **oc** (opencode S3 vertical question-menu extraction + ↑↓ send-back) — CODEX worker, running.

TO DISPATCH (CODEX):
- **T-codex** — codex S1 /model: make extraction RELIABLE (slow-render: widen the
  has-event-marker → screen-read window / poll until the menu settles) + confirm send-back
  (numbered down-nav, account for highlight). Live-repro + fixture + unit test.
- **T-wizard** — claude S4/S5 multi-step wizard + multi-select: (a) extraction parses the
  tab row (sections + which done ⊠/□) + current section question/options + multi-select
  flag + Submit; (b) GENERALIZE PendingPromptCard to render sections/tabs + checkboxes +
  Submit/Cancel, OR step-through one section at a time (do step-through first); (c) send-back
  maps card actions → keys (←/→ section, Space toggle, Enter select/submit, Esc). Live-repro
  the claude wizard (the "несколько вопросов сразу" menu) + fixtures + tests.
- **T-card** (may fold into T-wizard) — the card model generalization (single → multi-select
  → wizard → free-text S6). One component, capability-driven by pending_prompt shape.

THEN:
- **CE6** — Sonnet randomized interrogator: generates ALL shapes (S1–S6) × all providers,
  drives them through the chat, model-judges that we intercepted question+options correctly
  AND the pick advances the agent. Anti-overfit hard gate. (Runs Layer-B in CI + Layer-A
  Playwright on ROG.)

## Per-track contract (every worker)
- Investigate LIVE first (dev 7686, dc, never prod): drive the real agent to the picker,
  capture the exact screen → fixture. Don't presuppose the format.
- Extraction → `pane.pending_prompt`; send-back via `respond_prompt`/`build_prompt_response`
  + `keys.rs`. Core stays generic (no vendor strings); plugin owns shapes.
- Additive — never break normal chat (the OSC4 lesson) or other providers' pickers.
- Unit-test against the captured fixture + extend `evals/chat-roundtrip` with the case.
- Orchestrator re-runs the gate + verifies LIVE post-merge (picker → card → click → agent
  acts on exactly the chosen option).

## Sequencing
ord (ordering) underpins all. csb + oc + T-codex close per-provider single/list pickers +
send-back. T-wizard/T-card add multi-step/multi-select (the card generalization). CE6 locks
it all anti-overfit. Picker WIN is "done" when every provider's pickers render as a correct
card and a click does exactly the right thing — proven by CE6 across S1–S6.

<!-- revision: 1 -->
