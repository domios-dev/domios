<p align="center">
  <img src="assets/logo.png" alt="CodeTerm" width="128" />
</p>

<h1 align="center">CodeTerm</h1>

<p align="center">
  <strong>AI-first terminal multiplexer</strong><br>
  Built for developers who use AI coding agents as part of their workflow.
</p>

<p align="center">
  <a href="#features">Features</a> &bull;
  <a href="#installation">Installation</a> &bull;
  <a href="#keyboard-shortcuts">Shortcuts</a> &bull;
  <a href="#api">API</a> &bull;
  <a href="#configuration">Config</a> &bull;
  <a href="#building-from-source">Build</a>
</p>

<p align="center">
  <a href="README.zh.md">中文</a> &bull;
  <a href="README.ja.md">日本語</a> &bull;
  <a href="README.ko.md">한국어</a> &bull;
  <a href="README.es.md">Español</a> &bull;
  <a href="README.de.md">Deutsch</a> &bull;
  <a href="README.fr.md">Français</a> &bull;
  <a href="README.pt.md">Português</a> &bull;
  <a href="README.ru.md">Русский</a>
</p>

<!-- SCREENSHOT PLACEHOLDER: Add screenshots here -->
<!-- ![CodeTerm Screenshot](assets/screenshot.png) -->

---

## What is CodeTerm?

CodeTerm is a cross-platform terminal multiplexer designed from the ground up for AI-assisted development. It combines the power of a terminal multiplexer (like tmux) with first-class support for AI coding agents like Claude Code.

**Key difference from other terminals:** CodeTerm exposes a full REST API, CLI, and Socket API — so AI agents can create workspaces, split panes, run commands, read output, and manage the entire terminal programmatically. Your AI assistant becomes a first-class citizen in your terminal workflow.

Built with **Tauri v2** (Rust) + **React** + **xterm.js**.

## Features

### Terminal Multiplexer
- **Workspaces & tabs** — organize projects into separate workspaces, each with multiple tabs
- **Pane splitting** — split horizontally and vertically with draggable dividers
- **Session persistence** — workspaces, tabs, pane layout, and working directories are saved and restored across restarts
- **Spatial pane navigation** — move between panes with Alt+Arrow keys

### AI Agent Integration
- **Agent detection** — automatically detects Claude Code running in a pane (orange indicator)
- **Session resume** — saves Claude Code session IDs and resumes them on restart with `--resume`
- **Permission notifications** — surfaces Claude Code permission requests as desktop notifications
- **Webhook system** — register webhooks for agent events (permission requests, task completion)
- **Programmatic control** — agents can create panes, run commands, read output via API

### Full API
- **REST API** — 30+ endpoints for complete terminal control (port 7685)
- **CLI tool** — `codeterm` command-line tool for scripting and automation
- **Socket API** — JSON-RPC v2 over named pipes (Windows) / Unix sockets
- **Real-time streaming** — SSE endpoint for live terminal output
- **Output capture** — cursor-based pagination, ANSI stripping, screen snapshots

### Theming
- **8 built-in themes** — GitHub Dark, Dracula, Catppuccin Mocha, One Dark, Nord, GitHub Light, Solarized Light, Pip-Boy
- **Pip-Boy CRT mode** — scanlines, phosphor glow, screen flicker (activated automatically)
- **Full UI theming** — themes apply to the entire interface, not just the terminal
- **Custom colors** — override individual colors in `config.toml`
- **Light theme support** — automatic color derivation works for both light and dark themes

### Built-in Browser
- **Browser panes** — open web pages side-by-side with your terminal
- **Local file serving** — render local HTML/visualizations in browser panes
- **Navigation controls** — back, forward, refresh, URL bar

### Developer Experience
- **macOS-style scrollbars** — thin, rounded, unobtrusive
- **Drag & drop** — drop files into terminal (text files paste content, images save to disk)
- **Tab completion** — built-in shell completion engine
- **Search** — Ctrl+Shift+F to search terminal output
- **Visual bell** — screen flash instead of audio
- **CWD tracking** — tracks working directory via OSC 7 escape sequences

## Installation

### Download

