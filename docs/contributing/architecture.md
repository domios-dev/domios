# Architecture: agent providers, multi-agent design, detection

## Agent providers

Agent detection, lifecycle, session handling, command shaping, and schema registration are abstracted behind the `AgentProvider` trait in `crate::agent::provider`.

### Canonical provider layout (native target shape)

The canonical native implementation uses a facade that delegates to focused submodules:

- `detection`: title/output/tui/noise/event detection and prompt parsing
- `sessions`: session enumeration, session-id detection, session parsing
- `launch`: launch/resume command shaping
- `schemas`: models/settings/integration settings/notification schema ownership
- `integration` and `usage` as optional adjacent modules where needed

Current evidence from implemented providers:
- `agent_claude/provider/mod.rs` follows this shape and delegates directly to
  `detection`, `sessions`, `launch`, `schemas`, `integration`, and `usage`.
- `agent_codex/provider.rs` and `agent_opencode/provider.rs` are single-file
  implementations with helper functions (`models` module for Opencode only), and
  are candidates for decomposition toward the canonical layout.
- `js_runtime/provider.rs` is a plugin bridge: it reads manifest defaults,
  dispatches most behavior through JS (`detectFromTitle`, `parseSessionMessages`,
  etc.), and falls back to Rust helpers when the JS method is not implemented.

Provider tree sketch (current Claude pattern):

```
src-tauri/src/plugin/agent_claude/provider/
├─ mod.rs
├─ detection.rs
├─ integration.rs
├─ launch.rs
├─ schemas.rs
├─ sessions.rs
└─ usage.rs
```

### Native Rust vs JS plugin responsibilities

- Native Rust should own shared host concerns and platform-sensitive behavior:
  model catalog registration, provider metadata normalization, registry wiring,
  filesystem/DB probing, and non-trivial launch quoting/cross-platform command
  composition.
- JS plugins should own provider-specific heuristics and parser behavior in plugin
  code, surfaced through `PluginManifest` and exported plugin methods.
- `src-tauri/src/plugin/js_runtime/provider.rs` is authoritative glue for this split:
  manifest fields provide defaults, `JsProvider` tries JS methods first, and falls
  back to Rust logic when the JS layer omits methods.

### Layout constraints and parity contract

- Provider size constraints:
  - no file should exceed `1000` lines
  - target provider main file size is `800` lines
- JS/native sync contract:
  - `src-tauri/src/plugin/js_runtime/parity_test.rs` encodes the exact parity
    behaviors between native providers and JS providers (detection, parse/metadata,
    launch/resume command shape, session and integration behaviors) for the covered
    surfaces.

## Multi-agent mindset

**Always design for multiple agent types, not just Claude.** Every feature that interacts with agents must go through the AgentProvider abstraction. Never hardcode Claude-specific logic in generic code. If a behavior is Claude-specific (clipboard paste for images, JSONL session format, TUI noise patterns), it belongs in ClaudeProvider or the Claude provider config — not in ChatView, TerminalPane, or other generic components.

When adding agent-related features, ask: "How would this work with Codex? With Aider? With a local model?" If the answer requires Claude-specific code, put it in the provider.

## Plugin & capability discipline

The provider boundary is the project's load-bearing seam: native providers, JS drop-ins, and the core all meet here. It stays clean only if every change crossing it pays the same four-part tax. CLAUDE.md states the rules tersely; this is the why.

### 1. Core asks capabilities, never identities

