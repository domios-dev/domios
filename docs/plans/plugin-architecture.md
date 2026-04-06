# Plugin Architecture Plan

**Status:** Phase 1 structurally done, Phase 2 not started
**Date:** 2026-04-05 (last status update: 2026-04-06)

## Status (2026-04-06)

Code pointers for what actually exists today — match doc examples against these files, not against the aspirational Rust in later sections.

- **Unified dispatch:** `src-tauri/src/dispatch/mod.rs` — `Command` enum + `dispatch()`. REST/Socket/Tauri adapters call into it for `health`, `pane.list`, `send_text`, `get_output`, `get_pane_screen(_formatted)`, and the pane ops batch (`split`, `close`, `focus`, `rename`).
- **Plugin skeleton:** `src-tauri/src/plugin/mod.rs` — `PluginManifest`, `PluginMeta`, `PluginCapabilities`, `PluginPermissions`, `PluginActivation`, `PluginPrompt`, `Plugin` trait, `PluginManager`, `PluginState`, `SkillFile`. Manifest parser, directory discovery, and native plugin attach/activate/deactivate all work and are covered by unit tests.
- **Typed event bus:** `src-tauri/src/api/events.rs` — `SystemEvent` enum (see "Currently-published events" in `docs/PLUGINS.md`), in-process `tokio::broadcast` channel in `EventBroadcaster`. `ServiceContext::emit_state_changed` / `emit_pane_title_pinned` in `src-tauri/src/service/mod.rs` are the only publish sites today.
- **Agent provider trait:** `src-tauri/src/agent/provider.rs` — `AgentProvider` has been expanded with the plugin-facing surface (`install_skills`, `uninstall_skills`, `supports_lazy_skills`, `auto_approve_flags`, `compact_command`, `model_switch_command`, `exit_command`). Nothing has been moved out of `src-tauri/src/agent/claude.rs` yet; the trait shape in this document (later sections) is aspirational and in places richer than what is implemented.

**Phase 1 — Unified Dispatch + Plugin Manager skeleton:** DONE structurally, modulo extraction of the first plugin.
**Phase 2 — Agent Provider extraction + Skill system:** NOT STARTED. Trait surface expanded only.
**Phase 3+ (memory, WASM, bridges):** not started.

A developer-facing plugin authoring guide matching what exists *today* lives at `docs/PLUGINS.md`. The rest of this document describes the full target design and should be read as forward-looking.

## Core Architecture

