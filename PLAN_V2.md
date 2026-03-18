# CodeTerm v2 — Detailed Implementation Plan

> Cross-platform AI-first terminal multiplexer. "Terminal with holes for agents."

---

## 1. RESEARCH SUMMARY & CRITIQUE OF v1 PLAN

### What we learned from reference projects

**cmux** (macOS, Swift, ~20K LOC):
- Best-in-class UX: notification rings, sidebar with git/ports/status, in-app browser
- JSON-RPC v2 socket API — clean, extensible protocol
- Bonsplit split-tree — pure layout algorithm, conceptually portable
- Skills as markdown files in git repos — zero SDK overhead
- Browser integration via WKWebView + agent-browser skill (Vercel)
- Remote daemon in Go (cmuxd-remote) with SSH proxy
- **Limitation**: macOS-only (Metal GPU, AppKit, WKWebView)

**WezTerm** (Rust, ~500K LOC, MIT):
- Extremely modular — 53 crates, many published on crates.io
- **portable-pty** (0.9.0) — cross-platform PTY including Windows ConPTY ✅
- **wezterm-escape-parser** — robust escape sequence parser with extensible OSC ✅
- **wezterm-term** — full terminal emulator, decoupled from GUI ✅
- Pane/Domain/Mux abstractions — clean trait-based architecture
- Client-server protocol with LEB128 framing + optional Zstd compression
- Cross-platform windowing (X11, Wayland, Cocoa, Win32) + GPU rendering (wgpu/OpenGL)
- **Limitation**: Massive codebase, Lua config not ideal for agent integration

**Claude Squad** (Go, ~6K LOC):
- Git worktree isolation per session — great pattern, fully portable
- Status detection via SHA256 hash polling (500ms) + prompt pattern matching
- Bubbletea TUI — clean model-update-view pattern
- Daemon mode for headless auto-accept
- **Limitation**: Hard tmux dependency = no native Windows

**Claude Code** (Node.js):
- 9 hook lifecycle events (PreToolUse, PostToolUse, Notification, SessionStart, etc.)
- Skills = markdown auto-discovery from `skills/*/SKILL.md`
- Agent Teams need terminal backend (tmux, iTerm2 — NO Windows Terminal yet)
- MCP: stdio, SSE, HTTP, WebSocket transports
- Communication: stdin/stdout + OSC sequences + env vars (no custom IPC)
- Terminal detection already supports xterm.js-based terminals

### Critique of v1 plan

