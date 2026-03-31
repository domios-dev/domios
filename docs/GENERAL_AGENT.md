# General Agent — System Specification

## Overview

The General Agent is an always-on AI agent that serves as the persistent "brain" of a CodeTerm installation. It lives in a protected, unclosable workspace, auto-starts with CodeTerm, recovers from failures, and can orchestrate other agents. In the future it connects to external interfaces (Telegram bot, webhooks, scheduled tasks).

## Core Principles

- **Always alive**: starts with CodeTerm, restarts on crash, recovers from failed sessions
- **Unclosable**: user cannot close the General workspace or its primary tab
- **Self-aware**: knows it's inside CodeTerm, has full knowledge of available tools/skills
- **Orchestrator**: can spawn agents in other workspaces, manage cron jobs
- **Extensible**: Telegram bot, webhooks, external triggers plug into this agent

---

## Lifecycle

### Startup sequence

```
CodeTerm launches
  │
  ├─ Session restore (normal workspaces)
  │
  └─ General Agent check:
      │
      ├─ Does ~/.codeterm/general/ exist?
      │   NO → create directory + generate CLAUDE.md
      │   YES → continue
      │
      ├─ Does General workspace exist in session?
      │   NO → create protected workspace (name="General", cwd=~/.codeterm/general/)
      │   YES → use existing
      │
      ├─ Does General pane have agent_session_id?
      │   YES → attempt resume: `claude --resume {session_id} --dangerously-skip-permissions`
      │   NO → fresh start: `claude --dangerously-skip-permissions`
      │
      └─ Wait for agent detection (PTY output)
          │
          ├─ Detected → healthy, done
          │
          └─ Not detected after 30s → recovery (see below)
```

### Recovery

```
Recovery triggered when:
  - Resume failed (Claude error, session corrupt)
  - Agent not detected after timeout
  - Agent crashed (process exited, shell prompt visible)

Recovery steps:
  1. Clear agent_session_id on pane
  2. Kill any stuck process: send Ctrl+C + Enter
  3. Wait for shell prompt (poll_shell_prompt)
  4. Start fresh: `claude --dangerously-skip-permissions`
  5. If fresh start fails → wait 60s → retry (max 3 attempts)
  6. After 3 failures → mark as "General Agent failed to start"
     → show warning in sidebar (red dot on General workspace)
     → user can manually retry from 3-dot menu
```

### Shutdown

```
CodeTerm closing:
  1. Save General pane agent_session_id (for resume on next launch)
  2. Don't send /exit — let Claude be killed with PTY
  3. Session saved in normal session store
```

### Restart (CodeTerm restart)

```
1. Session restore creates General workspace + pane
2. agent_session_id exists → resume attempted
3. Resume success → General Agent continues previous context
4. Resume fail → recovery → fresh start
5. Fresh start → new CLAUDE.md context loaded automatically (it's the CWD)
```

---

## Workspace & Pane

### Protected workspace

| Property | Value |
|----------|-------|
| Name | "General" |
| CWD | `~/.codeterm/general/` (macOS/Linux) or `%USERPROFILE%\.codeterm\general\` (Windows) |
| Closable | NO — 3-dot menu shows no "Delete" option |
| Position | Always first in sidebar |
| Visual | Subtle accent (e.g., different icon, faint border color) |
| Created | On first CodeTerm launch, or if missing |

### General pane

| Property | Value |
|----------|-------|
| Alias | `general` — resolvable by name in API/CLI |
| Tab name | "General" (title_level: User, prevents auto-rename) |
| Closable | NO — 3-dot menu shows no "Close" option |
| Agent | Default provider from settings (initially Claude) |
| skip_permissions | Always true |
| Role | Orchestrator |
| can_spawn | true |

### Pane alias system

```
API:  POST /api/panes/general/send   → resolves "general" → UUID → sends
CLI:  codeterm send-text --pane general "do something"
Code: resolve_pane_id("general") checks aliases before UUID lookup
```

Alias resolution order:
1. Check registered aliases (general → UUID)
2. Check pane IDs (UUID match)
3. Check pane titles (partial match, returns first)

---

## ~/.codeterm/general/ directory

### Structure

```
~/.codeterm/general/
├── CLAUDE.md          # Auto-generated, explains CodeTerm + agent role
├── heartbeat.log      # Heartbeat cron output (if enabled)
└── state/             # Agent's own state files (memory, notes, etc.)
```

### CLAUDE.md content (auto-generated)

```markdown
# CodeTerm General Agent

You are the General Agent — the always-on system brain of this CodeTerm installation.

## Your role
- Monitor and manage this machine's CodeTerm environment
- Respond to user requests via the terminal or connected bots (Telegram, etc.)
- Spawn and coordinate other agents in project workspaces
- Manage cron jobs, notifications, system health

## What you can do

### CodeTerm CLI
You're running inside CodeTerm. Use the `codeterm` CLI:
- `codeterm workspace create "name"` — create a workspace
- `codeterm pane list` — list all panes
- `codeterm pane screen --pane ID` — see what's on another pane's screen
- `codeterm send-text --pane ID "text"` — send text to another pane
- `codeterm agent spawn --title "Worker" --task "do X"` — spawn a worker agent
- `codeterm cron create "name" --schedule "*/5 * * * *" --task "do Y"` — schedule tasks
- `codeterm notify "title" --body "message"` — send notification

### Your constraints
- You CANNOT close your own tab or workspace
- You CAN spawn agents in other workspaces
- You always run with --dangerously-skip-permissions
- Your CWD is ~/.codeterm/general/ — your workspace
- Use /memory to persist important notes across sessions

