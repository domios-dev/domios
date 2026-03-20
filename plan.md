# CodeTerm — Cross-platform Terminal for AI Coding Agents

## Idea

A cross-platform terminal multiplexer (Windows/macOS/Linux) purpose-built for Claude Code and other CLI coding agents. Think cmux but not locked to macOS/Ghostty. Key differentiator: deep Claude Code integration via skills, hooks, and MCP — not just a pretty terminal.

## Inspiration & Research

### cmux (manaflow-ai/cmux)
- macOS-only, Swift + AppKit, libghostty renderer
- AGPL-3.0 license
- Key features:
  - **Notification rings** — panes get blue ring + tab lights up when agent needs attention
  - **Notification panel** — all pending notifications, jump to unread
  - **Vertical + horizontal tabs** — sidebar shows git branch, PR status, working dir, listening ports, notification text
  - **In-app browser** — scriptable API (from vercel-labs/agent-browser), agents can snapshot a11y tree, click, fill forms, eval JS
  - **CLI & Socket API** — create workspaces, split panes, send keystrokes, automate browser. Commands: `list-panes`, `focus-pane`, `new-pane`, `new-surface`, `close-surface`, `notify`, `identify`, `capabilities`
  - **Agent discovery** — `cmux identify` (system.identify), `cmux capabilities` (system.capabilities)
  - **Skills in repo** — `skills/cmux`, `skills/cmux-browser`, `skills/cmux-debug-windows`, `skills/cmux-markdown`, `skills/release`
  - **OSC 9/99/777** terminal notification sequences support
  - **Multi-window** (Cmd+Shift+N), per-window workspaces
  - **Menu bar extra** with unread badge
  - **Sparkle auto-updates**

### Caudex (caudex.sh)
- macOS + Windows, but ugly UI, limited features
- Multi-tab, splits, context/cost monitoring
- Visual MCP server manager
- Remote session access (Mac/iPad/iPhone)

### Claude Squad (smtg-ai/claude-squad)
- TUI for managing multiple Claude/Codex/Aider sessions
- Git worktree isolation per task
- Depends on tmux — no native Windows

### WezTerm (wezterm.org)
- **Rust, GPU-accelerated, cross-platform** (Win/Mac/Linux/BSD)
- Built-in multiplexer with domains (local, SSH, TLS)
- Headless server mode (`wezterm-mux-server`) — GUI can attach/detach without session loss
- Remote access via SSH or TLS over TCP
- Lua scripting API, hot-reload config
- Rich CLI: spawn, split, list, focus, send-text
- Open source, MIT license
- **Best candidate as rendering/multiplexer backend or reference architecture**