```
╔══════════════════════════════════════════════════════════════════════╗
║                        AGENTS (any provider)                        ║
║         Claude · Codex · Gemini · Local LLMs · others              ║
╚══════════════════╤══════════════════════════════════╤════════════════╝
                   │ MCP (JSON-RPC stdio)             │ PTY I/O
                   ▼                                  ▼
╔══════════════════════════════════════════════════════════════════════╗
║                          PLUGIN HOST                                ║
║                                                                      ║
║  ┌─────────────────────────────────────────────────────────────────┐ ║
║  │                    BUNDLED PLUGINS                               │ ║
║  │                                                                  │ ║
║  │  ┌──────────────┐ ┌──────────────┐ ┌────────────────────────┐  │ ║
║  │  │ terminal     │ │ agent-claude │ │ bridge-telegram        │  │ ║
║  │  │ (native)     │ │ (native)     │ │ (process, Python)      │  │ ║
║  │  │              │ │              │ │                         │  │ ║
║  │  │ xterm.js on  │ │ detect OSC   │ │ bot polling → PTY      │  │ ║
║  │  │ Win/Linux    │ │ parse JSONL  │ │ outbound channel       │  │ ║
║  │  │ cmux on mac? │ │ prompt parse │ │ auto-edit, streaming   │  │ ║
║  │  └──────────────┘ └──────────────┘ └────────────────────────┘  │ ║
║  │                                                                  │ ║
║  │  ┌──────────────┐ ┌──────────────┐ ┌────────────────────────┐  │ ║
║  │  │ browser      │ │ memory       │ │ code-index             │  │ ║
║  │  │ (native)     │ │ (process)    │ │ (process)              │  │ ║
║  │  │              │ │              │ │                         │  │ ║
║  │  │ WebView2/WK  │ │ SQLite+VSS  │ │ tree-sitter AST        │  │ ║
║  │  │ CDP control  │ │ embeddings   │ │ project map            │  │ ║
║  │  │ screenshots  │ │ mesh-synced  │ │ semantic search        │  │ ║
║  │  └──────────────┘ └──────────────┘ └────────────────────────┘  │ ║
║  └─────────────────────────────────────────────────────────────────┘ ║
║                                                                      ║
║  ┌─────────────────────────────────────────────────────────────────┐ ║
║  │                   COMMUNITY PLUGINS                              │ ║
║  │                                                                  │ ║
║  │  ┌──────────────┐ ┌──────────────┐ ┌────────────────────────┐  │ ║
║  │  │ agent-codex  │ │ bridge-slack │ │ knowledge-graph        │  │ ║
║  │  │ (native)     │ │ (process)    │ │ (wasm, sandboxed)      │  │ ║
║  │  └──────────────┘ └──────────────┘ └────────────────────────┘  │ ║
║  └─────────────────────────────────────────────────────────────────┘ ║
║                                                                      ║
║  Plugin Manager: discover · load · activate · deactivate · call      ║
║  Event Bus: publish/subscribe for inter-plugin communication         ║
╚══════════════════╤═══════════════════════════════════════════════════╝
                   │ Plugin API (tools, hooks, commands, settings, UI)
                   ▼
╔══════════════════════════════════════════════════════════════════════╗
║                           CORE (spinal cord)                        ║
║                                                                      ║
║  ┌────────────┐  ┌──────────────┐  ┌──────────────┐  ┌──────────┐ ║
║  │ Multiplexer│  │ PTY Manager  │  │ Mesh         │  │ Config   │ ║
║  │            │  │              │  │ Transport    │  │          │ ║
║  │ workspaces │  │ spawn/kill   │  │ discovery    │  │ TOML     │ ║
║  │ tabs/panes │  │ read/write   │  │ pairing      │  │ schema   │ ║
║  │ splits     │  │ resize       │  │ signed relay │  │ validate │ ║
║  │ focus      │  │ output buf   │  │ peer state   │  │ persist  │ ║
║  └────────────┘  └──────────────┘  └──────────────┘  └──────────┘ ║
║                                                                      ║
║  ┌────────────┐  ┌──────────────┐  ┌──────────────┐  ┌──────────┐ ║
║  │ Plugin Mgr │  │ Event Bus    │  │ Auth &       │  │ Unified  │ ║
║  │            │  │              │  │ Permissions  │  │ Dispatch │ ║
║  │ manifest   │  │ pub/sub      │  │ API tokens   │  │          │ ║
║  │ lifecycle  │  │ system evts  │  │ mesh signing │  │ REST     │ ║
║  │ sandboxing │  │ plugin evts  │  │ plugin perms │  │ Socket   │ ║
║  │ MCP server │  │ hooks        │  │              │  │ Tauri    │ ║
║  └────────────┘  └──────────────┘  └──────────────┘  └──────────┘ ║
║                                                                      ║
║  ServiceContext — single entry point for all operations              ║
╚══════════════════════════════════════════════════════════════════════╝
                   │
                   ▼
╔══════════════════════════════════════════════════════════════════════╗
║                        EXTERNAL INTERFACES                          ║
║                                                                      ║
║  REST API (:7685)  ·  Socket (JSON-RPC)  ·  CLI  ·  MCP (optional) ║
║                                                                      ║
║  CLI + Skills = primary agent interface                              ║
║  MCP = optional wrapper for ecosystem (Cursor, Windsurf, etc.)      ║
║  All generated from Unified Dispatch — no duplication               ║
╚══════════════════════════════════════════════════════════════════════╝
```

## Platform-Specific Plugins

Plugins declare which platforms they support:

```toml
[plugin]
name = "terminal-cmux"
tier = "native"
platforms = ["macos"]              # only macOS

[plugin]
name = "bridge-telegram"
tier = "process"
platforms = ["windows", "macos", "linux"]  # all platforms

[plugin]
name = "gpu-renderer"
tier = "native"
platforms = ["linux"]              # Vulkan-only for now
```