Core code branches on a *capability*, never on a provider id or display string. Examples that are correct: `provider.supports_chat_ui()` gates the chat-UI default; `provider.available_models()` feeds spawn-time model validation; `provider.parse_interactive_events(&msg)` (#162) lets each provider surface its own answerable prompts (Claude's AskUserQuestion, etc.) out of the chat stream — core enriches every message through the capability and never knows the tool exists; a future `model_switch_strategy()` picks slash-command vs restart. Examples that are leaks: `if agent_cmd == "codex"`, matching `"OpenCode Zen"` in a generic component. A hardcoded id list means the next provider (or a JS drop-in) silently gets the wrong behavior, and nobody added it to the list. If you need per-provider behavior, add a capability method to the trait and let each provider answer for itself. The matching provider-side parser keys off the *tool name + arg shape* (a structural fingerprint), never display copy — see Detection below.

### 2. Native + JS parity in lockstep

Any `AgentProvider` method a JS drop-in can implement must gain a case in `src-tauri/src/plugin/js_runtime/parity_test.rs` that runs the native and JS paths over the same fixtures and asserts they agree. Without it, a capability that works natively drifts on drop-ins and no test notices — until a user does. A real instance: a JS validation helper used `console.warn`, which has no binding in the daemon's QuickJS, so a `ReferenceError` aborted parsing and the JS path silently returned zero results while native returned correct ones; parity tests caught it once they existed.

### 3. Tab→session binding is provider-reported only; reads key off the Tab's OWN session

**Binding (who owns which session id) is set only from a provider-reported report or an explicit user-preselected launch/resume id — never inferred.** Each provider reports its session id from *inside the running agent process* (claude/codex `SessionStart` hooks, the opencode `@opencode-ai/plugin` event hook), POSTing to `POST /api/panes/:pane_id/hook/session_start`. The report carries the Tab id (in the URL, from `CODETERM_PANE_ID`) and a per-launch nonce (`CODETERM_SESSION_BINDING_NONCE`, echoed as the `X-CodeTerm-Binding-Nonce` header). The route (`api/hook_routes.rs::bind_reported_session`) validates the provider matches the Tab, rejects a session id already claimed by another Tab (atomic, under the mux lock), and rejects a report whose nonce mismatches the Tab's launch nonce (a stale/foreign/relaunch claim); an absent nonce is tolerated so a not-yet-reinstalled hook keeps binding. A Tab with no report is honestly *pending* — never bound to a guess.

Because the report runs in the Tab's own process carrying that Tab's id + nonce, a report can only ever bind the Tab it came from. Two agents in the *same cwd* therefore each bind their own session — the case filesystem "newest for cwd" detection could never get right. The whole filesystem-guessing detector family (`detect_session_id` as an assigner, `detect_started_session`, `detect_launched_session_id`, the `kick_off_session_detection` retry loop) has been **removed**; `set_agent_session_id` and the hook route are the only paths that write `pane.agent_session_id`. Stress-tested by `evals/binding-matrix/` (mass spawn into same-cwd clusters + `/new` + close/respawn → every Tab bound + unique).

**Reads (model, status, usage, transcript) scope to the Tab's already-bound session id.** Never "latest global", "newest message for cwd", or "most recent file in the sessions dir" — those roll one Tab onto another's session and report a foreign model/status. If the Tab is still pending, return `None` (honest unknown), not a neighbor's value. The only surviving detector that may *follow* a session change is `detect_active_session` (provider-reported model-roll, e.g. opencode `/model` swap), and it is guarded against stealing a sibling's id. Filesystem session enumeration (`detect_session_id`) survives only as a read-only feed for the session browser — it must never assign a binding.

### 4. Audit the host-API surface when it grows

Every host function a JS drop-in can call is a confinement surface. Drop-in plugins run with a sandboxed view of FS/sqlite/env (see the host-API confinement in `src-tauri/src/plugin/`); each new host function widens what an untrusted drop-in can reach, so justify it against that sandbox before adding it. Also remember the runtimes are not symmetric: the daemon's QuickJS lacks bindings the native Rust path takes for granted (`console`, parts of the standard library), so a host helper that "obviously works" natively can throw in the drop-in runtime — guard accordingly (`typeof console !== "undefined"`).

## Detection: structural patterns, not literal strings

Detection code (prompts, task completion, agent identification, output parsing) must use structural fingerprints, not literal text. If Claude renames "Do you want to proceed?" to "Do you want to create?", detection should still work — match the structure (numbered options + footer), not the wording. Vendors change copy without warning; structure is stable. Generalize, don't enumerate.

Output parsing can classify activity and emit diagnostics, but it never clears Tab identity. Only explicit PTY, process, or authenticated session lifecycle transitions may clear the provider, tab-owned session, binding nonces, and Planner binding. Session snapshots preserve that tab-owned identity and its live model/state/boot projection for the same restored session. A Team with an active plan renders that plan's title as the visible and accessible effort title and suppresses Team rename; the persistent Team name and its rename action are planless identity.

"Structural" means detection survives any single vendor copy change. Each provider
keeps a signal list where each signal has a weight, and detection fires only when
the score reaches the provider threshold. No single literal string may carry more
than half of that threshold. Signal kinds, strongest first:

- process/title patterns
- UI glyphs such as `✱` / `✻` spinner characters
- layout shapes such as footer lines with multiple segments or prompt-character columns
- literal copy strings, which are weak signals and never the sole gate

<!-- revision: 9 -->
