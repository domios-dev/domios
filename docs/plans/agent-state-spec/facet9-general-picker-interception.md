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
- **DECISION (Andrey 2026-06-20): NO MCP — the GENERIC detector is the PRIMARY path for
  ALL providers, including claude approvals.** The claude `--permission-prompt-tool`/MCP
  route is DROPPED: registering an MCP tool inside claude is painful + brittle in the
  interactive-PTY setup. So we intercept everything via the generic DECSET+quiescence
  detector (read the prompt off the screen, send keys back) — one universal mechanism, no
  MCP. Non-MCP structured channels (codex `app-server` JSON-RPC, cursor ACP
  `session/request_permission`) remain an OPTIONAL bonus per-provider IF they're cheap and
  reliably better than the generic path — but they are NOT required and NOT MCP, and the
  generic detector must stand alone for every provider. (For reference: those structured
  channels would only ever cover the tool/command-APPROVAL class anyway, never `/model`,
  menus, or free-form questions — which always need the generic detector.)

## Architecture — two-tier, general-in-core / specific-in-plugin
```
                         ┌── TIER 1 (APPROVAL/permission class ONLY, deterministic) ──┐
 agent/CLI ── PTY ──►    PLUGIN structured channel  (claude permission-tool /
                         codex app-server / cursor ACP request_permission)
                              → { tool, input, question, answer(allow/deny) }   ─┐
                         (model-picker / menus / free-form Qs do NOT use this)   │
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

## Scope clarifications (Andrey Qs)
- **Folder-trust prompts ("Do you trust the files in this folder?")** are NOT the
  approval/permission channel — they're a startup TUI prompt. They go through the GENERIC
  DECSET detector (yes/no/menu + quiescence) → card in chat. Candidate for an explicit
  auto-trust policy (it's the user's own workspace) but trust is security-sensitive →
  default surface/ask unless a policy opts in.
- **Agents differ** (claude: folder-trust + tool-permission; codex: app-server approval;
  cursor: ACP; opencode: its own). The two-tier design absorbs this: generic detector is
  the common denominator (catches any prompt); structured channels deterministically cover
  the approval class where they exist.
- **Do we need an MCP?** Only for the claude approval channel, and optionally:
  `--permission-prompt-tool` points claude at an MCP tool we register, so approval requests
  reach us programmatically. Codex uses app-server JSON-RPC (NOT MCP); cursor uses ACP
  (its own). The GENERIC detector (folder-trust, `/model`, menus, free-form questions)
  needs NO MCP — it reads the output + sends keys. So MCP = one optional path for one class
  on one agent, never a universal requirement.

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
- **P9.2 (plugins, OPTIONAL non-MCP structured channels):** NOT claude-MCP (dropped). Only
  where cheap + clearly better than generic: codex `app-server` JSON-RPC, cursor ACP
  `session/request_permission` → `classify_pane_state` → `PaneAttentionState`. Lower
  priority than P9.1/P9.3 — the generic detector must work without it.
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
- **Structured channels (tier-1) live ENTIRELY in the plugin** (claude MCP permission-tool,
  codex app-server, cursor ACP) — core has ZERO MCP/app-server/provider specifics. Their
  output funnels into the SAME unified `PaneAttentionState` that the generic detector feeds,
  so the chat + core treat tier-1 and tier-2 identically. If a structured channel works for
  a provider → great, add it plugin-side; it must still pass through the general case. Cover
  more providers/cases incrementally — never hardcode one into core.

<!-- revision: 1 -->