Plugin Manager skips plugins that don't match current platform. `platforms` field is optional — omit for universal plugins.

## Plugin Tiers

### Native (compiled into binary)
- Rust crates, full ServiceContext access
- Used for: terminal renderers, agent providers, pane types
- Ship with CodeTerm, can't be removed (bundled) or can be installed (community)
- Fastest — zero IPC overhead

### Process (long-running, JSON-RPC stdio)
- Any language, same protocol as MCP/LSP/DAP
- Used for: memory, code-index, bridges, automation
- Plugin Manager spawns, monitors health, restarts on crash
- Agents call tools directly via MCP — no CLI overhead

### WASM (sandboxed third-party)
- Compiled to `wasm32-wasip2`, loaded via wasmtime/Extism
- Restricted: filesystem allowlist, no network unless granted, epoch preemption
- Used for: formatters, linters, transformers, community extensions
- Safe to install from marketplace

## Agent Provider as Plugin

Currently `agent-claude` is hard-wired in `src-tauri/src/agent/`. Becomes a native plugin. Each new agent type (Codex, Gemini CLI, etc.) is a separate plugin implementing the same trait.

### Full AgentProvider trait

Everything agent-specific lives in the provider. Core only calls trait methods.

```rust
pub trait AgentProvider: Send + Sync {
    // ── Identity ──
    fn id(&self) -> &str;                        // "claude", "codex"
    fn display_name(&self) -> &str;              // "Claude Code", "Codex CLI"

    // ── Launch & lifecycle ──
    fn build_launch_command(&self, params: &AgentLaunchParams) -> String;
    fn build_resume_command(&self, session_id: &str, skip_permissions: bool) -> Option<String>;
    fn readiness_strategy(&self) -> ReadinessStrategy;
    fn task_delivery(&self) -> TaskDelivery;
    fn auto_approve_flags(&self) -> Vec<String>;  // ["--dangerously-skip-permissions"] or ["--approval-mode", "full-auto"]

    // ── Detection ──
    fn detect_from_title(&self, title: &str) -> bool;
    fn detect_from_output(&self, text: &str) -> bool;
    fn has_event_markers(&self, chunk: &str) -> bool;
    fn detect_events(&self, text: &str) -> Vec<AgentEvent>;
    fn parse_prompt(&self, text: &str) -> Option<PermissionPrompt>;

    // ── TUI interaction ──
    fn build_prompt_response(&self, option_index: usize) -> Vec<Vec<u8>>;  // Claude: j+Enter, others: arrow+Enter
    fn is_output_noise(&self, text: &str) -> bool;
    fn status_bar_rows(&self) -> u16;

    // ── Session management ──
    fn session_file_path(&self, cwd: &str, session_id: &str) -> Option<PathBuf>;
    fn detect_session_id(&self, cwd: &str, exclude: &[String], max_age: Option<Duration>) -> Option<String>;

    // ── Agent-specific commands ──
    fn compact_command(&self) -> Option<&str>;       // Claude: "/compact", Codex: TBD
    fn model_switch_command(&self, model: &str) -> Option<String>;  // Claude: "/model sonnet", Codex: "--model gpt-4"
    fn exit_command(&self) -> Option<&str>;          // Claude: "/exit", Codex: TBD

    // ── Skill/prompt delivery ──
    fn install_plugin_skill(&self, plugin: &PluginManifest) -> Result<()>;  // Claude: ~/.claude/skills/
    fn uninstall_plugin_skill(&self, plugin_id: &str) -> Result<()>;
    fn deliver_prompt(&self, role: AgentRole, prompt: &str) -> Result<()>;  // Claude: CLAUDE.md, Codex: --system-prompt
    fn supports_lazy_skills(&self) -> bool;          // Claude: yes (Skill tool), others: maybe not

    // ── Output parsing ──
    fn parse_chat_message(&self, output: &str) -> Option<ChatMessage>;  // for ChatView rendering
    fn session_parser(&self) -> Option<Box<dyn SessionParser>>;  // JSONL parser for Claude, TBD for others
}
```

### What each provider owns (Claude vs Codex example)

