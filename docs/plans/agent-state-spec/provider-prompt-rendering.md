# Provider Prompt-Rendering Profiles (facet9 generic-detector coverage)

Research note for facet9 (general picker interception, NO-MCP). For each first-class
PTY agent — **codex**, **cursor** (cursor-agent), **opencode** — this documents:

1. **Prompt-rendering profile** — DECSET signals (alt-screen `?1049h` / cursor-hide
   `?25l` / bracketed-paste `?2004h`), layout (inline vs full-screen), and quirks the
   generic `picker_active := alt_screen OR (cursor_hidden AND Motion::Still AND NOT
   bracketed_paste_only)` detector must survive.
2. **Send-back key scheme** — the exact bytes the generic answer path injects.
3. **Non-MCP structured bonus** — server/app-server/ACP/JSON-RPC (NOT MCP) that
   deterministically covers the approval class.

Grounded in: the three plugins (`plugins/provider-{codex,cursor,opencode}/src/plugin.ts`),
the core send-back path (`src-tauri/src/service/pane.rs::respond_prompt`,
`src-tauri/src/agent/keys.rs`, `src-tauri/src/agent/state_reactor.rs`), and the prior-art
note (`prior-art-picker-interception.md`). Web sources cited inline.

> **Current state of the core machinery (read before planning P9.1).**
> The DECSET tracker does NOT exist yet — `grep` for `?25l`/`?1049`/`?2004`/`cursor_hidden`/
> `input_mode` in `signal_extractor.rs`/`semantic_marker.rs` finds nothing (only a screen_state
> test fixture). The quiescence/`Motion::Still` clock (`signal_extractor.rs`) and `CursorSignal`
> DO exist. So P9.1 (CSI private-mode tracker → `picker_active`) is genuinely unbuilt.
> What EXISTS today is the **text-heuristic** path: `detector.rs::detect_interactive_prompt`
> (numbered options + selector glyph `❯`/`>` + keyboard-hint footer near the bottom 20 lines),
> driving `prompt.rs::PromptDetector`. That is the baseline the DECSET detector augments.

---

## Send-back: how the answer actually leaves CodeTerm (shared by all providers)

Two distinct answer paths exist; both end in PTY byte writes with ~100ms spacing.

- **Picker card click → `ServiceState::respond_prompt`** (`service/pane.rs:547`). It
  re-verifies the prompt is still on screen (`detect_interactive_prompt`), converts the
  clicked option `value` (1-based) → `option_index` (0-based), then asks the provider plugin
  for `build_prompt_response(option_index)` (base64-encoded byte steps); falls back to
  `default_prompt_response` = `option_index × ESC[B (arrow-down)` then `\r (Enter)`
  (`pane.rs:519`). **The default model is "navigate down N times from the top option, then
  Enter."** Steps are written one-per-100ms on a spawned thread.
- **Auto-response from `classifyPaneState` → `state_reactor.rs::action_to_writes`**. The
  classifier returns symbolic `keys: ["down","down","enter"]`; `keys.rs::action_key_bytes`
  maps each to bytes (`down`→`ESC[B`, `up`→`ESC[A`, `enter`→`\r`, `esc`→`ESC`, `tab`→`\t`,
  `ctrl-c`→`0x03`, `space`→`0x20`, letters via `ctrl-`). This is the vocabulary any
  provider's auto-action must speak.

Implication for facet9 tier-2: the generic answer path is **arrow-key navigation from a
known cursor position + Enter**. It needs the option grid AND the highlighted index to
compute the delta — exactly what codex's `parseMenuOptions`/`skipNavigationKeys` already do.
Number-key and letter-key shortcuts are an OPTIONAL per-provider optimization, not required.

---

## CODEX (`plugins/provider-codex`)

### 1. Prompt-rendering profile
- **Full-screen ratatui TUI (Rust).** Codex runs an interactive ratatui terminal app
  → enters the **alternate screen (`?1049h`)** and hides the cursor while a selection is
  active. This is the high-precision branch of the generic detector — `alt_screen_active`
  alone fires, no quiescence needed.
