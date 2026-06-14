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

### 3. Detection keys off the pane's OWN session

Provider detection (model, session id, status, usage) reads only the record belonging to *this pane's claimed session id*. Never "latest global", "newest message for cwd", or "most recent file in the sessions dir" — those roll one pane onto another pane's session and report a foreign model/status. If the pane has no claimed session yet, return `None` (honest unknown) rather than guessing from a neighbor. This is a bug class we have shipped: codex panes reported a sibling's model, opencode read the globally-latest assistant model instead of the pane's own. The fix is always: scope the read to the pane's session id, drop the fallback.

### 4. Audit the host-API surface when it grows

Every host function a JS drop-in can call is a confinement surface. Drop-in plugins run with a sandboxed view of FS/sqlite/env (see the host-API confinement in `src-tauri/src/plugin/`); each new host function widens what an untrusted drop-in can reach, so justify it against that sandbox before adding it. Also remember the runtimes are not symmetric: the daemon's QuickJS lacks bindings the native Rust path takes for granted (`console`, parts of the standard library), so a host helper that "obviously works" natively can throw in the drop-in runtime — guard accordingly (`typeof console !== "undefined"`).

## Detection: structural patterns, not literal strings

Detection code (prompts, task completion, agent identification, output parsing) must use structural fingerprints, not literal text. If Claude renames "Do you want to proceed?" to "Do you want to create?", detection should still work — match the structure (numbered options + footer), not the wording. Vendors change copy without warning; structure is stable. Generalize, don't enumerate.

"Structural" means detection survives any single vendor copy change. Each provider
keeps a signal list where each signal has a weight, and detection fires only when
the score reaches the provider threshold. No single literal string may carry more
than half of that threshold. Signal kinds, strongest first:

- process/title patterns
- UI glyphs such as `✱` / `✻` spinner characters
- layout shapes such as footer lines with multiple segments or prompt-character columns
- literal copy strings, which are weak signals and never the sole gate

<!-- revision: 5 -->