| Concern | Claude (`agent-claude`) | Codex (`agent-codex`) |
|---------|------------------------|----------------------|
| **Launch** | `claude --dangerously-skip-permissions` | `codex --approval-mode full-auto` |
| **Readiness** | PollShellPrompt (30s) | TBD (different TUI) |
| **Task delivery** | InlineFlag (temp file) | PtyInput or flag |
| **Title detection** | Braille spinners `⠐`, `✳`, "claude" | Codex-specific title |
| **Output detection** | "Cogitated for", "Do you want to proceed" | Codex-specific markers |
| **Prompt navigation** | `j` (vim) + Enter | Arrow keys + Enter |
| **Auto-approve** | `--dangerously-skip-permissions` | `--approval-mode full-auto` |
| **Compact** | `/compact` | TBD |
| **Model switch** | `/model sonnet` | `--model gpt-4` or flag |
| **Exit** | `/exit` | Ctrl+C or command |
| **Noise filter** | "bypass permissions on", "shift+tab" | Different TUI chrome |
| **Status bar** | 3 rows | Different layout |
| **Session files** | `~/.claude/projects/{cwd}/` JSONL | Different path/format |
| **Skill delivery** | Files in `~/.claude/skills/` | System prompt or MCP |
| **Lazy skills** | Yes (Skill tool + triggers) | Maybe not |
| **Chat parser** | JSONL → messages/tool_calls/thinking | Different format |

### What moves out of core

- `src-tauri/src/agent/claude.rs` → `agent-claude` plugin
- `src-tauri/src/agent/detector.rs` hard-coded patterns → each provider's trait methods
- `src/components/ChatView/parser.ts` Claude-specific parsing → plugin contributes parser
- `src-tauri/prompts/` Claude-specific prompt sections → plugin's `on_prompt_build` hook

### What stays in core

- `AgentProvider` trait definition (the contract)
- `AgentRegistry` (populated by plugins, not hard-coded)
- PTY-level detection loop (calls into provider trait methods)
- Generic prompt/event types (`AgentEvent`, `PermissionPrompt`, `ChatMessage`)

## Plugin Capability Delivery per Provider

Plugins describe capabilities abstractly. Each AgentProvider translates them into the format its agent understands. Plugin doesn't know about Claude/Codex. Provider doesn't know about Memory/Whiteboard.

```
Plugin "memory" registers:
  commands: ["memory"]              # CLI subcommands
  skill_section: "memory.md"       # markdown instructions
  triggers: ["remember", "recall"]  # activation words

                    ↓ PluginManager passes to each active AgentProvider ↓

AgentProvider "claude":
  → Installs skill_section as ~/.claude/skills/codeterm-memory/SKILL.md
  → Triggers matched via Claude's Skill tool (lazy-load)
  → Agent calls: Bash → codeterm memory search "query"

AgentProvider "codex":
  → Generates tool definitions from CLI --help output
  → Or: wraps as MCP server (thin adapter)
  → Agent calls: shell tool → codeterm memory search "query"

AgentProvider "generic" (local LLMs, unknown agents):
  → Injects skill_section into system prompt directly
  → Agent calls: raw shell → codeterm memory search "query"
```

### What each provider implements

```rust
pub trait AgentProvider {
    // ... existing detection/launch methods ...

    /// How to deliver plugin capabilities to this agent type
    fn install_plugin_skill(&self, plugin: &PluginManifest) -> Result<()>;

    /// How to remove plugin capabilities
    fn uninstall_plugin_skill(&self, plugin_id: &str) -> Result<()>;

    /// Whether this provider supports lazy skill loading
    fn supports_lazy_skills(&self) -> bool;
}
```

### Delivery matrix

| Provider | Skill delivery | Lazy loading | Agent calls plugin via |
|----------|---------------|--------------|----------------------|
| Claude Code | `~/.claude/skills/` file | Yes (Skill tool + triggers) | Bash → `codeterm` CLI |
| Codex CLI | MCP thin wrapper or system prompt | Partial (`--tools` flag) | shell → `codeterm` CLI |
| Gemini | System prompt injection | No | shell → `codeterm` CLI |
| Local LLM | System prompt injection | No | shell → `codeterm` CLI |