### Claude Code Integration Points
- **Skills** — `.md` files in `~/.claude/skills/` or project `.claude/skills/`. Invoked as slash commands. No SDK needed, just markdown instructions
- **Hooks** — shell scripts fired at lifecycle events (before/after tool calls, session start/stop, notification). Defined in `settings.json`
- **MCP servers** — external tool connections (JIRA, GitHub, Slack, DB). Lazy-loaded tool search reduces context by ~95%
- **Agent Teams** — lead + teammate agents, split-pane backends (tmux, iTerm2, NO Windows Terminal yet — open issue #24384)
- **Notification hook** — `hooks.notification` fires when agent needs input. Can trigger desktop toast, sound, etc.

---

## Core Features (MVP)

### 1. Terminal Multiplexer
- Tabs (vertical sidebar like cmux) + horizontal/vertical splits
- GPU-accelerated rendering
- Cross-platform: Windows, macOS, Linux
- Ghostty/WezTerm config compatibility for themes/fonts

### 2. Agent Notifications
- Detect when Claude Code (or other agent) is waiting for input
- Visual indicators: tab highlight, pane ring, system tray badge
- Desktop notifications (native toast on each platform)
- OSC 9/99/777 sequence support
- Sound notifications (configurable)
- Notification panel — all pending, jump to unread

### 3. Agent Status Dashboard
- Per-pane: git branch, working dir, agent status, cost/tokens used
- Sidebar shows all sessions at a glance
- Listening ports display (like cmux)

### 4. Claude Code Skills Integration
- Ship skills IN THE REPO (`skills/` directory)
- Skills the agent can invoke to control the terminal:
  - `split-pane` — create new pane with command
  - `focus-pane` — switch to specific pane
  - `send-keys` — send keystrokes to a pane
  - `notify` — send notification from agent
  - `open-browser` — open URL in built-in browser
  - `list-sessions` — get all active sessions
  - `workspace-create` — create new workspace
- Skills users invoke as slash commands:
  - `/layout` — save/restore pane layouts
  - `/status` — show all agent statuses
  - `/broadcast` — send command to all panes

### 5. Remote Access
- Headless server mode (like wezterm-mux-server)
- Attach/detach GUI without losing sessions
- SSH tunnel or TLS for remote connection
- Access from any device (phone/tablet via web UI?)
- API for external automation (Rollo, CI/CD, etc.)

### 6. Socket/CLI API
- Unix socket (or named pipe on Windows)
- Full CLI: `codeterm split`, `codeterm notify`, `codeterm list`, `codeterm send-keys`, etc.
- JSON output mode for scripting
- Agent discovery protocol (identify, capabilities)

---

## Extended Features (Post-MVP)

### 7. Built-in Browser
- Side-by-side with terminal panes
- Scriptable (a11y tree snapshot, click, fill, eval JS)
- Agent can use it for web tasks

### 8. Git Integration
- PR status in sidebar
- Branch per workspace
- Auto-worktree isolation

### 9. Session Recording & Replay
- Record terminal sessions
- Replay for debugging/review
- Share session recordings

### 10. Agent Teams Support
- Be a valid `teammateMode` backend for Claude Code
- Split-pane management for agent teams
- Inter-agent communication visualization

---

## Tech Stack Options

### Option A: Rust + WezTerm fork/library
- **Pros**: WezTerm already solves 70% of the problem (cross-platform terminal, multiplexer, GPU rendering, remote access, CLI). MIT license. Mature codebase.
- **Cons**: Large codebase to understand. Lua scripting may not be ideal for deep agent integration.
- **Approach**: Fork WezTerm, add agent-aware UI layer (notification rings, status dashboard, skills integration). Replace Lua config with agent-oriented config.

### Option B: Rust from scratch (using alacritty_terminal or similar)
- **Pros**: Clean architecture, purpose-built. Can use `alacritty_terminal` crate for VT parsing.
- **Cons**: Much more work. Reinventing multiplexer, remote access, etc.
- **Approach**: Use `alacritty_terminal` for terminal emulation, `wgpu` for rendering, custom multiplexer.

### Option C: Electron/Tauri + xterm.js
- **Pros**: Fast to prototype. Web tech = easy UI. Tauri is lightweight.
- **Cons**: Electron is bloated. xterm.js performance is meh. "Another Electron terminal" stigma.
- **Approach**: Tauri (Rust backend) + xterm.js frontend. Ship as native app.

### Option D: Tauri v2 + custom terminal widget (Rust PTY + canvas/WebGL)
- **Pros**: Tauri v2 is lightweight (~5MB), cross-platform, Rust backend. Custom WebGL terminal renderer can match GPU-native performance.
- **Cons**: Need to build terminal renderer. More work than xterm.js but better result.
- **Approach**: Rust backend handles PTY, multiplexer, socket API. Tauri frontend handles UI with WebGL terminal rendering.

### Recommendation: Option A (WezTerm fork) for fastest path to MVP
WezTerm already has: cross-platform, GPU rendering, multiplexer, remote access, CLI, SSH domains, headless server. Add the agent-specific UI and integration layer on top.

Alternative: Option D (Tauri v2) if we want a cleaner, more modern architecture and are OK with more upfront work.

---

## Architecture

```
+------------------------------------------+
|              CodeTerm App                 |
|                                          |
|  +--------+  +------------------------+  |
|  |Sidebar |  |     Terminal Panes     |  |
|  |        |  |  +------+  +------+   |  |
|  | [ws1]  |  |  | PTY1 |  | PTY2 |   |  |
|  | [ws2]  |  |  | agent |  | shell|   |  |
|  | [ws3]  |  |  +------+  +------+   |  |
|  |        |  |  +------+  +------+   |  |
|  | status |  |  | PTY3 |  |Browser|  |  |
|  | notifs |  |  | agent |  |      |   |  |
|  +--------+  +------------------------+  |
+------------------------------------------+
        |              |
   Socket API     Headless Server
   (CLI/skills)   (remote attach)
        |              |
  Claude Code     SSH/TLS/Web
  hooks/skills    remote access
```

### Components
1. **Core (Rust)**
   - PTY management (cross-platform)
   - Terminal emulation (VT100/xterm)
   - Multiplexer (workspaces, panes, tabs)
   - Socket API server (Unix socket / named pipe)
   - Headless server mode
   - OSC sequence parser (notifications)
   - Agent discovery protocol

2. **UI Layer**
   - GPU-accelerated terminal rendering
   - Sidebar with workspace list, status, notifications
   - Notification system (in-app + native OS)
   - Split pane management
   - Built-in browser (post-MVP)

3. **CLI (`codeterm`)**
   - Full control via command line
   - JSON output for scripting
   - Mirrors socket API

4. **Skills Package (`skills/`)**
   - Markdown files for Claude Code
   - Terminal control skills
   - User-facing slash commands
   - Shipped in repo, users copy to their projects

5. **Config**
   - TOML or YAML config file
   - Theme/font compatibility with Ghostty/WezTerm
   - Per-workspace settings
   - Hook definitions

---

## Skills Design

### Directory Structure
```
skills/
  codeterm/                    # Core terminal control skill
    skill.md                   # Main skill definition
    examples/
      split-workflow.md
      multi-agent-setup.md
  codeterm-notify/             # Notification skill
    skill.md
  codeterm-browser/            # Browser control skill (post-MVP)
    skill.md
  codeterm-layout/             # Layout management
    skill.md
```

### Example: `skills/codeterm/skill.md`
```markdown
---
name: codeterm
description: Control CodeTerm terminal from Claude Code
---

# CodeTerm Terminal Control

You can control the CodeTerm terminal using the `codeterm` CLI.

## Split Panes
- `codeterm split --direction right --command "claude code"` — split right with new Claude Code session
- `codeterm split --direction down` — split below with shell

## Notify
- `codeterm notify "Build complete" --level info` — send notification
- `codeterm notify "Tests failed" --level error --sound true`

## Session Management
- `codeterm list` — list all panes (JSON)
- `codeterm focus <pane-id>` — focus specific pane
- `codeterm send-keys <pane-id> "npm test"` — send command to pane

## Workspace
- `codeterm workspace create --name "feature-x" --dir /path/to/repo`
- `codeterm workspace list`
```

---

## Remote Access Design

### Headless Mode
```bash
# Start headless server
codeterm server start --port 7685 --tls

# Attach from another machine
codeterm attach remote-host:7685

# Or via SSH tunnel
codeterm attach ssh://user@host
```

### Web UI (stretch goal)
- Lightweight web interface for mobile/tablet access
- WebSocket connection to headless server
- Read-only or full control mode
- Useful for monitoring agents from phone

### API for External Systems
```bash
# Rollo/webhook can query status
curl http://localhost:7685/api/sessions
curl http://localhost:7685/api/notifications

# Or trigger actions
curl -X POST http://localhost:7685/api/panes/1/send-keys -d '{"keys":"exit\n"}'
```

---

## Implementation Plan

### Phase 1: Core Terminal (2-3 weeks)
- [ ] Project setup (Rust, choose rendering approach)
- [ ] Basic terminal emulation + PTY (cross-platform)
- [ ] Single window with tabs and splits
- [ ] GPU rendering
- [ ] Basic config (themes, fonts)

### Phase 2: Agent Awareness (1-2 weeks)
- [ ] OSC notification sequence detection
- [ ] Claude Code hook integration (notification hook fires `codeterm notify`)
- [ ] Tab/pane status indicators (waiting, working, idle)
- [ ] Native desktop notifications per platform
- [ ] Notification panel in sidebar

### Phase 3: CLI & Socket API (1 week)
- [ ] Socket server (Unix socket + named pipe on Windows)
- [ ] CLI commands: split, list, focus, send-keys, notify
- [ ] JSON output mode
- [ ] Agent discovery (identify, capabilities)

### Phase 4: Skills Package (1 week)
- [ ] Write skill markdown files
- [ ] Test with Claude Code
- [ ] Document skill installation
- [ ] Example workflows (multi-agent, monitoring)

### Phase 5: Remote Access (2 weeks)
- [ ] Headless server mode
- [ ] Attach/detach without session loss
- [ ] SSH tunnel support
- [ ] TLS connection
- [ ] REST API for external systems

### Phase 6: Polish & Extended (ongoing)
- [ ] Built-in browser
- [ ] Git integration in sidebar
- [ ] Agent Teams backend support
- [ ] Session recording
- [ ] Web UI for remote access
- [ ] Auto-updates

---

## Open Questions

1. **Name**: CodeTerm? AgentTerm? Something else?
2. **License**: MIT (permissive) or AGPL (like cmux)?
3. **WezTerm fork vs clean build**: Fork is faster but inherits complexity. Clean build is more work but purpose-built.
4. **Tauri vs native**: Tauri v2 gives us cross-platform UI fast. Native (like WezTerm's approach) is harder but more performant.
5. **Browser**: Worth including in MVP or strictly post-MVP?
6. **Pricing**: Open source? Freemium? Commercial license for teams?

---

## References

- [cmux](https://github.com/manaflow-ai/cmux) — macOS terminal for AI agents (AGPL-3.0)
- [WezTerm](https://github.com/wezterm/wezterm) — cross-platform terminal multiplexer (MIT)
- [Claude Code docs — hooks](https://code.claude.com/docs/en/hooks-guide)
- [Claude Code docs — agent teams](https://code.claude.com/docs/en/agent-teams)
- [Claude Squad](https://github.com/smtg-ai/claude-squad) — TUI agent manager
- [Caudex](https://caudex.sh/) — Claude Code terminal (Win/Mac)
- [Agent Deck](https://github.com/asheshgoplani/agent-deck) — TUI session manager
- [Windows Terminal agent teams feature request](https://github.com/anthropics/claude-code/issues/24384)
- [Claude Code Bridge](https://www.verdent.ai/guides/claude-code-bridge-terminal-ai-agents) — WezTerm/tmux bridge