| Issue | Problem | Fix |
|-------|---------|-----|
| **Option A (WezTerm fork)** was recommended | WezTerm is 500K LOC, Lua-scripted, its `mux` and `term` crates are NOT published. Forking means maintaining a huge codebase with changes upstream constantly breaking. | Use WezTerm **crates** as libraries, don't fork the whole project |
| **GPU rendering from scratch** in Option D | Building a custom WebGL terminal renderer is months of work for questionable benefit over xterm.js | Use xterm.js with WebGL addon — proven, 900% faster than canvas, used by VS Code |
| **Headless server in Phase 5** | Remote access is a core differentiator, not an afterthought. The architecture should be server-first from day 1 | Move to Phase 2 — design as daemon + GUI client from the start |
| **Browser in Post-MVP** | Browser is what makes cmux special. Without it, we're just "another terminal" | Move to Phase 3 — use WebView2/CEF instead of WKWebView |
| **No mention of Agent Teams backend** | This is THE killer feature — being a valid `teammateMode` backend for Claude Code solves the Windows Terminal gap (issue #24384) | Make it Phase 2 priority |
| **Vague multiplexer design** | "Workspaces, panes, tabs" without data model | Define clear hierarchy: Server → Workspaces → Tabs → Panes (split tree) |
| **No auth model** | Socket API without auth = security hole | Adopt cmux's model: UID check + optional HMAC-SHA256 password |
| **Config compatibility** | "Ghostty/WezTerm config compatibility" is scope creep | Own config format (TOML), with theme import from popular terminals |

### Revised architecture decision

**Winner: Tauri v2 + xterm.js (WebGL) + Rust backend using WezTerm crates**

Rationale:
- **Tauri v2** (~5MB binary) — cross-platform desktop app, Rust backend, web frontend
- **xterm.js + WebGL addon** — battle-tested terminal renderer (VS Code, Hyper), handles all VT/escape parsing client-side
- **portable-pty** — cross-platform PTY from WezTerm (Windows ConPTY included)
- **Rust backend** — multiplexer, socket API server, OSC detection, agent status tracking
- **React/Svelte frontend** — sidebar, notifications, browser panel, split management
- **Built-in browser** — Tauri's WebView (WebView2 on Windows, WebKitGTK on Linux, WKWebView on macOS) — FREE, already cross-platform

This gives us:
- Cross-platform from day 1 (Win/Mac/Linux)
- Fast UI development (web tech for sidebar/notifications/settings)
- GPU-accelerated terminal rendering (xterm.js WebGL)
- Native browser integration (Tauri WebView)
- Small binary size (~10-15MB)
- Access to all WezTerm crates as libraries

---

## 2. ARCHITECTURE

```
┌─────────────────────────────────────────────────────────────┐
│                    CodeTerm Desktop App                       │
│                      (Tauri v2 shell)                        │
│                                                              │
│  ┌──────────┐  ┌────────────────────────────────────────┐   │
│  │ Sidebar  │  │          Content Area                   │   │
│  │          │  │  ┌──────────────┐ ┌──────────────┐     │   │
│  │ [ws:main]│  │  │  xterm.js    │ │  xterm.js    │     │   │
│  │  branch  │  │  │  Terminal 1  │ │  Terminal 2  │     │   │
│  │  status  │  │  │  (agent)     │ │  (shell)     │     │   │
│  │  ports   │  │  └──────────────┘ └──────────────┘     │   │
│  │          │  │  ┌──────────────┐ ┌──────────────┐     │   │
│  │ [ws:feat]│  │  │  xterm.js    │ │  WebView     │     │   │
│  │  branch  │  │  │  Terminal 3  │ │  Browser     │     │   │
│  │  🔔 notif│  │  │  (agent)     │ │  Panel       │     │   │
│  │          │  │  └──────────────┘ └──────────────┘     │   │
│  │──────────│  └────────────────────────────────────────┘   │
│  │ 🔔 Notifs│                                               │
│  │ ⚙ Config │                                               │
│  └──────────┘                                               │
└──────────┬──────────────────────────┬───────────────────────┘
           │                          │
    Tauri IPC (invoke)         Tauri IPC (events)
           │                          │
┌──────────▼──────────────────────────▼───────────────────────┐
│                    Rust Backend (codeterm-core)              │
│                                                              │
│  ┌─────────────┐  ┌──────────────┐  ┌────────────────────┐ │
│  │ Multiplexer │  │ Socket API   │  │ Agent Monitor      │ │
│  │             │  │ Server       │  │                    │ │
│  │ Workspaces  │  │ Unix socket  │  │ OSC detection      │ │
│  │   Tabs      │  │ Named pipe   │  │ Prompt patterns    │ │
│  │     Panes   │  │ JSON-RPC v2  │  │ Status tracking    │ │
│  │       PTYs  │  │ REST API     │  │ Git branch/status  │ │
│  └─────────────┘  └──────────────┘  │ Port scanning      │ │
│                                      └────────────────────┘ │
│  ┌─────────────┐  ┌──────────────┐  ┌────────────────────┐ │
│  │ PTY Manager │  │ Notification │  │ Session Store      │ │
│  │ portable-pty│  │ Engine       │  │                    │ │
│  │ ConPTY/Unix │  │              │  │ Persist/restore    │ │
│  │             │  │ Desktop notif│  │ Layout snapshots   │ │
│  │             │  │ Sound alerts │  │ JSON serialization │ │
│  └─────────────┘  └──────────────┘  └────────────────────┘ │
│                                                              │
│  ┌─────────────────────────────────────────────────────────┐│
│  │                    CLI Binary (codeterm)                 ││
│  │  Connects to socket API. Mirrors all commands.          ││
│  └─────────────────────────────────────────────────────────┘│
└─────────────────────────────────────────────────────────────┘
           │
    Socket API / REST
           │
┌──────────▼──────────────────────────────────────────────────┐
│  External Consumers                                          │
│  - Claude Code (via hooks/skills)                            │
│  - Rollo / webhooks                                          │
│  - CI/CD pipelines                                           │
│  - Other terminals (attach)                                  │
└─────────────────────────────────────────────────────────────┘
```

### Data Model

```
Server (codeterm-core daemon)
├── Config
│   ├── theme (colors, opacity, blur)
│   ├── font (family, size, ligatures)
│   ├── keybindings
│   └── socket settings (auth mode, path)
│
├── Workspaces[] (like cmux workspaces / wezterm windows)
│   ├── id: UUID
│   ├── name: String
│   ├── working_dir: PathBuf
│   ├── git_branch: Option<String>
│   ├── git_worktree: Option<GitWorktree>  (claude-squad pattern)
│   │
│   ├── Tabs[] (vertical sidebar entries)
│   │   ├── id: UUID
│   │   ├── title: String
│   │   ├── split_tree: SplitNode  (bonsplit-like tree)
│   │   │   ├── Leaf(Pane)
│   │   │   └── Split { orientation, ratio, children[] }
│   │   │
│   │   └── Panes[] (terminal or browser instances)
│   │       ├── id: UUID
│   │       ├── type: Terminal | Browser
│   │       ├── pty: Option<PtyHandle>  (portable-pty)
│   │       ├── status: Working | Waiting | Idle | Dead
│   │       ├── notifications: Vec<Notification>
│   │       ├── agent_type: Option<AgentType>  (claude, codex, aider, shell)
│   │       ├── env_vars: HashMap  (CODETERM_PANE_ID, etc.)
│   │       └── metadata: PaneMetadata
│   │           ├── cwd: PathBuf
│   │           ├── git_branch: String
│   │           ├── listening_ports: Vec<u16>
│   │           └── process_name: String
│   │
│   └── status_entries: Vec<StatusEntry>  (pills in sidebar)
│
├── Notifications[]
│   ├── id: UUID
│   ├── workspace_id, pane_id
│   ├── title, body, level (info/warn/error)
│   ├── timestamp
│   └── read: bool
│
└── Sessions (persistence)
    ├── save() → JSON snapshot
    └── restore() → recreate workspaces/panes
```

### Communication Flow

```
Agent (Claude Code) running in PTY
    │
    ├─ stdout ──► Rust backend parses byte stream
    │              ├─ Forward raw bytes to xterm.js via Tauri event
    │              ├─ Detect OSC 9/99/777 ──► Notification engine
    │              ├─ Detect prompt patterns ──► Status = Waiting
    │              └─ Detect CWD change (OSC 7) ──► Update metadata
    │
    ├─ Claude Code hook (Notification) ──► fires `codeterm notify`
    │              └─ CLI connects to socket ──► Notification engine
    │
    └─ Claude Code skill ──► `codeterm split --right "npm test"`
                   └─ CLI connects to socket ──► Multiplexer creates pane

External (Rollo / webhook)
    │
    ├─ REST API ──► http://localhost:7685/api/...
    └─ Socket  ──► JSON-RPC v2 over Unix socket / named pipe
```

---

## 3. TECH STACK (FINAL)

| Layer | Technology | Why |
|-------|-----------|-----|
| **App shell** | Tauri v2 | Lightweight (~5MB), cross-platform, Rust backend, WebView UI |
| **Terminal rendering** | xterm.js + @xterm/addon-webgl | Battle-tested (VS Code), WebGL = 900% faster, handles all VT parsing |
| **PTY** | portable-pty 0.9 | From WezTerm, cross-platform including Windows ConPTY |
| **Escape parsing** | Server-side: scan for OSC only (lightweight). Client-side: xterm.js handles full parsing | Split responsibility — backend only needs OSC/notification detection |
| **Frontend framework** | React + TypeScript | Fast development, huge ecosystem, good for complex UI (sidebar, notifications) |
| **Styling** | Tailwind CSS | Fast, consistent styling |
| **Split management** | Custom split-tree (inspired by Bonsplit) | Simple recursive tree, drag-resize dividers |
| **Socket API** | JSON-RPC v2 over Unix socket / named pipe | cmux-compatible protocol, proven design |
| **REST API** | Axum (embedded in Rust backend) | For webhooks, remote monitoring |
| **Browser panel** | Tauri WebView (secondary) | Free — WebView2 (Win), WebKitGTK (Linux), WKWebView (Mac) |
| **Config** | TOML | Simple, readable, Rust-native (serde support) |
| **Persistence** | JSON files | Session snapshots, notification history |
| **Desktop notifications** | notify-rust / tauri-plugin-notification | Cross-platform native notifications |
| **CLI** | clap (Rust) | Standard Rust CLI framework |
| **Build** | cargo + pnpm | Rust backend + frontend |

### Key Rust Dependencies

```toml
[dependencies]
tauri = "2"
portable-pty = "0.9"
tokio = { version = "1", features = ["full"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
uuid = { version = "1", features = ["v4"] }
toml = "0.8"
axum = "0.7"                    # REST API
tower = "0.4"
notify-rust = "4"               # Desktop notifications
interprocess = "2"              # Unix socket + named pipe
clap = { version = "4", features = ["derive"] }
git2 = "0.19"                   # Git operations (libgit2)
sysinfo = "0.31"                # Process/port info
rodio = "0.19"                  # Sound notifications
```

### Key Frontend Dependencies

```json
{
  "@anthropic-ai/tauri": "^2",
  "@xterm/xterm": "^5",
  "@xterm/addon-webgl": "^0.18",
  "@xterm/addon-fit": "^0.10",
  "@xterm/addon-unicode11": "^0.8",
  "@xterm/addon-web-links": "^0.11",
  "react": "^19",
  "react-dom": "^19",
  "tailwindcss": "^4",
  "@tauri-apps/api": "^2"
}
```

---

## 4. IMPLEMENTATION PHASES

### Phase 0: Project Bootstrap
> Get a blank Tauri v2 + React app running with a single xterm.js terminal connected to a PTY

- [ ] **0.1** Initialize Tauri v2 project with React + TypeScript frontend
  - `pnpm create tauri-app codeterm --template react-ts`
  - Verify builds on Windows (primary dev machine)
- [ ] **0.2** Add portable-pty dependency, create basic PTY spawn
  - Tauri command: `spawn_pty(command, args, cwd, cols, rows) → pty_id`
  - Read loop: PTY stdout → Tauri event `pty-output-{id}`
  - Write: Tauri command `write_pty(pty_id, data)`
  - Resize: Tauri command `resize_pty(pty_id, cols, rows)`
- [ ] **0.3** Add xterm.js with WebGL addon
  - Single terminal filling the window
  - Connect to PTY via Tauri events
  - Verify: type commands, see output, colors work, resize works
- [ ] **0.4** Basic window chrome
  - Custom titlebar (Tauri decorations: false)
  - Minimize/maximize/close buttons
  - App icon

**Deliverable**: Working terminal app, single pane, typed text appears, shell works.

---

### Phase 1: Multiplexer Core
> Tabs, splits, workspaces — the terminal multiplexer foundation

- [ ] **1.1** Data model implementation (Rust)
  - `Workspace`, `Tab`, `Pane`, `SplitNode` structs
  - `Multiplexer` struct managing all state
  - PTY lifecycle (spawn, kill, resize per pane)
  - Unique IDs (UUID) for everything
- [ ] **1.2** Split tree layout engine
  - `SplitNode::Leaf(PaneId)` / `SplitNode::Split { orientation, ratio, children }`
  - Calculate pixel bounds for each pane given container size
  - Drag-resize dividers (update ratio)
  - Split actions: split-right, split-down, close-pane, zoom-pane
- [ ] **1.3** Tab system (frontend)
  - Vertical sidebar with tab list
  - New tab (Ctrl+Shift+T / Cmd+T)
  - Close tab (Ctrl+Shift+W / Cmd+W)
  - Switch tabs (Ctrl+1-9 / Cmd+1-9)
  - Tab shows: title, working directory
- [ ] **1.4** Split pane UI (frontend)
  - Render split tree recursively
  - Each leaf = xterm.js instance connected to its PTY
  - Draggable dividers between panes
  - Focus management (click to focus, keyboard shortcuts)
  - Ctrl+Shift+D / Cmd+D = split right
  - Ctrl+Shift+E / Cmd+Shift+D = split down
- [ ] **1.5** Workspace management
  - Multiple workspaces (like virtual desktops)
  - Create/rename/delete workspace
  - Switch workspaces (sidebar click or keyboard)
  - Each workspace has its own tab tree
- [ ] **1.6** Keyboard shortcuts system
  - Configurable keybindings (TOML config)
  - Default keybindings matching common terminals
  - Vim-style pane navigation (Ctrl+h/j/k/l optional)

**Deliverable**: Full multiplexer — multiple workspaces, tabs, horizontal/vertical splits, keyboard navigation.

---

### Phase 2: Agent Awareness + Socket API
> The "holes" that let agents in — detection, notifications, socket control

- [ ] **2.1** OSC sequence detection (Rust backend)
  - Scan PTY output stream for OSC 9, 99, 777 (notification sequences)
  - Scan for OSC 7 (current working directory)
  - Scan for custom OSC 1337 (iTerm2-style, for future extensions)
  - Don't full-parse VT — just detect OSC boundaries in byte stream
  - Forward detected OSC events to frontend
- [ ] **2.2** Agent status detection
  - Prompt pattern matching (like Claude Squad):
    - Claude Code: "Do you want to proceed?" / permission prompts
    - Aider: "(Y)es/(N)o"
    - Generic: no output for N seconds = idle
  - Hash-based change detection (SHA256 of last N lines)
  - Status enum: `Working | Waiting | Idle | Dead`
  - Polling interval: 500ms (configurable)
- [ ] **2.3** Notification engine
  - In-app notification store (list of notifications with read/unread)
  - Desktop notifications (notify-rust / tauri-plugin-notification)
  - Sound alerts (configurable, rodio)
  - Visual indicators:
    - Pane border glow (blue = notification)
    - Tab badge (unread count)
    - Sidebar workspace badge
  - Notification panel in sidebar (click to jump to pane)
- [ ] **2.4** Socket API server (Rust)
  - Unix domain socket on macOS/Linux
  - Named pipe on Windows (`\\.\pipe\codeterm`)
  - JSON-RPC v2 protocol (newline-delimited JSON)
  - Methods (matching cmux v2 API for compatibility):
    - `system.identify` — version, features, caller info
    - `system.capabilities` — supported operations
    - `workspace.list` / `workspace.create` / `workspace.focus`
    - `pane.list` / `pane.create` / `pane.focus` / `pane.close`
    - `pane.split` — create split with direction + command
    - `pane.send_text` — send keystrokes to pane
    - `pane.send_key` — send special key
    - `notification.create` / `notification.list` / `notification.clear`
    - `browser.open` / `browser.navigate` (post-MVP)
  - Authentication: UID check + optional HMAC-SHA256
  - Environment variables injected into PTY:
    - `CODETERM_SOCKET_PATH` — socket path for CLI
    - `CODETERM_PANE_ID` — current pane UUID
    - `CODETERM_WORKSPACE_ID` — current workspace UUID
    - `CODETERM_VERSION` — app version
- [ ] **2.5** CLI binary (`codeterm`)
  - Separate Rust binary, connects to socket API
  - Commands mirror API: `codeterm split`, `codeterm notify`, `codeterm list`, `codeterm send-keys`, `codeterm identify`
  - `--json` flag for machine-readable output
  - Auto-discover socket path from env or default location
- [ ] **2.6** Sidebar metadata
  - Git branch detection (git2 crate, poll on CWD change)
  - Listening ports (sysinfo crate, poll every 5s)
  - Process name (from PTY child process)
  - Working directory (from OSC 7 or /proc/PID/cwd)
  - Agent type auto-detection (process name matching)

**Deliverable**: Agents can be detected (waiting/working), notifications work (in-app + desktop + sound), full socket API for external control, CLI works.

---

### Phase 3: Claude Code Integration
> Skills, hooks, Agent Teams backend — deep Claude Code integration

- [ ] **3.1** Skills package
  - `skills/codeterm/SKILL.md` — core terminal control
  - `skills/codeterm-notify/SKILL.md` — notification management
  - `skills/codeterm-layout/SKILL.md` — layout save/restore
  - Each skill documents CLI commands with examples
  - Test: install skill, use Claude Code to control terminal
- [ ] **3.2** Hook integration
  - Ship example hook configs for `settings.json`:
    - `Notification` hook → `codeterm notify "$TITLE" --body "$BODY"`
    - `SessionStart` hook → `codeterm pane.set-agent-type claude`
  - Document hook setup in README
- [ ] **3.3** Agent Teams backend
  - Implement the teammate spawning protocol:
    - Create new pane with Claude Code session
    - Set up environment variables
    - Report pane output back
  - Register as valid `teammateMode` backend
  - Test: lead agent spawns sub-agents in CodeTerm panes
- [ ] **3.4** Agent discovery
  - `codeterm identify` — returns version, platform, features
  - `codeterm capabilities` — returns supported API methods
  - Agents detect CodeTerm via `CODETERM_SOCKET_PATH` env var or `command -v codeterm`

**Deliverable**: Claude Code can control CodeTerm via skills, hooks fire notifications, Agent Teams work with CodeTerm as backend.

---

### Phase 4: Browser Panel + Polish
> Built-in browser, config system, theming, UX polish

- [ ] **4.1** Browser panel
  - Open browser as pane type (Tauri secondary WebView)
  - URL bar with navigation (back/forward/reload)
  - Agent can open URLs: `codeterm browser open https://...`
  - JavaScript evaluation: `codeterm browser eval "document.title"`
  - Side-by-side with terminal panes in split tree
- [ ] **4.2** Configuration system
  - TOML config file (`~/.config/codeterm/config.toml`)
  - Settings:
    - `[theme]` — colors, background opacity, blur
    - `[font]` — family, size, ligatures
    - `[keybindings]` — key → action mapping
    - `[notifications]` — sound, desktop, visual settings
    - `[socket]` — auth mode, custom path
    - `[agent]` — detection patterns, polling interval
  - Hot-reload on file change (notify crate)
  - Settings UI panel in sidebar
- [ ] **4.3** Theme system
  - Built-in themes (Dracula, One Dark, Catppuccin, Solarized, Nord)
  - Import from WezTerm/iTerm2 color schemes
  - Background opacity + blur (Tauri window effects)
  - Font: system fonts, with bundled fallback (JetBrains Mono Nerd Font)
- [ ] **4.4** UX polish
  - Smooth animations (pane resize, tab switch)
  - Context menu (right-click in terminal, tab, sidebar)
  - Search in terminal (Ctrl+Shift+F)
  - Copy/paste (Ctrl+Shift+C/V, auto-copy on select option)
  - Scrollback buffer (configurable size)
  - Link detection (clickable URLs in terminal output)
  - Bell handling (visual bell option)
- [ ] **4.5** Agent Canvas (browser panel skill)
  - Canvas host: embedded HTTP server serving agent-generated HTML in browser panel
  - A2UI integration: declarative JSONL → Lit web components rendering
    - Agent produces JSONL payloads describing UI (Text, Button, Card, Image, Chart, etc.)
    - Browser panel renders via Lit components (from OpenClaw's vendor/a2ui)
  - Canvas actions via socket API:
    - `canvas.present` — show canvas with URL or inline content
    - `canvas.hide` — hide canvas
    - `canvas.navigate` — load URL
    - `canvas.eval` — execute JavaScript, return result
    - `canvas.snapshot` — capture canvas as PNG/JPEG image
    - `canvas.a2ui_push` — render A2UI component JSONL
    - `canvas.a2ui_reset` — clear canvas state
  - Live reload via WebSocket (dev mode)
  - Message bridge: canvas → agent (user interactions sent back)
  - Skill: `skills/codeterm-canvas/SKILL.md` — agent instructions for canvas use
  - Use cases: dashboards, diagrams, previews, interactive forms, data visualization
  - Reference: OpenClaw canvas-host (github.com/openclaw/openclaw)
- [ ] **4.6** Git integration in sidebar
  - Branch name with colored indicator (clean/dirty)
  - PR status badge (if GitHub CLI available)
  - Quick actions: checkout, pull, push (stretch)

**Deliverable**: Full-featured terminal with browser, theming, config, smooth UX.

---

### Phase 5: Session Persistence + Remote Access
> Don't lose work, access from anywhere

- [ ] **5.1** Session persistence
  - Save on exit: workspace layout, pane positions, CWD, tab titles
  - Restore on start: recreate workspaces, spawn PTYs
  - Manual save/restore of named layouts
  - Auto-save interval (configurable)
- [ ] **5.2** Headless daemon mode
  - `codeterm daemon start` — run without GUI
  - PTYs survive GUI disconnect
  - GUI attaches/detaches to running daemon
  - On app close: option to keep daemon running
- [ ] **5.3** REST API for remote monitoring
  - Embedded Axum HTTP server (optional, configurable port)
  - Endpoints:
    - `GET /api/workspaces` — list all
    - `GET /api/panes` — list with status
    - `GET /api/notifications` — pending notifications
    - `POST /api/panes/{id}/send` — send input
    - `POST /api/notify` — create notification
  - Auth: Bearer token (configured in TOML)
  - CORS for web clients
- [ ] **5.4** SSH tunnel support
  - `codeterm attach ssh://user@host` — connect to remote daemon
  - Port forwarding for socket API
  - Multiplex remote PTYs
- [ ] **5.5** Web UI (stretch)
  - Lightweight web client (same React frontend, served by daemon)
  - WebSocket for real-time PTY output
  - Read-only or full control mode
  - Mobile-responsive for phone monitoring

**Deliverable**: Sessions survive crashes/restarts, remote access via SSH/REST/Web.

---

### Phase 6: Advanced Features (ongoing)
> Extended capabilities, ecosystem growth

- [ ] **6.1** Auto-updates (Tauri updater plugin)
- [ ] **6.2** Session recording & replay
  - Record raw PTY output with timestamps
  - Replay at original speed or fast-forward
  - Export as asciicast (compatible with asciinema)
- [ ] **6.3** Git worktree isolation (Claude Squad pattern)
  - Per-workspace worktree
  - Branch management
  - Diff tracking
- [ ] **6.4** Multi-window support
  - Open new window (Ctrl+Shift+N)
  - Drag tabs between windows
- [ ] **6.5** Plugin system
  - User-defined extensions (TypeScript or Lua)
  - Hook into lifecycle events
  - Custom pane types
- [ ] **6.6** System tray
  - Tray icon with notification badge
  - Quick access menu
  - Background mode (close window, keep running)

---

## 5. SOCKET API SPECIFICATION (JSON-RPC v2)

### Protocol

Transport: Unix domain socket (macOS/Linux) or Named pipe (Windows)
Framing: Newline-delimited JSON
Authentication: UID check (default) or HMAC-SHA256

### Request

```json
{"jsonrpc":"2.0","id":"req-1","method":"pane.split","params":{"direction":"right","command":"claude"}}
```

### Response (success)

```json
{"jsonrpc":"2.0","id":"req-1","result":{"pane_id":"550e8400-e29b-41d4-a716-446655440000"}}
```

### Response (error)

```json
{"jsonrpc":"2.0","id":"req-1","error":{"code":-32602,"message":"invalid direction"}}
```

### Methods

| Method | Params | Returns | Description |
|--------|--------|---------|-------------|
| `system.identify` | — | `{version, platform, features}` | Agent discovery |
| `system.capabilities` | — | `{methods: [...]}` | List supported methods |
| `workspace.list` | — | `[{id, name, tab_count}]` | List workspaces |
| `workspace.create` | `{name?, dir?}` | `{workspace_id}` | Create workspace |
| `workspace.focus` | `{workspace_id}` | `{}` | Focus workspace |
| `workspace.close` | `{workspace_id}` | `{}` | Close workspace |
| `pane.list` | `{workspace_id?}` | `[{id, type, status, cwd}]` | List panes |
| `pane.split` | `{direction, command?, cwd?}` | `{pane_id}` | Split current pane |
| `pane.focus` | `{pane_id}` | `{}` | Focus pane |
| `pane.close` | `{pane_id}` | `{}` | Close pane |
| `pane.send_text` | `{pane_id, text}` | `{}` | Send text to pane |
| `pane.send_key` | `{pane_id, key, modifiers?}` | `{}` | Send key event |
| `pane.get_output` | `{pane_id, lines?}` | `{content}` | Get terminal output |
| `notification.create` | `{title, body?, level?}` | `{notification_id}` | Create notification |
| `notification.list` | `{unread_only?}` | `[{id, title, body, read}]` | List notifications |
| `notification.clear` | `{notification_id?}` | `{}` | Clear notification(s) |
| `browser.open` | `{url, pane_id?}` | `{pane_id}` | Open browser panel |
| `browser.navigate` | `{pane_id, url}` | `{}` | Navigate browser |
| `browser.eval` | `{pane_id, js}` | `{result}` | Evaluate JavaScript |
| `canvas.present` | `{target?, x?, y?, w?, h?}` | `{pane_id}` | Show canvas panel |
| `canvas.hide` | `{pane_id}` | `{}` | Hide canvas |
| `canvas.navigate` | `{pane_id, url}` | `{}` | Load URL in canvas |
| `canvas.eval` | `{pane_id, js}` | `{result}` | Execute JS in canvas |
| `canvas.snapshot` | `{pane_id, format?, max_width?}` | `{image_base64}` | Capture canvas as image |
| `canvas.a2ui_push` | `{pane_id, jsonl}` | `{}` | Render A2UI components |
| `canvas.a2ui_reset` | `{pane_id}` | `{}` | Clear canvas state |

---

## 6. ENVIRONMENT VARIABLES

Injected into every PTY spawned by CodeTerm:

| Variable | Example | Description |
|----------|---------|-------------|
| `CODETERM` | `1` | Flag: running inside CodeTerm |
| `CODETERM_VERSION` | `0.1.0` | App version |
| `CODETERM_SOCKET_PATH` | `/tmp/codeterm.sock` | Socket API path |
| `CODETERM_PANE_ID` | `uuid-...` | Current pane ID |
| `CODETERM_WORKSPACE_ID` | `uuid-...` | Current workspace ID |
| `CODETERM_API_PORT` | `7685` | REST API port (if enabled) |
| `TERM_PROGRAM` | `CodeTerm` | Standard terminal identification |
| `TERM_PROGRAM_VERSION` | `0.1.0` | Standard version identification |

---

## 7. DIRECTORY STRUCTURE

```
codeterm/
├── PLAN_V2.md                    # This file
├── plan.md                       # Original plan (archived)
├── references/                   # Cloned reference projects
│   ├── cmux/
│   ├── wezterm/
│   ├── claude-squad/
│   └── claude-code/
│
├── src-tauri/                    # Rust backend (Tauri)
│   ├── Cargo.toml
│   ├── src/
│   │   ├── main.rs              # Tauri app entry
│   │   ├── lib.rs               # Library root
│   │   ├── multiplexer/         # Core multiplexer
│   │   │   ├── mod.rs
│   │   │   ├── workspace.rs     # Workspace management
│   │   │   ├── tab.rs           # Tab management
│   │   │   ├── pane.rs          # Pane + PTY lifecycle
│   │   │   └── split_tree.rs    # Split layout algorithm
│   │   ├── pty/                 # PTY management
│   │   │   ├── mod.rs
│   │   │   └── manager.rs       # Spawn, read, write, resize
│   │   ├── agent/               # Agent detection
│   │   │   ├── mod.rs
│   │   │   ├── detector.rs      # OSC + prompt detection
│   │   │   ├── status.rs        # Status tracking
│   │   │   └── patterns.rs      # Agent-specific patterns
│   │   ├── socket/              # Socket API server
│   │   │   ├── mod.rs
│   │   │   ├── server.rs        # Unix socket / named pipe
│   │   │   ├── handler.rs       # JSON-RPC method dispatch
│   │   │   └── auth.rs          # Authentication
│   │   ├── api/                 # REST API
│   │   │   ├── mod.rs
│   │   │   └── routes.rs        # Axum routes
│   │   ├── notification/        # Notification engine
│   │   │   ├── mod.rs
│   │   │   ├── store.rs         # In-memory store
│   │   │   ├── desktop.rs       # OS notifications
│   │   │   └── sound.rs         # Sound alerts
│   │   ├── config/              # Configuration
│   │   │   ├── mod.rs
│   │   │   └── schema.rs        # TOML schema
│   │   ├── session/             # Session persistence
│   │   │   ├── mod.rs
│   │   │   └── store.rs         # Save/restore
│   │   └── commands.rs          # Tauri IPC commands
│   └── tauri.conf.json
│
├── src/                         # React frontend
│   ├── App.tsx                  # Root component
│   ├── main.tsx                 # Entry point
│   ├── components/
│   │   ├── Sidebar/
│   │   │   ├── Sidebar.tsx      # Main sidebar
│   │   │   ├── WorkspaceList.tsx
│   │   │   ├── WorkspaceRow.tsx
│   │   │   ├── NotificationPanel.tsx
│   │   │   └── SettingsPanel.tsx
│   │   ├── Terminal/
│   │   │   ├── TerminalPane.tsx # xterm.js wrapper
│   │   │   ├── SplitContainer.tsx # Recursive split renderer
│   │   │   └── Divider.tsx      # Draggable divider
│   │   ├── Browser/
│   │   │   ├── BrowserPane.tsx  # Tauri WebView wrapper
│   │   │   └── UrlBar.tsx
│   │   ├── Titlebar/
│   │   │   └── Titlebar.tsx     # Custom titlebar
│   │   └── Notifications/
│   │       ├── NotificationBadge.tsx
│   │       └── NotificationToast.tsx
│   ├── hooks/                   # React hooks
│   │   ├── usePty.ts            # PTY communication
│   │   ├── useMultiplexer.ts    # State management
│   │   └── useNotifications.ts
│   ├── store/                   # State management
│   │   └── index.ts             # Zustand store
│   ├── styles/
│   │   └── globals.css          # Tailwind + custom styles
│   └── lib/
│       ├── tauri.ts             # Tauri API wrappers
│       └── types.ts             # TypeScript interfaces
│
├── cli/                         # CLI binary
│   ├── Cargo.toml
│   └── src/
│       └── main.rs              # codeterm CLI
│
├── skills/                      # Claude Code skills
│   ├── codeterm/
│   │   └── SKILL.md
│   ├── codeterm-notify/
│   │   └── SKILL.md
│   ├── codeterm-canvas/
│   │   └── SKILL.md
│   └── codeterm-layout/
│       └── SKILL.md
│
├── themes/                      # Built-in themes
│   ├── dracula.toml
│   ├── catppuccin-mocha.toml
│   ├── one-dark.toml
│   └── nord.toml
│
├── package.json                 # Frontend deps
├── pnpm-lock.yaml
├── tsconfig.json
├── tailwind.config.js
├── vite.config.ts
└── README.md
```

---

## 8. AGENT TASK BREAKDOWN

Each phase broken into tasks suitable for delegation to Claude Code subagents.

### Phase 0 Tasks (can be parallelized)
```
Agent 0A: Tauri v2 + React project setup
  - Initialize project, configure build for Windows
  - Custom titlebar, app icon, window settings

Agent 0B: PTY integration
  - Add portable-pty, implement spawn/read/write/resize
  - Tauri IPC commands for PTY operations
  - Event streaming (PTY output → frontend)

Agent 0C: xterm.js integration
  - Add xterm.js with WebGL addon
  - React component wrapping xterm.js Terminal
  - Connect to Tauri PTY events
  - Handle resize (addon-fit)
```

### Phase 1 Tasks
```
Agent 1A: Multiplexer data model (Rust)
  - Workspace/Tab/Pane/SplitNode structs
  - CRUD operations, ID management
  - Tauri commands exposing multiplexer API

Agent 1B: Split tree layout (Rust + React)
  - SplitNode algorithm (calculate bounds)
  - React SplitContainer (recursive renderer)
  - Divider drag-resize

Agent 1C: Tab/Workspace UI (React)
  - Sidebar component
  - Tab list with drag-reorder
  - Workspace switcher
  - Keyboard shortcuts
```

### Phase 2 Tasks
```
Agent 2A: Agent detection (Rust)
  - OSC scanner in byte stream
  - Prompt pattern matcher
  - Status state machine

Agent 2B: Socket API server (Rust)
  - JSON-RPC v2 server (Unix socket + named pipe)
  - Method dispatch
  - Auth (UID + HMAC)

Agent 2C: Notification system (Rust + React)
  - Notification store
  - Desktop notifications
  - Sound alerts
  - UI (badges, panel, toasts)

Agent 2D: CLI binary
  - clap-based CLI
  - Socket client
  - All commands
```

### Phase 3 Tasks
```
Agent 3A: Skills markdown files
  - Write skill definitions
  - Test with Claude Code

Agent 3B: Agent Teams backend
  - Implement teammate spawning protocol
  - Pane management for sub-agents

Agent 3C: Sidebar metadata
  - Git branch (git2)
  - Listening ports (sysinfo)
  - CWD tracking
```

---

## 9. OPEN QUESTIONS (RESOLVED)

| Question | Decision | Rationale |
|----------|----------|-----------|
| **Name** | CodeTerm | Clear, descriptive, available |
| **License** | MIT | Maximum adoption, WezTerm crates are MIT too |
| **WezTerm fork vs clean** | Clean build using WezTerm crates | Fork is unmaintainable at 500K LOC |
| **Tauri vs native** | Tauri v2 | Cross-platform UI fast, built-in WebView for browser |
| **Browser in MVP?** | Phase 4 (early post-core) | Important but not blocking core value |
| **Pricing** | Open source (MIT), commercial support later | Build community first |
| **Frontend framework** | React | Largest ecosystem, best for complex UI |
| **Config format** | TOML | Rust-native, human-readable |
| **Socket protocol** | JSON-RPC v2 | cmux-compatible, proven |

---

## 10. RISK ASSESSMENT

| Risk | Impact | Mitigation |
|------|--------|------------|
| xterm.js performance in Tauri WebView | Medium | WebGL addon solves 90% of cases. Benchmark early. |
| portable-pty Windows ConPTY edge cases | Medium | WezTerm community already handles most issues. Test thoroughly on Windows. |
| Tauri WebView differences across platforms | Low | Tauri v2 handles abstraction. Test on all 3 OS. |
| Browser panel (secondary WebView) complexity | Medium | Start with simple URL loading, add JS eval later. |
| Socket API named pipes on Windows | Low | interprocess crate handles cross-platform. |
| xterm.js + multiple instances performance | Medium | Lazy rendering for hidden tabs. Limit visible terminals. |
| Binary size growth | Low | Tauri is ~5MB base. Monitor with each dependency. |

---

## 11. SUCCESS METRICS

**MVP (Phases 0-2 complete):**
- Terminal works on Windows, macOS, Linux
- Multiple tabs and split panes
- Agent waiting detection works for Claude Code
- Socket API functional — `codeterm split`, `codeterm notify` work
- Desktop notifications fire when agent needs attention

**v1.0 (Phases 0-4 complete):**
- Claude Code skills work out of the box
- Agent Teams backend is valid for Claude Code
- Browser panel with URL loading
- Theming and config system
- Stable, daily-driveable for AI agent development

**Growth indicators:**
- GitHub stars
- Claude Code Agent Teams adoption
- Rollo/webhook integrations
- Community-contributed skills and themes