All paths end at **`codeterm CLI → HTTP/Socket → ServiceContext`**. The CLI is the universal backend. Providers only differ in how they teach the agent about available commands.

## Prompt System: Roles × Providers × Plugins

Three dimensions determine what prompt an agent gets:

### Role (what the agent knows)

| Role | Gets | Knows about |
|------|------|-------------|
| **General Agent** | Full core prompt + ALL active plugin sections + `/codeterm` skill inlined | Everything: mesh, bridges, plugins, delegation, secrets |
| **Project Agent** | `/codeterm` skill (as file) + plugin skills lazy-loaded by triggers | Workspace tools, can spawn workers. No GA identity/routing. |
| **Worker** | `worker_suffix` only + plugin skills by triggers | Minimal. Task-scoped. Report and die. |

### Provider (how prompt is delivered)

Skills are a **universal format** — one markdown file, same content for every provider. No per-provider variants. Providers only differ in delivery mechanism:

| Provider | Delivery | Notes |
|----------|----------|-------|
| Claude Code | File in `~/.claude/skills/` | Native skill system, lazy-loaded by triggers |
| Codex CLI | Parsed and passed via `--system-prompt` or tools flag | Provider adapter converts |
| Gemini | Injected into system prompt | Provider reads skill file, prepends to conversation |
| Local LLM | Injected into system prompt | Same as Gemini |

```
Plugin "memory" provides ONE skill file:

prompts/
└── memory_skill.md    # universal — same for all providers
```

Each provider knows how to deliver it. New provider = new delivery adapter, not new skill files.

### Assembly flow

```
PluginManager.build_prompt(role) → String

role = GeneralAgent:
  1. Load core: prompts/general_agent.md
  2. If mesh enabled: + prompts/general_agent_mesh.md
  3. For each active plugin: + plugin.ga_section
  4. /codeterm skill INLINED (not separate)
  = Full GA system prompt
  → Provider.deliver_prompt(GeneralAgent, assembled_prompt)

role = ProjectAgent:
  1. /codeterm skill as standalone file
  2. Each active plugin's skill as standalone file
  → Provider.install_skills(skill_files)
  → Lazy-loaded by trigger words (if provider supports it)

role = Worker:
  1. worker_suffix.md appended to task
  2. Plugin skills available but only loaded if task triggers them
```

### Provider implements prompt delivery

```rust
pub trait AgentProvider {
    // ... detection, launch, etc ...

    /// Deliver assembled prompt to agent (provider knows how)
    fn deliver_prompt(&self, role: AgentRole, prompt: &str) -> Result<()>;

    /// Install plugin skills for this provider's agents
    fn install_skills(&self, skills: &[SkillFile]) -> Result<()>;

    /// Remove plugin skills
    fn uninstall_skills(&self, plugin_ids: &[&str]) -> Result<()>;

    /// Whether this provider supports lazy skill loading (triggers)
    fn supports_lazy_skills(&self) -> bool;
}
```

| Provider | `deliver_prompt` | `install_skills` | Lazy |
|----------|-----------------|-----------------|------|
| Claude | Write to GA's `CLAUDE.md` | Files in `~/.claude/skills/` | Yes |
| Codex | Pass `--system-prompt` | Merge into system prompt | No |
| Generic | Prepend to first message | Merge into system prompt | No |

## Unified Dispatch (kill the duplication)

Current: REST (2225 LOC) + Socket (974 LOC) + Tauri (1547 LOC) = ~4746 LOC for same ~40 operations.

Solution: single operation definition, multiple transports generated:

```rust
#[codeterm_operation]
pub async fn split_pane(ctx: &ServiceContext, pane_id: String, direction: String) -> Result<Pane> {
    ctx.split_pane(&pane_id, &direction)
}
// Macro generates:
// - REST handler: POST /api/panes/{pane_id}/split
// - Socket method: "split_pane"
// - Tauri command: #[tauri::command] split_pane
// - MCP tool: pane.split
// - CLI subcommand: codeterm pane split
```

Target: ~1500 LOC operation definitions + ~500 LOC transport glue = 2000 LOC total (vs 4746).