- **Prompt classes drawn:**
  - Command-execution approval — "Would you like to run the following command".
  - File-edit approval — "Would you like to make the following edits".
  - Network-access approval — "Do you want to approve network access".
  - Permission-grant — "Would you like to grant these permissions".
  - `/permissions` preset picker (Auto / Read Only / …) and `/model` picker — numbered
    menus inside the TUI ([Codex CLI features](https://developers.openai.com/codex/cli/features),
    [slash commands](https://developers.openai.com/codex/cli/slash-commands)).
  - Self-update prompt ("update available", "X.Y.Z → A.B.C", Update now / Skip), and the
    **auth/login prompt** (sign-in / OAuth / API-key paste).
- **Layout:** numbered menu, leading selector glyph marks the highlight. The plugin's
  `MENU_OPTION_RE = /^[ \t]*([>❯▶➤])?[ \t]*(\d+)[.)][ \t]+(.+?)[ \t]*$/` (plugin.ts:514) is the
  canonical shape: `> 1. Yes`, `2. No`. Codex defaults the highlight to option 1.
- **Quirks the generic detector must handle:**
  - **Spinner also hides cursor** while codex is thinking — `Motion::Still` discriminates
    (the alt-screen branch is immune, but the spinner churns the volatile region so a
    fused detector should still require quiescence before declaring "awaiting input").
  - **Auth is sacred** — codex login looks menu-like but must be SURFACED, never auto-pressed
    (`scoreAuthPrompt`, plugin.ts:582). Generic detector should yield to the auth classifier.
  - **Self-update menu defaults to "Update now"** — a blind Enter installs the update. The
    plugin navigates to "Skip" instead (`skipNavigationKeys`, plugin.ts:529). The generic
    path must NEVER blind-Enter a codex menu whose default is destructive/irreversible.
  - Box-drawing chrome lines (`─━═`) are noise (`isOutputNoise`).
- **Does the generic detector catch codex prompts?** **YES, strongly** — alt-screen `?1049h`
  is near-zero-false-positive, and the text path already matches the numbered menu. Approval,
  `/model`, `/permissions`, update, and free-form prompts all render as alt-screen numbered
  menus → caught.
- **Where it might miss:** (a) an approval rendered as a *streaming* region while the spinner
  is still animating — quiescence gate may briefly delay detection; (b) distinguishing
  "menu I may auto-answer" from "auth I must not" is a CLASSIFY concern, not a detection gap;
  (c) if a future codex build renders some confirmations inline (not alt-screen) the `?25l`
  branch still covers it.

### 2. Send-back key scheme
- **Arrow keys + Enter.** Codex menus are arrow-navigable numbered lists; the plugin emits
  symbolic `["down" × delta, "enter"]` (or `up`) computed from highlight→target delta
  (`skipNavigationKeys`). Default highlight = option 1, so from the top: N×`down` then `enter`.
- Number keys (`1`/`2`/`3`) also select directly in most codex menu builds, but the
  plugin/core path standardizes on arrow+Enter (matches `default_prompt_response`). Use
  arrow+Enter as the reliable scheme; number keys are an optional shortcut.
- **Cancel/escape:** `esc` (or `Ctrl-C` to interrupt the turn) — already in the key vocab.

### 3. Non-MCP structured bonus — `codex app-server` (JSON-RPC over stdio) — STRONG
- `codex app-server` speaks JSON-RPC and emits **server-initiated approval requests**:
  `item/commandExecution/requestApproval`, `item/fileChange/requestApproval`,
  `item/permissions/requestApproval`. The client replies with a decision payload —
  command: `accept` | `acceptForSession` | `decline` | `cancel` |
  `{acceptWithExecpolicyAmendment:{…}}`; file-change: `accept` | `acceptForSession` |
  `decline` | `cancel`. Requests carry `threadId`/`turnId` to scope to the active turn, plus
  the command, cwd, reason, optional `additionalPermissions`/`grantRoot`.
  ([App Server – OpenAI](https://developers.openai.com/codex/app-server),
  [openai/codex app-server README](https://github.com/openai/codex/blob/main/codex-rs/app-server/README.md)).
- **Verdict:** clearly better than scraping for the APPROVAL class (deterministic tool+input+
  reason, structured accept/decline, turn-scoped). BUT it is a *different process model* — the
  pane would run `codex app-server` instead of the interactive `codex` TUI; CodeTerm's whole
  PTY/session-binding/usage-parsing stack is built around the interactive TUI. So this is a
  real but **heavyweight** bonus: only worth it if a pane is launched specifically in
  app-server mode. `/model`, `/permissions`, update, auth still need the generic detector. Keep
  it OPTIONAL per the facet9 decision; do not make codex depend on it.

---

## CURSOR / cursor-agent (`plugins/provider-cursor`)

### 1. Prompt-rendering profile
- **Full-screen alternate-screen TUI.** Confirmed: "the TUI uses **alternate-screen
  rendering** with differential updates … no scrollback clutter after you quit"
  ([Cursor Agent CLI blog](https://cursor.com/blog/cli)). → strong `?1049h` signal, cursor
  hidden during selection. Generic detector's alt-screen branch fires directly.
- **Prompt classes:**
  - **Workspace-trust prompt** at startup — "Workspace Trust Required", "Cursor Agent can
    execute code", "Run Everything" (the plugin's fingerprints, plugin.ts:7). This is the
    folder-trust class facet9 routes through the generic detector (surface/ask by default;
    the plugin already pre-writes the `.workspace-trusted` marker only under `--force`/
    skip-permissions, plugin.ts:327).
  - **Command/tool approval** — by default terminal commands need approval; "Run Mode" /
    allowlist / "Run Everything" disable prompting
    ([Using Agent in CLI](https://cursor.com/docs/cli/using),
    [Agent Security](https://cursor.com/docs/agent/security)).
  - `/model` picker, conversation navigation — in-TUI menus.
- **Quirks:**
  - The plugin currently does **NOT** classify prompts at all — `hasEventMarkers` returns
    `false`, `detectEvents` returns `[]`, there is no `parsePrompt`/`classifyPaneState`. So
    cursor relies ENTIRELY on the core generic path (text heuristic today, DECSET tomorrow).
  - Differential redraws: cursor repaints regions, so partial-redraw noise is expected —
    quiescence gating matters to avoid firing mid-repaint.
  - Box-drawing chrome includes rounded corners `╭╮╯╰│┃` (`isOutputNoise`, plugin.ts:42).
- **Does the generic detector catch cursor prompts?** **YES via alt-screen** — `?1049h` is
  the cleanest possible signal and cursor is fully alt-screen. The trust prompt and approval
  prompts are all in-TUI overlays.
- **Where it might miss:** option-grid extraction (tier-2) is unproven for cursor — its
  overlays may use a horizontal button row or non-numbered layout (like opencode) rather than
  the numbered list `detect_interactive_prompt` keys on. **This is the real gap: detection
  (is a picker up?) is solid; structured EXTRACTION of cursor's option labels/highlight is
  not yet implemented and may need a cursor-specific `parsePrompt`.**

### 2. Send-back key scheme
- **Arrow keys + Enter** for in-TUI menus; `esc` to cancel. No plugin-declared scheme exists
  yet → falls back to `default_prompt_response` (N×down + Enter). If cursor's approval overlay
  is a horizontal row (left/right navigation) the default down-navigation would be WRONG — a
  cursor-specific `buildPromptResponse`/`parsePrompt` returning `left`/`right` deltas would be
  needed (same problem opencode already solved).
- **ACP optionIds** (below) are the clean answer for the approval class.

### 3. Non-MCP structured bonus — Agent Client Protocol (ACP) — STRONG, emerging standard
- cursor-agent speaks **ACP** (JSON-RPC). `session/request_permission` carries an `options[]`
  array, each `{ optionId, name, kind }` where `kind ∈ {allow_once, allow_always,
  reject_once}` (a `reject_always` is requested but not yet shipped). The client replies with
  the chosen `optionId` — explicitly "without relying on terminal UI prompts."
  ([ACP | Cursor Docs](https://cursor.com/docs/cli/acp),
  [Session Modes – ACP](https://agentclientprotocol.com/protocol/session-modes)).
- **Verdict:** ACP is the cross-vendor direction worth tracking and is clearly better than
  scraping for cursor's approval class. Same caveat as codex app-server: it's a distinct
  run-mode (cursor-agent as an ACP server) vs the interactive TUI that CodeTerm's session
  binding targets. Known rough edges: rejection not always reported to the client, missing
  shortcut for "Allow once" in some hosts ([forum bug](https://forum.cursor.com/t/acp-permission-rejection-not-reported-to-client/153825)).
  Keep OPTIONAL; the generic detector must stand alone (and today it's cursor's ONLY path).

---

## OPENCODE / sst/opencode (`plugins/provider-opencode`)

### 1. Prompt-rendering profile
- **Full-screen bubbletea TUI (Go).** Uses the alternate screen → `?1049h` fires; cursor
  hidden during selection. Leader-key driven (`ctrl+x` default leader,
  [OpenCode TUI docs](https://opencode.ai/docs/tui/)).
- **Prompt classes:**
  - **Permission dialog** — "Permission required", "Allow this tool call",
    "Approve this command" (plugin OUTPUT_SIGNALS, plugin.ts:13). Three outcomes:
    **once** / **always** / **reject** ([Permission System – DeepWiki](https://deepwiki.com/sst/opencode/5.2-permission-system)).
  - `/model`, agents (`tab agents`), commands palette (`ctrl+p commands`) — TUI menus.
- **CRITICAL layout quirk — horizontal option row, NOT a numbered list.** opencode's
  permission options render as a **single horizontal row** ("Allow once  Allow always
  Reject"), so the host's universal numbered-list reader CANNOT parse it. The plugin owns a
  bespoke `parsePrompt` (plugin.ts:94) that strips box borders, finds the row containing
  allow/reject, splits on 2+ spaces into labels, and returns `{question, options:[[label,
  "1"],[label,"2"],…]}` (value = 1-based index). The host then navigates by index.
- **Quirks:**
  - Box chrome includes `█▀▀█` logo blocks and `┃╹` separators (used as detection signals,
    plugin.ts:13; treated as noise in `isOutputNoise`).
  - **Keybinding REGRESSION (must design around):** single-key shortcuts (Enter=allow once,
    `a`=always, `d`=reject, Esc=reject) were **REMOVED** in a permission rework. Current
    opencode requires **arrow keys `←/→` (or `h/l`) to move between the horizontal options,
    then Enter**, with an EXTRA confirmation prompt for "Always allow"
    ([opencode #7954](https://github.com/anomalyco/opencode/issues/7954)). So the answer
    scheme is version-dependent and the "always" path is multi-step.
- **Does the generic detector catch opencode prompts?** **YES for detection** — alt-screen
  `?1049h` + quiescence reliably says "a picker is up." But **generic numbered-list EXTRACTION
  fails** (horizontal row) — this is precisely why opencode ships its own `parsePrompt`. So
  detection = generic; extraction = plugin-specific. This is the model facet9 expects
  (core detects, plugin reads/answers).
- **Where it might miss:** purely on extraction, not detection — already mitigated by the
  plugin's `parsePrompt`.

### 2. Send-back key scheme
- **Horizontal navigation: `left`/`right` (or `h`/`l`) + `enter`; `esc` rejects.** The current
  build removed single-key shortcuts. Because options are a horizontal row, the correct
  send-back is `right × (target_index − current_index)` then `enter` — NOT the default
  vertical `down × N`. **The `default_prompt_response` (down+Enter) is WRONG for opencode**;
  opencode needs a plugin-side response builder emitting `left`/`right` (it currently provides
  `parsePrompt` for reading but does not override `buildPromptResponse`, so the host falls
  back to down-navigation — a likely BUG to fix when wiring facet9).
- **"Always allow"** triggers a secondary confirmation → the answer path is multi-step
  (`right…enter` then confirm). Auto-answering "always" must account for the extra step.

### 3. Non-MCP structured bonus — opencode server + plugin event API — MODERATE
- opencode runs a local **server** and has a first-class **plugin API** (CodeTerm already
  installs a session-binding JS plugin into `~/.config/opencode/opencode.json`, plugin.ts:196).
  The plugin API exposes hooks (`tool.execute.before`, `chat.message`, `event` with
  `session.*`/`message.*`) and opencode supports ACP as a host too
  ([opencode #28926](https://github.com/anomalyco/opencode/issues/28926)). A
  `tool.execute.before` hook can observe/gate tool calls programmatically (NOT MCP).
- **Verdict:** opencode's plugin/event channel is the cheapest structured bonus of the three
  because CodeTerm ALREADY installs an opencode plugin — extending it to push permission
  events is incremental. But it's still OPTIONAL; the permission DIALOG itself is best read by
  the existing `parsePrompt` + generic detection, and `/model`/menus need the generic path.

---

## Per-provider verdict — does the generic DECSET+quiescence detector cover it?

| Provider | Render | Best DECSET signal | Detection coverage | Extraction coverage | Send-back | Structured bonus (non-MCP) |
|---|---|---|---|---|---|---|
| **codex** | full-screen ratatui (alt-screen) | `?1049h` (near-zero FP) | **Solid** — alt-screen + numbered menu | **Solid** — numbered list, plugin already parses + skip-nav | arrow `down/up` + `enter`; `1/2/3` optional; `esc`/`ctrl-c` cancel | `codex app-server` JSON-RPC `requestApproval` (accept/decline) — STRONG, heavyweight |
| **cursor** | full-screen alt-screen TUI | `?1049h` (near-zero FP) | **Solid** — fully alt-screen | **GAP** — no plugin `parsePrompt`; layout (numbered vs row) unverified | arrow + `enter`; `esc` cancel (no plugin scheme yet → default may misnav) | ACP `session/request_permission` optionIds (allow_once/always/reject_once) — STRONG, emerging std |
| **opencode** | full-screen bubbletea (alt-screen) | `?1049h` (near-zero FP) | **Solid** — alt-screen | **Solid (plugin)** — horizontal row, bespoke `parsePrompt` | **`left`/`right` + `enter`** (NOT down); "always" is multi-step; default down-nav is WRONG | local server + plugin `tool.execute.before` hook (already installs a plugin) — MODERATE, cheapest |

### Headline conclusions
1. **All three are full-screen alternate-screen TUIs**, so the generic detector's
   high-precision `?1049h` branch covers DETECTION for every one of them with near-zero false
   positives. The harder inline `?25l + Motion::Still + NOT ?2004` branch is the backstop if
   any vendor moves a confirmation inline, and is what generalizes to opaque non-agent TUIs.
2. **Detection ≠ extraction.** The gaps are all in *reading the option grid*, not in *knowing
   a picker is up*:
   - codex: numbered list — generic extraction works; plugin adds skip-safety.
   - opencode: **horizontal row breaks generic numbered-list extraction** — already solved by
     the plugin's `parsePrompt`; this is the canonical "plugin owns format" case.
   - cursor: **no plugin parser yet** + layout unverified — the one provider that may need a
     new `parsePrompt`/`buildPromptResponse`. Highest-priority gap to close.
3. **Send-back is not uniform.** Default = vertical `down × N + Enter`. That is correct for
   codex, plausibly wrong for opencode (**horizontal `left/right`**) and unverified for cursor.
   Wiring facet9 should let each plugin override the response builder (codex/claude already do
   via `buildPromptResponse`); opencode and cursor need theirs to avoid mis-navigation.
4. **Quirks the detector must respect (CLASSIFY, not detection):** auth prompts (codex login)
   surface-only; codex self-update default = "Update now" → never blind-Enter; opencode
   "Always allow" is multi-step; cursor workspace-trust is security-sensitive (surface/ask
   unless an explicit policy opts in). These reinforce facet9's default-deny + hard-deny guards.
5. **Structured non-MCP channels exist for all three** (codex app-server, cursor ACP,
   opencode plugin/server) and cleanly cover the APPROVAL class only — never `/model`, menus,
   trust, or free-form questions. They remain OPTIONAL bonuses; the generic detector must (and
   does, for detection) stand alone. Cheapest to adopt: opencode (plugin already installed);
   strongest/most-standard: cursor ACP and codex app-server, both heavyweight (distinct
   run-mode vs the interactive TUI CodeTerm currently binds).

<!-- revision: 1 -->
