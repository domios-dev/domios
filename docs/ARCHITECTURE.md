# CodeTerm Architecture

## System Overview

```
┌─────────────────────────────────────────────────────────┐
│  FRONTEND (React + TypeScript)                          │
│  Terminal (xterm.js) · ChatView · Settings · Sidebar    │
│  ↕ Tauri IPC (invoke/listen)                            │
├─────────────────────────────────────────────────────────┤
│  SERVICE LAYER (Rust)                                    │
│  ServiceContext — single entry point for all operations  │
│  ↕                                                       │
│  ┌─────────┐ ┌─────┐ ┌──────┐ ┌──────┐ ┌────────────┐ │
│  │Multiplex│ │ PTY │ │ Mesh │ │Bridge│ │Agent Detect│ │
│  │  State  │ │Mgr  │ │  Mgr │ │  Mgr │ │  Registry  │ │
│  └─────────┘ └─────┘ └──────┘ └──────┘ └────────────┘ │
├─────────────────────────────────────────────────────────┤
│  EXTERNAL INTERFACES                                     │
│  REST API (Axum :7685) · Socket (JSON-RPC) · CLI        │
└─────────────────────────────────────────────────────────┘
```

## Directory Map

```
src-tauri/src/
├── lib.rs                  — App initialization, state wiring
├── commands.rs             — Tauri IPC commands (frontend ↔ backend)
├── commands_mesh.rs        — Mesh-specific Tauri commands
├── shell.rs                — Platform shell syntax helpers
├── prompts.rs              — Compiled prompt templates loader
├── attachments.rs          — Shared temp-file storage (clipboard/drops/bridge), bucket dirs under %TEMP%/codeterm-<bucket>/, MaxBytes/MaxCount eviction
├── clipboard_files.rs      — Native OS clipboard file-reference reader (Win CF_HDROP, macOS NSPasteboardTypeFileURL, Linux stub)
│
├── dispatch/               — UNIFIED COMMAND DISPATCH (Phase B foundation, not yet wired to adapters)
│   └── mod.rs              — Command enum: Health, TabList, SendText, GetOutput, GetScreen
│
├── service/                — UNIFIED BUSINESS LOGIC (all ops go through here)
│   ├── mod.rs              — ServiceContext (god object, needs facade split)
│   ├── workspace.rs        — Create/rename/close workspaces
│   ├── tab.rs             — Tab CRUD, focus, rename
│   ├── pty_io.rs           — send_text, batch operations
│   ├── pty_lifecycle.rs    — Spawn/resize/kill PTY, backend reader
│   ├── hierarchy.rs        — Agent roles, reports, status
│   ├── orchestrator.rs     — Agent spawn, batch commands
│   ├── general_agent.rs    — GA workspace setup & watchdog
│   ├── cron.rs             — Cron task execution
│   ├── notification.rs     — OS toast notifications
│   └── output.rs           — Output processing pipeline
│
├── multiplexer/            — WORKSPACE/TAB STATE MACHINE
│   ├── state.rs            — Central state (65KB — needs split)
│   ├── types.rs            — Workspace, Tab, Tab (one tab per tab)
│   └── commands.rs         — Thin Tauri wrappers
│
├── pty/                    — PTY LIFECYCLE
│   ├── manager.rs          — Spawn, kill, output buffering
│   ├── write_queue.rs      — Batched PTY writes
│   ├── screen_state.rs     — VT100 screen content tracking
│   └── shell_integration.rs — OSC 7 CWD parsing
│
├── agent/                  — AGENT DETECTION & PROVIDERS
│   ├── provider.rs         — AgentProvider trait (14 methods)
│   ├── registry.rs         — AgentRegistry (HashMap<id, Box<dyn AgentProvider>>)
│   ├── claude.rs           — Claude provider implementation
│   ├── detector.rs         — Agent event detection (41KB — needs refactor)
│   ├── status.rs           — Idle detection, status tracking
│   └── prompt.rs           — Permission prompt handling
│
├── mesh/                   — PEER-TO-PEER NETWORKING
│   ├── mod.rs              — MeshManager facade
│   ├── identity.rs         — Ed25519 signing
│   ├── peers.rs            — Persistent peer storage (JSON)
│   ├── discovery.rs        — mDNS + UDP broadcast
│   ├── pairing.rs          — Trust verification
│   ├── transport.rs        — HTTP relay between peers
│   ├── crown.rs            — Coordinator election
│   └── types.rs            — Peer/request types
│
├── bridge/                 — MESSAGING INTEGRATIONS
│   ├── manager.rs          — BridgeManager orchestrator
│   ├── telegram.rs         — Telegram bot handler
│   └── traits.rs           — MessagingService trait
│
├── api/                    — REST API (Axum)
│   ├── routes.rs           — All HTTP endpoints (2225 LOC — needs split)
│   └── mesh_routes.rs      — Mesh-specific endpoints
│
├── socket/                 — SOCKET API (JSON-RPC)
│   └── server.rs           — Named pipe/Unix socket (974 LOC)
│
├── cron/                   — CRON SCHEDULER
│   ├── scheduler.rs        — tokio-based execution
│   └── store.rs            — Persistent job storage
│
├── config/                 — CONFIGURATION
│   ├── mod.rs              — TOML loading
│   └── schema.rs           — Serde schema with defaults
│
└── update/                 — AUTO-UPDATER
    └── mod.rs              — Version check logic

src/                        — FRONTEND (React)
├── App.tsx                 — Root, theme application
├── components/
│   ├── Terminal/           — TerminalTab (44KB), TabContent (per-tab wrapper)
│   ├── ChatView/           — Agent interaction UI, message parser
│   ├── Settings/           — SettingsTabl, MeshSection
│   ├── Sidebar/            — Workspace/tab/tab navigation
│   ├── CronTabl/          — Cron job management UI
│   ├── Onboarding/         — First-run setup
│   └── shared/             — DropdownMenu, common components
├── store/
│   ├── multiplexer.tsx     — useReducer for mux state (39KB)
│   └── crons.tsx           — Cron job state
├── hooks/                  — useConfig, custom hooks
└── lib/
    ├── types.ts            — TypeScript types (mirrors Rust)
    ├── agentProviders/     — Frontend agent provider abstraction
    └── completions/        — Tab completion engine

cli/                        — CLI TOOL
├── src/main.rs             — 40+ subcommands (82KB)
├── src/client.rs           — HTTP & socket client
└── src/mesh_cli.rs         — Mesh peer operations

src-tauri/prompts/          — AGENT PROMPT TEMPLATES (compiled into binary)
├── general_agent.md        — General Agent system prompt
├── orchestrator_suffix.md  — Appended to Orchestrator tasks
├── worker_suffix.md        — Appended to Worker tasks
├── codeterm_rule.md        — Installed to ~/.claude/rules/
├── bootstrap_instructions.md — First-run onboarding
└── cron_complete_suffix.md — Appended to cron tasks
```