## Plugin Manifest (full spec)

```toml
[plugin]
name = "memory"
version = "0.1.0"
description = "Semantic memory with vector search"
tier = "process"                    # "native" | "process" | "wasm"
entrypoint = "python3 main.py"     # process plugins
wasm_module = "plugin.wasm"        # WASM plugins
platforms = ["windows", "macos", "linux"]  # optional, omit = all
min_codeterm = "0.10.0"            # minimum CodeTerm version

[capabilities]
tools = ["memory.save", "memory.search", "memory.recall", "memory.forget"]
commands = ["memory"]              # CLI subcommands
settings = true                    # has settings UI panel
hooks = ["on_agent_compact", "on_session_start", "on_prompt_build"]
pane_types = []                    # custom pane renderers
agent_providers = []               # agent type detection

[permissions]
filesystem = ["~/.codeterm/plugins/memory/data/"]
network = false                    # no outbound network
mesh_sync = true                   # participates in mesh sync

[activation]
triggers = ["on_startup"]          # "on_command:memory", "on_tool:memory.*", "lazy"

[mesh]
replicate_state = true             # state syncs across mesh peers
replicate_plugin = false           # don't auto-install on peers

[prompt]
# Markdown section appended to GA prompt when this plugin is active
ga_section = "prompts/ga_memory.md"
skill_section = "prompts/skill_memory.md"  # appended to /codeterm skill
```

## Event Bus

```rust
pub enum SystemEvent {
    // Core events
    PaneCreated { pane_id: String, pane_type: String },
    PaneDestroyed { pane_id: String },
    PaneResized { pane_id: String, cols: u16, rows: u16 },
    WorkspaceCreated { workspace_id: String },

    // Agent events
    AgentStarted { pane_id: String, provider: String },
    AgentCompacting { pane_id: String },
    AgentIdle { pane_id: String, duration_secs: u64 },
    AgentDied { pane_id: String },

    // Mesh events
    MeshPeerConnected { peer_id: String, name: String },
    MeshPeerDisconnected { peer_id: String },
    MeshSyncCompleted { peer_count: usize },

    // Bridge events
    MessageReceived { service: String, from: String, text: String },
    MessageSent { service: String, to: String },

    // Plugin events
    PluginActivated { plugin_id: String },
    PluginDeactivated { plugin_id: String },

    // Config events
    ConfigChanged { section: String, key: String },
}
```

Plugins subscribe via manifest (`hooks` field). Core emits. No polling.

## What Becomes a Plugin

| Current Module | Plugin | Tier | Platform | Priority |
|---------------|--------|------|----------|----------|
| Claude agent | `agent-claude` | Native | All | P1 |
| Telegram bridge | `bridge-telegram` | Process | All | P2 |
| Browser pane | `browser` | Native | All | P2 |
| Code indexing (new) | `code-index` | Process | All | P2 |
| Codex agent (new) | `agent-codex` | Native | All | P3 |
| Slack bridge (new) | `bridge-slack` | Process | All | P3 |

