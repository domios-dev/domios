# Facet 9 — general picker interception, categorization & auto-response

> Owner: Dante (orchestrator), from Andrey 2026-06-20. Goal: intercept ANY interactive
> prompt an agent/CLI shows in the terminal, surface it in the chat UI as choosable cards,
> categorize the choices, and AUTO-RESPOND to safe categories (update→yes, rate-limit→
> self-delay) — so you rarely touch the console. Architecture: GENERAL in core, SPECIFICS
> in plugins. Grounded in research: `prior-art-picker-interception.md`. Branch
> `plugins-rethink`. Tests via facet8's agent-driven E2E + the CE6 randomized interrogator.

## What the research settled (don't re-litigate)
- **Termios-from-master is INFEASIBLE — rejected.** The slave's line discipline
  (`~ICANON`/`~ECHO`/raw — what flips when a CLI reads menu keys) is invisible to the PTY
  master on BOTH Unix (separate termios structs; packet-mode only signals EXTPROC) and
  Windows ConPTY (no API exposes the child's input mode). We will NOT chase it.
- **Generic detector = OUTPUT-stream DEC private modes fused with the quiescence clock.**
  Signal: `alt_screen_active OR (cursor_hidden AND quiescence==Still AND NOT
  bracketed_paste_only)`. Rationale: inline pickers (inquirer/prompts/enquirer/bubbletea/
  fzf/gh/…) almost all HIDE the cursor (`?25l`) while a selection is active; require
  `Motion::Still` so a spinner (also hides cursor) isn't mistaken for a menu; `?2004h`
  bracketed-paste is an ANTI-signal (shells set it on every line prompt → use to EXCLUDE
  the normal shell); `?1049h` alt-screen is high-precision but full-screen-only. Keys on
  protocol brackets, not vendor menu text — fits "structural fingerprints, never strings."
- **Structured channels BEAT scraping for the 3 first-class agents** — the highest-leverage
  finding. Claude `--permission-prompt-tool` / `canUseTool`; Codex `app-server` JSON-RPC;
  Cursor ACP `session/request_permission`. These hand us the exact question + options + a
  PROGRAMMATIC answer path — deterministic, no terminal scraping. Use them for known
  agents; the generic output detector is the FALLBACK for opaque TUIs (aider, open-
  interpreter, fzf, gh, npm, ssh).

## Architecture — two-tier, general-in-core / specific-in-plugin
```
                         ┌── TIER 1 (preferred, deterministic) ──┐
 agent/CLI ── PTY ──►    PLUGIN structured channel  (claude permission-tool /
                         codex app-server / cursor ACP request_permission)
                              → { question, options[], answer(choice) }   ─┐
   (same stream)                                                          │→ PaneAttentionState
                         ┌── TIER 2 (generic fallback) ──┐               │   { presented_options |
 [CORE] semantic_marker + SignalExtractor:                                │     clarifying_question,
   CSI private-mode tracker (?25l/h cursor, ?1049 alt, ?2004 paste)       │     options[], answer_fn }
   fused with quiescence/cursor → `picker_active` signal ───────────────►─┘            │
   + extract the option grid from the vt100 screen (opaque TUIs)                       ▼
                                                                            [CHAT UI] choosable card;
                                                                            click → answer_fn (structured
                                                                            answer or nav-keys); cancel
                                                                            uses the known stage.
```
Core stays generic: it only emits `picker_active` (DECSET+quiescence) + the rendered option
grid. The plugin decides what the picker MEANS and how to answer — via its structured
channel (tier 1) or by declaring how to read/drive the grid (tier 2). register + consume;
core matches no vendor string.

## What this unlocks (Andrey's use-cases)
- `/model` with no arg → model picker rendered as buttons in chat; click sends the choice
  back into the picker. A "select model" UI button = the same send-back.
- Cancel the in-progress turn correctly — we know the exact stage (working/picker/question/
  done) → context-correct Esc/Ctrl-C, not blind.
- Any permission/options/question prompt → card in chat; never need the console.

## Categorize → auto-respond (default-deny, safety-first)
- **Default-deny:** auto-answer ONLY an explicit allowlist; everything else asks the human.
- **Auto-allow categories (examples):** `update_prompt` → accept; `rate_limited` → set a
  self-delay/resume automatically; benign informational confirmations with a capitalized
  safe default.
- **HARD-DENY always (never auto):** ssh host-key/TOFU prompts; any `[y/N]`-default;
  danger vocabulary ("permanently", "force", "drop", "delete"); typed-name confirmations.
  Classify the triggering command too (whitelist-first danger regexes). Circuit-break
  repeated identical prompts (kill auto-answer loops). Audit every auto-response.
- Category + policy live in the PLUGIN (provider knows its prompts); core enforces the
  hard-deny guards generically.

## Tasks (delegate; sequenced AFTER facet8 CE1 harness so each is provable)
- **P9.1 (core):** CSI private-mode tracker in `semantic_marker.rs` (cursor ?25l/h, alt
  ?1049, paste ?2004) → `StreamSignals.input_mode`; fuse with quiescence → `picker_active`.
  TDD over fixtures. Additive.
- **P9.2 (plugins, tier 1):** wire the structured permission/option channels — claude
  `--permission-prompt-tool`/canUseTool, codex app-server, cursor ACP request_permission —
  into `classify_pane_state` → `PaneAttentionState` with options + an answer path.
- **P9.3 (core/plugin, tier 2):** generic option-grid extraction from the vt100 screen when
  `picker_active` and no structured channel (opaque TUIs); nav-key answer path.
- **P9.4 (chat UI) = facet8 CE5:** render the card (options/model buttons/question); click →
  answer_fn; stage-aware cancel.
- **P9.5 (categorize → auto-respond):** the default-deny policy engine + hard-deny guards +
  circuit-breaker + audit; per-plugin categories.
- **P9.6 = facet8 CE6:** the randomized Sonnet interrogator gates BOTH tiers across known
  agents AND opaque TUIs (fzf/gh/npm) — anti-overfit.

## Guardrails
- Never termios-from-master (infeasible). Structural DECSET+quiescence only for the generic
  path; structured channels preferred for known agents.
- Tier-1 structured beats tier-2 scraping wherever available.
- Auto-response is default-deny + hard-deny guards + audit; destructive never auto.
- Core generic (picker_active + grid); plugin owns meaning/answer/category. No vendor
  strings in core.

<!-- revision: 1 -->
