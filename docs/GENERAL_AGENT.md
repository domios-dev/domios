# General Agent — System Specification v2

## Overview

The General Agent is an always-on AI agent living in `~/.codeterm/` (platform-adjusted). It auto-starts, self-recovers, and serves as the persistent brain of a CodeTerm installation. Reachable via terminal, Telegram, Discord, or any future integration through a unified messaging bridge.

## Core Principles

- **Always alive**: starts with CodeTerm, self-recovers on crash, resumes sessions
- **Unclosable**: user cannot close the General workspace or its primary tab
- **Provider-agnostic**: works with Claude, Codex, or any registered provider. Instructions file name comes from provider (`CLAUDE.md`, `AGENTS.md`, etc.)
- **Extensible**: integrations (Telegram, Discord, phone) plug in via a messaging bridge, not hardcoded per-service
- **Self-aware**: knows it's inside CodeTerm, has full skill knowledge, can manage other agents

---

## Directory Structure

Root: `~/.codeterm/` (macOS/Linux) or `%USERPROFILE%\.codeterm\` (Windows)

```
~/.codeterm/
├── general/                    # General Agent workspace CWD
│   ├── CLAUDE.md               # Auto-generated instructions (provider-specific name)
│   ├── EXTRA_INSTRUCTIONS.md   # User-editable, never overwritten
│   └── state/                  # Agent's own notes, memory files
├── secrets/                    # Secrets storage (agent-readable .md files)
│   ├── telegram.md             # Bot token, allowed users
│   ├── discord.md              # Bot token, server config
│   └── api_keys.md             # Any API keys agent needs
├── integrations/               # Integration configs
│   ├── telegram.toml           # Telegram bridge config
│   └── discord.toml            # Discord bridge config
├── config.toml                 # Main CodeTerm config (existing)
├── logs/                       # Existing log directory
└── sessions/                   # Existing session data
```

### Secrets system
- Agent reads `~/.codeterm/secrets/*.md` for credentials
- Each file = one service/context
- Markdown format — human-readable, agent-parseable
- `.gitignore`'d by default
- Agent can create/update secrets files itself

---

## Instructions File

### Auto-generated section
Provider-specific filename (e.g., `CLAUDE.md` for Claude, `AGENTS.md` for Codex).

Generated with markers — content between markers is overwritten on version change, content outside is preserved:

```markdown
<!-- CODETERM-AUTO-START v{version} -->
# CodeTerm General Agent

You are the General Agent — the always-on system brain of this CodeTerm installation.
You run inside CodeTerm, an AI-first terminal multiplexer.

## Your role
- Manage this machine's CodeTerm environment
- Respond to user requests via terminal or connected services (Telegram, Discord, etc.)
- Spawn and coordinate agents in project workspaces
- Manage cron jobs, notifications, system health
- Self-recover if you crash — resume your session or start fresh

## Available tools
{codeterm_skill_content}

## Machine info
- Platform: {platform}
- Machine: {hostname}
- CodeTerm: v{version}
- Provider: {agent_provider}

## Connected services
{connected_services}

## Secrets
Read credentials from ~/.codeterm/secrets/*.md when needed.

## Self-management
- You CANNOT close your own tab or workspace
- You CAN spawn agents in other workspaces
- You always run with skip-permissions
- If you need to restart, use /exit — CodeTerm will auto-resume you
- Your CWD is ~/.codeterm/general/
<!-- CODETERM-AUTO-END -->

# Extra Instructions
See EXTRA_INSTRUCTIONS.md for user-defined instructions.
```

### EXTRA_INSTRUCTIONS.md
- Created empty on first run
- Never touched by CodeTerm
- Agent told to read it
- User puts custom rules, personality, preferences here

### Provider switching
If user changes provider in settings (e.g., Claude → Codex):
1. Rename instructions file (`CLAUDE.md` → `AGENTS.md`)
2. Preserve user content outside markers
3. Regenerate auto section with new provider info
4. Restart agent

---

## Lifecycle

### Startup

```
CodeTerm launches
  │
  ├─ Session restore (user workspaces)
  │
  └─ General Agent init:
      ├─ Ensure ~/.codeterm/general/ exists + secrets/ + integrations/
      ├─ Generate/update instructions file (marker-based, preserves user content)
      ├─ Ensure General workspace exists (protected, always first)
      │
      ├─ Has agent_session_id?
      │   YES → provider.build_resume_command(session_id)
      │   NO  → provider.build_launch_command(skip_permissions=true)
      │
      └─ Backend spawns PTY + starts backend read loop
          ├─ Agent detected → healthy
          └─ Not detected 30s → recovery
```

### Recovery

```
Triggered when:
  - Resume failed
  - Agent not detected after timeout
  - Crash detected (shell prompt in screen_state)

Steps:
  1. Clear agent_session_id
  2. Send Ctrl+C + Enter (kill stuck process)
  3. Wait for shell prompt
  4. Fresh start via provider.build_launch_command()
  5. Retry up to 3 times with 60s delay
  6. After 3 fails → red dot on sidebar, manual retry available
```

### Shutdown
- Save agent_session_id for resume on next launch
- Don't send /exit — let PTY die with process
- Session saved normally

---

## Protected Workspace & Pane

### Workspace
| Property | Value |
|----------|-------|
| Name | "General" |
| CWD | `~/.codeterm/general/` |
| Closable | NO |
| Position | Always first in sidebar |
| Visual | Subtle accent icon/color |

### Pane
| Property | Value |
|----------|-------|
| Alias | `general` |
| Closable | NO |
| Provider | From settings (default: Claude) |
| skip_permissions | Always true |
| Role | Orchestrator, can_spawn=true |

### Pane alias system
- `general` → resolves to UUID in API/CLI
- Resolution: alias registry → UUID lookup (no partial title match)
- `codeterm send-text --pane general "message"`
- `POST /api/panes/general/send`

---

## Backend PTY Autonomy

**The General Agent's PTY runs independently of frontend.**

- Backend spawns PTY directly (not via frontend TerminalPane)
- Backend read loop feeds screen_state + output_buffer + process_pty_output
- Coexists with frontend read loop when tab is visible (try_clone_reader)
- Recovery monitoring runs in idle-poll, not dependent on tab focus
- New module: `src-tauri/src/service/general_agent.rs`

---

## Messaging Bridge (Integrations)

### Architecture

NOT per-service hardcoded. A unified bridge layer:

```
                    ┌── Telegram listener
User message ──→   ├── Discord listener     ──→ Bridge ──→ General Agent PTY
                    ├── Phone (future)                          │
                    └── Webhook (future)                        │
                                                                ↓
User response ←─── Bridge ←── Output capture ←── PTY output
```

### Bridge responsibilities
- Receive message from any integration
- Validate sender (allowed users per integration)
- Send to General Agent PTY as text + Enter
- Capture response (wait for agent output to stabilize)
- Send response back via integration
- Handle long responses (chunking for Telegram 4096 char limit)

### Auto-send on paste
When message comes through bridge (or floating chat input):
- Text is sent via `write_pty_string` with `\r`
- NOT via bracket paste that leaves text in input unsubmitted
- Bridge messages always auto-submit (no "pending in input" state)

### Integration config (`~/.codeterm/integrations/telegram.toml`)
```toml
enabled = false
# Agent reads token from ~/.codeterm/secrets/telegram.md
allowed_users = ["186809246"]
chunk_size = 4000
```

### Settings panel: Integrations section
```
┌─────────────────────────────────────────┐
│ Integrations                            │
│                                         │
│ Connect services to the General Agent.  │
│ Credentials stored in ~/.codeterm/secrets/ │
│                                         │
│ ── Telegram ──                          │
│ ○ Enable              [toggle: OFF]     │
│ ○ Allowed users       [186809246     ]  │
│ ○ Status: Not configured                │
│   [Configure via General Agent]         │
│                                         │
│ ── Discord ──                           │
│ ○ Enable              [toggle: OFF]     │
│   [Configure via General Agent]         │
│                                         │
│ ── Webhooks ──                          │
│ ○ Enable inbound      [toggle: OFF]     │
│ ○ Port                [7687          ]  │
│                                         │
│ "Configure via General Agent" sends a   │
│ message to the agent asking it to set   │
│ up the integration and store secrets.   │
└─────────────────────────────────────────┘
```

The agent itself handles setup — user clicks "Configure via General Agent" → CodeTerm sends a message to the General pane: "Set up Telegram integration. Ask the user for the bot token." → agent guides user through setup, stores token in secrets/.

---

## Config Schema

```rust
pub struct GeneralAgentConfig {
    pub enabled: bool,              // Start with CodeTerm (default: true)
    pub agent_provider: String,     // "claude" default
    pub auto_restart: bool,         // true
    pub max_restart_attempts: u32,  // 3
    pub restart_delay_s: u32,       // 60
}

pub struct IntegrationsConfig {
    pub telegram_enabled: bool,
    pub telegram_allowed_users: Vec<String>,
    pub discord_enabled: bool,
    pub discord_allowed_users: Vec<String>,
    pub webhook_enabled: bool,
    pub webhook_port: u16,          // 7687
}
```

---

## Implementation Phases

### Phase 0: Backend PTY autonomy (PREREQUISITE)
- Backend read loop for protected panes
- spawn_pty callable from backend without frontend
- Coexists with frontend reader
- New: `src-tauri/src/service/pty_lifecycle.rs`

### Phase 1: Protected workspace + pane + alias
- `protected: bool` on Workspace and Pane
- Guards in close_workspace/close_pane
- Sidebar: hide delete/close for protected
- General workspace always first
- Pane alias registry + "general" auto-registered
- Centralized resolve_pane_id with alias support

### Phase 2: General Agent foundation
- `~/.codeterm/general/` + `secrets/` + `integrations/` creation
- Instructions file generation (marker-based, provider-agnostic)
- EXTRA_INSTRUCTIONS.md (user-owned)
- GeneralAgentConfig in settings
- Settings panel section
- Auto-start on CodeTerm launch
- `src-tauri/src/service/general_agent.rs`

### Phase 3: Startup + recovery + resume
- Backend-driven PTY spawn for General Agent
- Resume via provider.build_resume_command()
- Fresh start via provider.build_launch_command()
- Recovery: crash detection, retry with backoff
- Sidebar warning on failure
- Manual restart from 3-dot menu

### Phase 4: Messaging bridge
- Bridge abstraction: `src-tauri/src/bridge/mod.rs`
- Message in → PTY → output capture → message out
- Auto-send (no bracket paste, always submit)
- Response chunking for platform limits
- Telegram implementation (first integration)
- Discord implementation (second)
- Webhook receiver (third)

### Phase 5: OS autostart
- macOS: launchd plist
- Windows: registry key
- Linux: .desktop autostart
- Settings toggle

### Phase 6: Self-management
- Agent can read/write secrets
- Agent can configure integrations (guided setup)
- Agent can restart itself (/exit → CodeTerm auto-resumes)
- Heartbeat as cron job (existing cron infra, direct HTTP from Rust — not through agent)

---

## Edge Cases

| Case | Behavior |
|------|----------|
| First launch ever | Create dirs, generate instructions, start agent |
| Resume succeeds | Agent continues with context |
| Resume fails | Clear session, fresh start |
| Agent crashes | Idle-poll detects → auto-restart |
| 3 restart failures | Red dot, manual retry |
| Provider changed | Rename instructions file, restart |
| User edits EXTRA_INSTRUCTIONS.md | Preserved always, agent reads on next start |
| CodeTerm version upgrade | Regenerate auto section (markers), preserve user content |
| Telegram message while agent busy | Queue in bridge, deliver when agent idle |
| Long agent response | Bridge chunks for platform limits |
| Multiple CodeTerm instances (release) | Single-instance plugin prevents |
| Multiple instances (debug) | Lock file in general/, second skips |
| Message via chat bubble | write_pty_string + \r, no bracket paste hold |
| Message via Telegram | Same: direct write + auto-submit |

<!-- revision: 2 -->
