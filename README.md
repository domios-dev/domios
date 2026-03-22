<p align="center">
  <img src="src-tauri/icons/icon.png" alt="CodeTerm" width="128" />
</p>

<h1 align="center">CodeTerm</h1>

<p align="center">
  <strong>AI-first terminal multiplexer</strong>
</p>

CodeTerm is a cross-platform terminal multiplexer built for developers who work with AI coding agents. It combines tmux-style workspace management with a full programmatic API, so your AI assistant can create panes, run commands, and orchestrate workflows alongside you.

Built with Tauri v2 (Rust) + React + xterm.js.

## Features

- **Workspaces and split panes** — organize projects into workspaces with tabs and horizontal/vertical splits
- **Agent orchestration** — spawn and coordinate AI agents with conductor/worker roles via REST API
- **Claude Code integration** — auto-detects Claude Code sessions, resumes them on restart, surfaces permission requests as notifications
- **Canvas panels** — open browser panes side-by-side with your terminal for dashboards, docs, or local HTML
- **Full API** — 30+ REST endpoints, CLI tool, JSON-RPC socket, and SSE streaming for complete programmatic control
- **Cross-platform** — Windows, macOS (ARM + Intel), and Linux

## Download

Grab the latest release for your platform:

**[Download CodeTerm](https://github.com/rollacode/codeterm/releases)**

## Quick Start

```bash
git clone https://github.com/rollacode/codeterm.git
cd codeterm
pnpm install
pnpm tauri dev
```

## Documentation

- [API Reference](docs/API.md) — REST API, Socket API, CLI
- [Configuration](docs/CONFIG.md) — Themes, keybindings, settings
- [Build Guide](docs/BUILD.md) — Platform-specific build instructions

## License

MIT

## Author

Created by [Andrey Kovalev](https://github.com/rollacode)

[rollacode.org](https://rollacode.org)