## Machine info
- Platform: {platform}
- Machine: {hostname}
- CodeTerm version: {version}

## Connected services
{connected_services_section}
```

Fields `{platform}`, `{hostname}`, `{version}` filled at generation time.
`{connected_services_section}` lists Telegram bot config if connected.

---

## Settings

### Settings panel: General Agent section

```
┌─────────────────────────────────────────┐
│ General Agent                           │
│                                         │
│ The always-on system agent. Starts with │
│ CodeTerm and manages background tasks.  │
│                                         │
│ ○ Start with CodeTerm     [toggle: ON]  │
│ ○ Agent provider          [Claude    ▼] │
│                                         │
│ ── Heartbeat ──                         │
│ ○ Enable heartbeat        [toggle: OFF] │
│ ○ Interval                [10 min    ▼] │
│ ○ Endpoint URL            [__________ ] │
│                                         │
│ ── Telegram Bot ──                      │
│ ○ Enable bot              [toggle: OFF] │
│ ○ Bot token               [__________ ] │
│ ○ Allowed user IDs        [__________ ] │
│ ○ Status: Not connected                 │
│   [Test Connection]                     │
│                                         │
│ ── Advanced ──                          │
│ ○ Auto-restart on crash   [toggle: ON]  │
│ ○ Max restart attempts    [3        ▼]  │
│ ○ Restart delay (sec)     [60         ] │
│                                         │
│ [Restart General Agent]                 │
│ [Open General Directory]                │
└─────────────────────────────────────────┘
```

### Config schema (Rust)

```rust
pub struct GeneralAgentConfig {
    pub enabled: bool,              // Start with CodeTerm
    pub agent_provider: String,     // "claude" (default)
    pub auto_restart: bool,         // Restart on crash
    pub max_restart_attempts: u32,  // 3
    pub restart_delay_s: u32,       // 60

    // Heartbeat
    pub heartbeat_enabled: bool,
    pub heartbeat_interval_m: u32,  // 10
    pub heartbeat_url: Option<String>,

    // Telegram bot
    pub telegram_enabled: bool,
    pub telegram_bot_token: Option<String>,
    pub telegram_allowed_users: Vec<String>,
}
```

---

## Telegram Bot Integration (future)

### Concept
The General Agent can be reached via Telegram. Messages from allowed users are forwarded to the General Agent's PTY. Agent responses are sent back via bot.

### Architecture

```
Telegram ──bot API──→ CodeTerm webhook listener
                          │
                          ├─ Validate user ID
                          ├─ Send message to General pane PTY
                          ├─ Wait for response (poll output)
                          └─ Send response back via bot API
```

### How it connects
1. User configures bot token + allowed user IDs in Settings
2. General Agent gets `[telegram]` section in CLAUDE.md
3. CodeTerm starts a lightweight webhook/polling listener
4. Messages flow: Telegram → CodeTerm → General Agent PTY → response → Telegram

### The agent configures itself
When Telegram is enabled, CLAUDE.md gets:
```markdown
## Telegram Bot
You are connected to Telegram bot @{bot_username}.
Messages from allowed users are forwarded to you.
Reply normally — your responses are sent back.
Use `codeterm notify` for proactive messages to the user.
```

---

## Implementation phases

### Phase 1: Foundation
- [ ] `~/.codeterm/general/` directory creation
- [ ] CLAUDE.md generation (template + machine info)
- [ ] Protected workspace (unclosable, always-first)
- [ ] General pane (unclosable tab, alias "general")
- [ ] Auto-start on CodeTerm launch
- [ ] Config schema (GeneralAgentConfig)
- [ ] Settings panel section

### Phase 2: Reliability
- [ ] Resume on restart (existing mechanism)
- [ ] Recovery on failed resume (clear session, fresh start)
- [ ] Recovery on crash (idle-poll detects shell prompt)
- [ ] Retry with backoff (3 attempts, 60s delay)
- [ ] Sidebar warning indicator on failure
- [ ] Manual restart from 3-dot menu

### Phase 3: Pane aliases
- [ ] Alias registry (name → UUID)
- [ ] "general" alias auto-registered
- [ ] API alias resolution (routes.rs)
- [ ] CLI alias support
- [ ] User-defined aliases (future)

### Phase 4: Heartbeat
- [ ] Default cron job (disabled)
- [ ] Settings toggle + interval + URL
- [ ] Heartbeat reports: agent alive, version, machine

### Phase 5: Telegram Bot
- [ ] Settings: bot token, allowed users
- [ ] Webhook/polling listener in Rust
- [ ] Message forwarding to General pane
- [ ] Response capture + send back
- [ ] CLAUDE.md section for bot awareness

---

## Edge cases

| Case | Behavior |
|------|----------|
| CodeTerm first launch | Create General workspace + directory + CLAUDE.md + start agent |
| Resume succeeds | Agent continues with full context |
| Resume fails (corrupt session) | Clear session_id, fresh start |
| Agent crashes mid-session | Idle-poll detects shell prompt → auto-restart |
| 3 restart failures | Stop trying, show warning, user retries manually |
| CodeTerm crash (no clean shutdown) | Next launch: resume attempt → recovery if needed |
| User tries to close General tab | Blocked (close option hidden) |
| User tries to close General workspace | Blocked (delete option hidden) |
| No internet (Claude can't connect) | Agent starts but may fail to respond; retry on next message |
| Multiple CodeTerm instances | Each has own General Agent (different PIDs, same directory) |
| Config change (different provider) | Restart General Agent with new provider |
| Start with system enabled (OS) | CodeTerm added to autostart (launchd/Task Scheduler/systemd) |

<!-- revision: 1 -->