**Stays in core (never a plugin):**
- Terminal (xterm.js renderer + PTY I/O — too tightly coupled to extract)
- Multiplexer (workspaces, tabs, panes, splits)
- PTY Manager (spawn, read, write, resize)
- Mesh Transport (discovery, pairing, relay)
- Memory Store + Knowledge Graph (too fundamental, plugins depend on them)
- Skill Broker (agents can't function without it)
- Plugin Manager
- Event Bus
- Config System
- Unified Dispatch
- Auth & Permissions
- Cron Scheduler

## GA Prompt Composition

```
┌──────────────────────────────────┐
│ general_agent.md (core)          │  Always present: role, CLI, rules
├──────────────────────────────────┤
│ agent-claude: prompt section     │  If Claude provider active
├──────────────────────────────────┤
│ mesh: prompt section             │  If mesh enabled
├──────────────────────────────────┤
│ bridge-telegram: prompt section  │  If telegram plugin active
├──────────────────────────────────┤
│ memory: prompt section           │  If memory plugin active
├──────────────────────────────────┤
│ browser: prompt section          │  If browser plugin active
└──────────────────────────────────┘
= Final assembled GA system prompt
```

Each plugin provides `ga_section` markdown file. Plugin Manager concatenates during GA spawn.

## Plugin Dev in Production

Plugins (process tier) are hot-reloadable in production CodeTerm — no dev build needed.

### Workflow

```
~/.codeterm/plugins/my-plugin/
├── plugin.toml
├── main.py          # or any entrypoint
└── ...

# Develop:
1. Edit main.py
2. codeterm plugin restart my-plugin    # hot-restart, no app restart
3. Test via MCP tools or CLI
4. Repeat
```

### Plugin Dev Commands

```
codeterm plugin create "my-plugin" --tier process --lang python
  → Scaffolds plugin.toml + entrypoint + test harness

codeterm plugin restart <name>
  → Kill + respawn process plugin. Sends listChanged to MCP.

codeterm plugin logs <name>
  → Tail plugin's stderr (JSON-RPC debug output)

codeterm plugin test <name>
  → Run plugin's self-test (calls each declared tool, checks responses)

codeterm plugin validate <name>
  → Check manifest, verify tools respond to list, check permissions
```

### Native Plugin Dev

Native plugins compile into the binary — can't hot-reload. But:
- Dev mode: load `.dll`/`.dylib`/`.so` from `~/.codeterm/plugins/<name>/lib/`
- `codeterm plugin dev <name>` — watch + recompile + reload dynamic library
- Production: statically linked into release build

### Test Harness

Each plugin gets a test suite scaffold:

```python
# test_plugin.py (auto-generated by `plugin create`)
def test_tools_list():
    """All declared tools respond to list"""
    ...

def test_tool_call():
    """Each tool handles a basic call"""
    ...

def test_error_handling():
    """Tools return proper errors, not crashes"""
    ...
```

Run: `codeterm plugin test my-plugin` — executes in-process, reports pass/fail.

## Agent Interface: CLI + Skills (not MCP)

### Why CLI + Skills wins

| | CLI + Skills | MCP |
|---|---|---|
| **Reliability** | Stateless, each call independent. Crash doesn't affect next call. | Server can silently die, tools disappear without warning. |
| **Token cost** | Skill loaded on demand (0 when idle). ~50 tokens per CLI command description. | Tool schemas always in context. ~100 tokens per tool × N tools. |
| **Latency** | ~10-30ms (compiled Rust binary). Bash tool reuses shell session. | ~5-50ms. Irrelevant — LLM call takes 1-10 seconds. |
| **Error handling** | Exit codes + stderr. Model trained on billions of shell examples. | Structured but opaque. Server crashes produce confusing errors. |
| **Complexity** | Zero. Binary exists, skill file exists. Done. | Server lifecycle, crash recovery, Windows hacks, timeout config. |
| **Scalability** | Skills lazy-load by trigger words. 100 plugins = 0 idle tokens. | All tool schemas in context. 100 tools = 10K+ tokens always. |

### How plugins extend CLI + Skills

Each plugin declares:
```toml
[capabilities]
commands = ["memory"]              # adds `codeterm memory` subcommand
```

Plugin Manager:
1. Registers plugin's CLI subcommands at startup
2. Auto-generates skill section from manifest (tool descriptions, examples)
3. Adds trigger words from `[activation].triggers`
4. When agent mentions trigger → skill section loads into context
5. Agent calls `codeterm memory search "query"` via Bash tool
6. On compact → skill section unloads

### MCP as optional ecosystem wrapper

MCP stays as a thin optional layer for external tool discoverability:
- Other editors (Cursor, Windsurf) can discover CodeTerm via MCP
- Wraps same CLI commands, no separate implementation
- Not the primary path for agents inside CodeTerm

## Implementation Phases

### Phase 0: Foundation (done)
- [x] Fix stability (double input, mesh, encoding)
- [x] Document architecture (ARCHITECTURE.md)
- [x] Update VISION.md
- [x] Create this plan

### Phase 1: Unified Dispatch + Plugin Manager skeleton (2 weeks)
- [ ] Define `Operation` trait and `#[codeterm_operation]` macro
- [ ] Refactor REST/Socket/Tauri to use unified dispatch
- [ ] Create `PluginManager` struct in `src-tauri/src/plugin/`
- [ ] Plugin discovery: scan `~/.codeterm/plugins/` for `plugin.toml`
- [ ] Process plugin lifecycle: spawn, health check, restart, shutdown
- [ ] JSON-RPC stdio communication with process plugins

### Phase 2: Agent Provider extraction + Skill system (2 weeks)
- [ ] Move Claude detection into `agent-claude` native plugin
- [ ] Define stable `AgentProvider` plugin interface
- [ ] Move hard-coded patterns from `detector.rs` to provider
- [ ] Plugin skill contribution: each plugin provides skill section + trigger words
- [ ] Skill auto-generation from plugin manifest (CLI commands, examples)
- [ ] Plugin prompt contribution (`on_prompt_build` hook for GA prompt)

### Phase 3: Memory & Knowledge System (4-5 weeks)

**Detailed plan:** `docs/plans/memory-architecture.md`

Core modules (graph engine, memory store, skill broker, learnings) live in `src-tauri/src/memory/`.
Graph and memory are CORE, not plugins — too fundamental. Embeddings and dreaming are opt-in.

- [ ] Phase 3.1: Memory store + Graph engine (Rust port of ontology)
- [ ] Phase 3.2: Skill broker (single source of truth, CLI-based delivery)
- [ ] Phase 3.3: Learnings capture + compaction detection
- [ ] Phase 3.4: Dreaming (cron-based consolidation, graph-aware)
- [ ] Phase 3.5: Embeddings / vector search (optional, sqlite-vec)

### Phase 4: WASM Plugin Support (2-3 weeks)
- [ ] Integrate Extism or wasmtime
- [ ] WIT interface for host↔guest
- [ ] Sandbox: filesystem allowlist, epoch preemption
- [ ] First WASM plugin: text transformer or formatter

### Phase 6: Bridge + Browser as Plugins (1-2 weeks)
- [ ] Extract Telegram into process plugin
- [ ] Browser pane as native plugin with CDP control
- [ ] Settings UI generated from plugin manifest

## Research Sources

| Source | What we take from it |
|--------|---------------------|
| [Zed Extensions](https://github.com/zed-industries/zed/tree/main/crates/extension_host) | WASM host in Rust, WIT interface, epoch preemption |
| [Extism](https://github.com/extism/extism) | Quick WASM plugin bootstrap, bytes-in/out |
| [MCP Spec](https://modelcontextprotocol.io/specification/) | Agent tool protocol, JSON-RPC stdio |
| [Codex CLI](https://github.com/openai/codex/tree/main/codex-rs) | ToolRouter → ToolOrchestrator pattern |
| [Tauri Plugins](https://v2.tauri.app/develop/plugins/) | Builder pattern, permissions TOML |
| [Home Assistant](https://developers.home-assistant.io/docs/architecture_components/) | Manifest discovery, ConfigEntry lifecycle |
| [VS Code API](https://code.visualstudio.com/api) | Contributes manifest, activation events |
| [Neovim](https://neovim.io/doc/user/lua-plugin/) | Convention dirs, lazy-loading triggers |

## Key Decisions

1. **CLI + Skills is the agent interface.** Not MCP. Stateless, reliable, already exists. Skills lazy-load by trigger words — zero idle token cost. MCP optional for ecosystem discoverability.
2. **Process plugins first.** JSON-RPC stdio is battle-tested (MCP/LSP/DAP). WASM later for sandboxing.
3. **Terminal stays in core.** PTY + xterm.js too tightly coupled to extract as plugin.
4. **Manifest-driven.** `plugin.toml` declares everything including platform support and trigger words.
5. **Plugins compose the GA prompt.** Each contributes a skill section loaded on demand. Core prompt stays small.
6. **Unified Dispatch kills duplication.** One operation definition → REST + Socket + Tauri + CLI.
7. **Platform field.** Plugins declare `platforms = [...]`. Plugin Manager skips incompatible ones.
8. **Mesh-native.** Plugins opt into mesh replication. State syncs like peer state.
9. **Core is minimal.** Terminal, Multiplexer, PTY, Mesh, Config, Plugin Manager, Event Bus, Dispatch.
