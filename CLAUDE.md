# CodeTerm

AI-first cross-platform terminal multiplexer. Tauri v2 (Rust) + React/TypeScript + xterm.js.

## Quick ref

- `pnpm tauri dev` — dev mode
- `pnpm tauri build` — production
- `cd cli && cargo build --release` — CLI tool
- All code/comments in English

## Documentation

- `docs/API.md` — REST API, Socket API, CLI reference
- `docs/CONFIG.md` — Configuration options, themes, keybindings
- `docs/SKILLS.md` — Claude Code skill for CodeTerm control
- `docs/AGENT_PROMPT.md` — Agent integration prompt
- `docs/BUILD.md` — Build instructions

Each doc has a `<!-- revision: N -->` tag at the bottom. When you change API endpoints, config options, CLI commands, or skills — bump the revision in affected docs. This lets users detect when to update their local skill/config copies.

## Cross-platform notes

- OSC 7 CWD tracking: injected into PowerShell prompt on Windows. On macOS/Linux, bash/zsh usually emit OSC 7 natively.
- PTY: uses `portable-pty` crate (ConPTY on Windows, native on Unix)
- Paths: use forward slashes in code, OS-native in user-facing output
