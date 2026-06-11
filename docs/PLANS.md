# CodeTerm — Future Plans
nPlans live here and only here — no plan files at repo root.

## Index

- [Plugin Architecture](plans/plugin-architecture.md) — core + plugins, MCP agent interface, unified dispatch, 6-phase roadmap
- [Plugin Authoring Guide](PLUGINS.md) — how to author a plugin given what exists in code today (manifest, `Plugin` trait, lifecycle, events)
- [Browser Improvements](plans/browser_improvements.md) — WebView2/WKWebView, CDP control, agent screenshots
- [Whiteboard Plugin](plans/whiteboard-plugin.md) — tldraw canvas, agents draw/read diagrams, MCP tools
- [Secret Store](plans/secrets-store.md) — pluggable backend (Bitwarden default), OS-keychain persistence, `codeterm mem secret` CLI
- [Naming Research](plans/naming-research.md) — rebrand candidates, trademark checks, available names

---

## Claude Code Channels Integration (waiting for stable release)

**Status:** On hold — Channels API is research preview (`--dangerously-load-development-channels`, claude.ai auth only). Will implement when stable.

**Date:** 2026-03-25

### Why

Current agent reporting uses PTY injection (`[Report from X] msg\r` written to parent's terminal). Works but:
- Text mixed with terminal output
- Timing-sensitive (Update Available, etc.)
- No structured data

Channels push structured MCP events directly into Claude session — cleaner, two-way.

### Architecture

```
Current:  Worker → codeterm agent report → socket → PTY write to parent
Channels: Worker → codeterm agent report → socket → HTTP POST to channel server → MCP push → Claude session
```

PTY injection stays as fallback. Channel mode is opt-in via config.

### 5 Phases

**Phase 1: MCP Channel Server** (`channel-server/server.ts`, ~200 lines Bun/TS)
- MCP server declaring `claude/channel` capability
- Localhost HTTP listener for CodeTerm reports
- Pushes structured `<channel source="codeterm" agent_id="..." status="...">` events
- Reply tool `codeterm_command` for two-way communication

**Phase 2: Backend Changes**
- `ChannelRegistry` in ServiceContext (pane_id → channel endpoint)
- `submit_report()` tries channel first, fallback PTY
- `channel.register/unregister` socket + REST methods
- Config: `agent.channel_mode = "auto" | "off" | "force"`
- `build_agent_command()` adds `--dangerously-load-development-channels` flag

**Phase 3: CLI**
- `codeterm channel status [--pane ID]`

**Phase 4: Skill/Docs**
- Document channel mode in SKILL.md

**Phase 5: Two-Way MCP Tools** (bonus)
- `codeterm_command(method, params)` — Claude calls CodeTerm API via MCP, no shell needed
- Replaces CLI-based interaction entirely

### Risks
- API will change (preview)
- Only claude.ai auth (not API key)
- Bun/Node dependency for channel server
- PTY fallback guarantees no regression

### References
- https://code.claude.com/docs/en/channels
- https://code.claude.com/docs/en/channels-reference
- Research completed 2026-03-25 — full architecture in conversation history

<!-- revision: 1 -->
