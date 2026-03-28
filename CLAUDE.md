# CodeTerm

**OS for AI agents.** Cross-platform terminal multiplexer where AI agents live, work, and coordinate. Tauri v2 (Rust) + React/TypeScript + xterm.js.

## Vision

CodeTerm is not a terminal with AI bolted on. It's infrastructure that AI agents need — lifecycle management, observability, coordination, verification — wrapped in a terminal because that's where developers work. Model-agnostic: Claude, Codex, Gemini, local models — any agent that runs in a terminal.

## Quick ref

- All code/comments in English
- See `docs/TESTING.md` for full testing guide

## Running dev & tests

### Dev server

**macOS/Linux:**
```bash
./build.sh dev                    # auto-kills previous instance
# With WebDriver for UI tests:
pnpm tauri dev --config src-tauri/tauri.dev.conf.json -- --features webdriver
```

**Windows (PowerShell):**
```powershell
# Set MSVC environment first (see build.sh for paths)
pnpm tauri dev --config src-tauri/tauri.dev.conf.json
# With WebDriver:
pnpm tauri dev --config src-tauri/tauri.dev.conf.json -- --features webdriver
```

**IMPORTANT on Windows:** Do NOT use `./build.sh` — it's a bash script. Use `pnpm tauri dev` directly. Do NOT kill the dev process by terminating the shell — use Ctrl+C or close the window.

### Tests (3 layers)

```bash
# 1. UNIT — no instance needed, fast
cargo test --manifest-path src-tauri/Cargo.toml --lib    # Rust (210 tests)
npx tsx src/components/ChatView/parser.test.ts            # TS (100 tests)

# 2. E2E — needs running dev instance (./build.sh dev)
CODETERM_API_URL=http://127.0.0.1:7686 pytest tests/e2e/ -v   # 84 tests
# Remote: CODETERM_API_URL=http://HOST:PORT CODETERM_API_TOKEN=TOKEN pytest tests/e2e/ -v

# 3. UI — needs dev with WebDriver (pnpm tauri dev ... -- --features webdriver)
cd tests/webdriver && npm test                            # 19 tests
```

### Dev logs
- macOS/Linux: `~/.codeterm-dev/logs/codeterm.log`
- Windows: `%USERPROFILE%\.codeterm-dev\logs\codeterm.log`
- Live view: `tail -f ~/.codeterm-dev/logs/codeterm.log`

## Agent roles

- **devops** agent — builds, releases, CI/CD, version bumps, tagging, GitHub Actions, `build.sh` maintenance. Delegate all build/release/deploy tasks to it.
- **docs** agent — keeps `docs/API.md`, `docs/CONFIG.md`, `docs/SKILLS.md`, `docs/AGENT_PROMPT.md`, `skills/codeterm/SKILL.md` in sync with code. Delegate doc updates to it after feature changes.

## Documentation

- `docs/API.md` — REST API, Socket API, CLI reference
- `docs/CONFIG.md` — Configuration options, themes, keybindings
- `docs/SKILLS.md` — Claude Code skill for CodeTerm control
- `docs/AGENT_PROMPT.md` — Agent integration prompt
- `docs/BUILD.md` — Build instructions

Each doc has a `<!-- revision: N -->` tag at the bottom. When you change API endpoints, config options, CLI commands, or skills — bump the revision in affected docs. This lets users detect when to update their local skill/config copies.

## Architecture: Agent Providers

Agent detection and integration is abstracted into providers. Currently only Claude Code is implemented, but the architecture supports any CLI agent.

**What a provider does:**
- **Detect** — recognize when an agent starts in a PTY (OSC title, output patterns, process name)
- **Session** — link to session files for structured data (Claude: JSONL, Codex: TBD)
- **Parse** — extract structured messages from agent output (chat view)
- **Control** — send input to the agent, understand its prompt format

**Current providers:**
- `claude` — detects via OSC title containing "claude", reads `~/.claude/projects/` JSONL, parses messages/tool_calls/thinking blocks

**Future providers (architecture ready, not implemented):**
- `codex` — OpenAI Codex CLI
- `aider` — Aider pair programmer
- `goose` — Block's Goose agent
- `generic` — fallback, terminal output parsing only

Provider code lives in:
- Rust: `src-tauri/src/agent/` — detection, session file reading
- TypeScript: `src/components/ChatView/parser.ts` — output parsing
- TypeScript: `src/components/Terminal/TerminalPane.tsx` — detection hooks (onTitleChange)

## Multi-agent mindset

**Always design for multiple agent types, not just Claude.** Every feature that interacts with agents must go through the AgentProvider abstraction. Never hardcode Claude-specific logic in generic code. If a behavior is Claude-specific (clipboard paste for images, JSONL session format, TUI noise patterns), it belongs in ClaudeProvider or the Claude provider config — not in ChatView, TerminalPane, or other generic components.

When adding agent-related features, ask: "How would this work with Codex? With Aider? With a local model?" If the answer requires Claude-specific code, put it in the provider.

## No hardcode — always patterns

**Never hardcode specific strings for detection.** Always use structural patterns/fingerprints. If Claude changes wording from "Do you want to proceed?" to "Do you want to create?", detection must still work. Same for any agent — detect by structure (numbered options + footer), not by specific text. This applies everywhere: prompt detection, task completion, agent identification, output parsing. Generalize, don't enumerate.

## Lock ordering (deadlock prevention)

All parking_lot Mutexes must be acquired in this order. Never hold a higher-numbered lock while acquiring a lower-numbered one.

1. `mux` (MultiplexerState)
2. `output_buf` (OutputBuffer)
3. `agent_tracker` (PaneStatusTracker)
4. `pty` (PtyManager)

**Rules:**
- Never hold two locks simultaneously if possible — lock, read/write, drop, then lock the next.
- parking_lot Mutex is NOT reentrant — `mux.lock()` inside `mux.lock()` = instant deadlock.
- After parallel Rust refactors, grep for nested `.lock()` calls to verify ordering.

## Git rules

**NEVER commit or push without explicit user request.** No auto-commits, no "let me commit this". Only commit when the user says "коммить", "commit", "запушь", etc.

## Cross-platform notes

**Always verify cross-platform correctness.** Every feature must work on macOS, Windows, and Linux. Never add platform-specific code without handling all targets.

- OSC 7 CWD tracking: injected into PowerShell prompt on Windows. On macOS/Linux, bash/zsh usually emit OSC 7 natively.
- PTY: uses `portable-pty` crate (ConPTY on Windows, native on Unix)
- Paths: use forward slashes in code, OS-native in user-facing output
- Keybindings: `Ctrl` in config maps to `Cmd` on macOS, `Ctrl` on Win/Linux. Use `e.code` (not `e.key`) for keyboard layout independence.
- Tauri plugins (dialog, notification, shell): cross-platform by design, prefer over custom solutions.