## Known Issues & Tech Debt

### Critical (address first)
1. **Three-way API duplication** — REST (2225 LOC), Socket (974 LOC), Tauri commands (1547 LOC) all implement same ~40 operations. ~1500 LOC can be eliminated with shared dispatch.
2. **detector.rs god module** (41KB) — Mixes OSC parsing, VT100 state, agent detection, idle detection. Should split into focused modules delegating to AgentProvider.

### Medium
3. **ServiceContext god object** — All modules see all state. Needs facade split (MuxContext, AgentContext, BridgeContext).
4. **Hard-coded values** — Output buffer 65536, auto-edit 60s, agent timeout 30s, etc. Should move to config.
5. **multiplexer/state.rs** (65KB) — Contains all manipulation logic. Split into state + operations.
6. **CLI feature parity burden** — 40+ subcommands maintained manually. Could generate from API spec.

### Low
7. **Frontend state sync** — React has local copy of mux state, syncs via events. Could use optimistic updates or WebSocket push.
8. **Mesh persistence** — JSON files with no atomic writes. Should use write-tmp-then-rename.

### Known: ConPTY double cursor on Windows
ConPTY historically strips/modifies DECTCEM escape sequences (`\x1b[?25l` / `\x1b[?25h`). TUI apps like Claude Code hide xterm's cursor via DECTCEM and draw their own — if ConPTY swallows the hide, both cursors are visible. Fixed in Windows Terminal v1.0+ (PR #4902), but older Windows 10 builds may still exhibit this. Screen restore now uses `contents_formatted()` + `cursor_state_formatted()` which preserves cursor visibility state. If the issue returns during live sessions (not restore), the fix is: detect alternate screen enter (`\x1b[?1049h`) → force `\x1b[?25l` client-side; detect exit (`\x1b[?1049l`) → force `\x1b[?25h`. References: [xterm.js #4228](https://github.com/xtermjs/xterm.js/issues/4228), [Windows Terminal #3093](https://github.com/microsoft/terminal/issues/3093), [xterm.js #3513](https://github.com/xtermjs/xterm.js/issues/3513).

## Communication Patterns

All three entry points delegate to ServiceContext:

```
Frontend → invoke() → commands.rs → ServiceContext::method()
CLI      → HTTP     → routes.rs   → ServiceContext::method()
CLI      → Socket   → server.rs   → ServiceContext::method()
Mesh     → HTTP     → mesh_routes → ServiceContext::method()
```

This is correct architecture — ServiceContext is the single source of truth.
The duplication is in the entry point glue code, not business logic.

## Lock Ordering

All parking_lot Mutexes acquired in this order (see CLAUDE.md):

1. `mux` (MultiplexerState)
2. `output_buf` (OutputBuffer)
3. `screen_state` (ScreenState)
4. `agent_tracker` (TabStatusTracker)
5. `pty` (PtyManager)
