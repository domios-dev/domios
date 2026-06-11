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

<!-- revision: 3 -->