Pre-built binaries will be available on the [Releases](https://github.com/rollacode/codeterm/releases) page.

### Building from Source

**Prerequisites:**
- [Node.js](https://nodejs.org/) 18+
- [pnpm](https://pnpm.io/) 10+
- [Rust](https://rustup.rs/) 1.75+
- Platform-specific build tools (Visual Studio Build Tools on Windows, Xcode on macOS)

```bash
git clone https://github.com/rollacode/codeterm.git
cd codeterm
pnpm install

# Development
pnpm tauri dev

# Production build
pnpm tauri build

# CLI tool
cd cli && cargo build --release
```

## Keyboard Shortcuts

| Shortcut | Action |
|----------|--------|
| `Ctrl+Shift+T` | New tab |
| `Ctrl+Shift+W` | Close tab |
| `Ctrl+Shift+D` | Split right |
| `Ctrl+Shift+E` | Split down |
| `Ctrl+Shift+Q` | Close pane |
| `Ctrl+Shift+N` | New workspace |
| `Ctrl+Shift+B` | Open browser pane |
| `Ctrl+Shift+F` | Search in terminal |
| `Ctrl+Shift+C` | Copy |
| `Ctrl+Shift+V` | Paste |
| `Ctrl+Tab` | Next tab |
| `Ctrl+Shift+Tab` | Previous tab |
| `Ctrl+1-9` | Switch to tab N |
| `Ctrl+,` | Settings |
| `Ctrl+=` / `Ctrl+-` | Zoom in/out |
| `Ctrl+0` | Reset zoom |
| `Alt+Arrows` | Navigate panes |

## API

CodeTerm exposes three APIs for integration:

### REST API (port 7685)

```bash
# List panes
curl http://localhost:7685/api/panes

# Execute a command and get output
curl -X POST http://localhost:7685/api/panes/{id}/exec \
  -H "Content-Type: application/json" \
  -d '{"command": "git status"}'

# Send text to terminal
curl -X POST http://localhost:7685/api/panes/{id}/send \
  -H "Content-Type: application/json" \
  -d '{"text": "npm test", "enter": true}'

# Create workspace
curl -X POST http://localhost:7685/api/workspaces \
  -H "Content-Type: application/json" \
  -d '{"name": "My Project"}'

# Split pane
curl -X POST http://localhost:7685/api/panes/{id}/split \
  -H "Content-Type: application/json" \
  -d '{"direction": "right"}'

# Stream output in real-time (SSE)
curl http://localhost:7685/api/panes/{id}/output/stream
```

### CLI Tool

```bash
# Workspace management
codeterm workspace list
codeterm workspace create "Project"

# Terminal control
codeterm pane list
codeterm pane split PANE_ID --direction right
codeterm send-text PANE_ID "npm run build"
codeterm get-output PANE_ID --lines 50

# Notifications
codeterm notify "Build complete" --level info

# Browser
codeterm browser open "http://localhost:3000"
```

### Environment Variables

CodeTerm sets these in every PTY session:

| Variable | Description |
|----------|-------------|
| `CODETERM` | Always `1` — detect if running inside CodeTerm |
| `CODETERM_PANE_ID` | UUID of the current pane |
| `CODETERM_SOCKET_PATH` | Socket path for JSON-RPC API |
| `TERM_PROGRAM` | `CodeTerm` |

See [API Reference](docs/API.md) for the complete endpoint documentation.

## Configuration

Config file: `%APPDATA%/codeterm/config.toml` (Windows) or `~/.config/codeterm/config.toml`

```toml
[theme]
name = "github-dark"
# Available: github-dark, dracula, catppuccin-mocha, one-dark, nord,
#            github-light, solarized-light, pip-boy

[font]
family = "JetBrains Mono, Cascadia Code, Fira Code, Consolas, monospace"
size = 14
ligatures = true

[terminal]
cursor_style = "block"    # block, underline, bar
cursor_blink = true
scrollback = 10000
copy_on_select = false

[api]
enabled = true
port = 7685
# token = "your-secret-token"
```

See [Configuration Guide](docs/CONFIG.md) for all options.

## Architecture

```
┌─────────────────────────────────────────────┐
│                  CodeTerm                    │
│  ┌──────────┐  ┌──────────┐  ┌───────────┐  │
│  │ React UI │  │ xterm.js │  │  Browser   │  │
│  │ Sidebar  │  │ Terminal │  │   Panes    │  │
│  └────┬─────┘  └────┬─────┘  └─────┬─────┘  │
│       │              │              │        │
│  ┌────┴──────────────┴──────────────┴────┐   │
│  │           Tauri v2 (Rust)             │   │
│  │  ┌────────┐ ┌─────┐ ┌─────────────┐  │   │
│  │  │  Mux   │ │ PTY │ │  REST API   │  │   │
│  │  │ State  │ │ Mgr │ │  (Axum)     │  │   │
│  │  └────────┘ └─────┘ └─────────────┘  │   │
│  │  ┌────────┐ ┌─────────┐ ┌─────────┐  │   │
│  │  │Session │ │  Agent  │ │ Socket  │  │   │
│  │  │Persist │ │Detector │ │JSON-RPC │  │   │
│  │  └────────┘ └─────────┘ └─────────┘  │   │
│  └───────────────────────────────────────┘   │
└─────────────────────────────────────────────┘
```

## Tech Stack

| Layer | Technology |
|-------|-----------|
| Framework | Tauri v2 |
| Backend | Rust, Axum, tokio |
| Frontend | React 19, TypeScript, Tailwind CSS 4 |
| Terminal | xterm.js 6 (WebGL renderer) |
| PTY | portable-pty, ConPTY (Windows) |
| Build | pnpm, Cargo |

## Platforms

| Platform | Status |
|----------|--------|
| Windows | Supported |
| macOS | Supported |
| Linux | Supported |

## License

MIT

## Contributing

Contributions welcome. Please open an issue first to discuss what you'd like to change.
