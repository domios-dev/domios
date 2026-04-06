# CodeTerm

**OS for AI agents.** Cross-platform terminal multiplexer where AI agents live, work, and coordinate. Tauri v2 (Rust) + React/TypeScript + xterm.js.

## Vision

CodeTerm is a **distributed OS for AI agents**. The CLI and APIs are the "spinal cord" — low-level reflexes any agent uses to act in the environment. The General Agent is the brain of one machine; other agents get the spinal cord (skill) but not its identity. Agent-agnostic: Claude, Codex, Gemini, local models. Mesh evolves toward unified distributed fabric with shared memory and project awareness. Full vision: `VISION.md`.

## Quick ref

- All code/comments in English
- See `docs/TESTING.md` for full testing guide

## Running dev & tests

All commands go through `build.sh` — one script for all platforms. Safe on Windows: never kills production CodeTerm, only dev instance (port 7686).

```bash
./build.sh dev              # start dev server (incremental, hot reload, auto-builds CLI on start)
./build.sh check            # cargo check only (~2s incremental)
./build.sh test             # Rust unit tests
./build.sh build            # production build (includes CLI + bundling)
./build.sh cli              # rebuild CLI only → ~/.codeterm/bin/ (no dev restart needed)
./build.sh clean            # wipe build cache (next build from scratch)
./build.sh kill             # kill dev instance only (safe)
./build.sh webdriver        # dev with WebDriver for UI tests
./build.sh e2e              # E2E tests against dev (port 7686)
source build.sh env         # load MSVC env into shell (Windows, for manual cargo commands)
```

**CLI note:** `./build.sh dev` auto-builds CLI on every start. If you change CLI code (cli/ crate) mid-session, use `./build.sh cli` separately — no dev restart needed.

### Tests (3 layers)

```bash
# 1. UNIT — no instance needed, fast
./build.sh test                                           # Rust (~290 tests)
npx tsx src/components/ChatView/parser.test.ts             # TS (~100 tests)

# 2. E2E — needs running dev instance (./build.sh dev)
./build.sh e2e                                            # runs against port 7686
# Remote: CODETERM_API_URL=http://HOST:PORT CODETERM_API_TOKEN=TOKEN pytest tests/e2e/ -v

# 3. UI — needs dev with WebDriver (./build.sh webdriver)
cd tests/webdriver && npm test
```

### Dev logs
- macOS/Linux: `~/.codeterm-dev/logs/codeterm.log`
- Windows: `%USERPROFILE%\.codeterm-dev\logs\codeterm.log`
- Live view: `tail -f ~/.codeterm-dev/logs/codeterm.log`

### Build verification — MANDATORY before debugging

`build.sh dev` increments `BUILD_NUMBER` on each run. The running instance exposes it via health API.

**Before debugging or testing any change, ALWAYS:**
1. Check logs: `tail -30 ~/.codeterm-dev/logs/codeterm.log`
2. Verify the running build matches your code:
   ```bash
   curl -s http://127.0.0.1:7686/api/health | jq '{build, build_number}'
   ```
   Compare `build` (git commit hash) and `build_number` with what you expect. If they don't match, the old build is still running — your changes aren't live yet.
3. Only THEN start debugging. Don't waste time investigating behavior from a stale build.

## Agent roles & orchestration

You are the **Lead** inside this project. You orchestrate specialized workers. The hierarchy is:

```
Lead (you, running in a CodeTerm pane)
├── backend-dev   — Rust code in src-tauri/
├── frontend-dev  — React/TS code in src/
├── docs          — all *.md docs, skills, READMEs
├── code-reviewer — read-only review of diffs/files
└── devops        — builds, releases, CI/CD
```

Agent definitions live in `.claude/agents/*.md` (standard Claude Code format + they double as prompts when spawning via `codeterm agent spawn`).

### When to delegate vs do-it-yourself

**Delegate** when the task is scoped, parallelizable, or outside your current context:
- "Add these 15 tests to service/pane.rs" → backend-dev
- "Extract this hook from the component" → frontend-dev
- "Review the last 3 commits for issues" → code-reviewer
- "Update docs after these changes" → docs
- "Bump version and release" → devops

**Do-it-yourself** when the task is small, architectural, or cross-cutting:
- Single-file edit under ~50 lines
- Architecture decisions (splitting modules, choosing patterns)
- Multi-module refactors spanning 5+ files across both frontend and backend
- Tasks that require holding the full project context

### How to spawn workers

Two mechanisms:

**1. Built-in Agent tool** (in-process, cheap, fast):
```
Agent(subagent_type="backend-dev", prompt="...")
```
Use for: quick research, small isolated tasks, one-off subtasks. The agent runs in your context and returns a message. No pane is created.

**2. `codeterm agent spawn claude`** (heavyweight, visible, dogfooded):
```bash
codeterm agent spawn claude \
  --role Worker --parent $CODETERM_PANE_ID \
  --title "backend-dev: add pane tests" \
  --task "$(cat .claude/agents/backend-dev.md)

TASK: ..."
```
Use for: bigger parallelizable tasks, when the user should watch work happen, when the worker needs its own PTY and real filesystem access. Reports auto-inject into your pane as `[Report from ...]`.

**Rule of thumb:** if the task is >15 min of real work, spawn via `codeterm agent spawn` so the user can observe. If it's quick research/edit, use the built-in tool.

### Reporting back

Workers report via `codeterm agent report "..."`. Reports auto-inject into your terminal. Do NOT poll or sleep — just continue working. When a report arrives, act on it.

Worker report template (enforced via `worker_suffix.md`):
```
STATUS: done | blocked | partial
CHANGES: files touched
BUILD/TYPECHECK/TESTS: result
FOLLOWUP: anything Lead should know
```

### Recursion limits

- Workers (backend-dev, frontend-dev, code-reviewer, docs) MUST NOT spawn other workers. If they need decomposition, they report back and you spawn siblings.
- devops is special — it coordinates multi-machine builds and may spawn helpers. Still ultimately reports to you.

### Proactive delegation rules

**After significant changes, auto-invoke `docs`.** Significant = new Tauri command, new CLI subcommand, new config option, new feature, API breaking change, README-visible behavior change. Don't ask — just do `Agent(subagent_type="docs", prompt="sync docs with HEAD~N")`.

**Before merging feat branch to develop, auto-invoke `code-reviewer`** on the full branch diff. Act on Critical + High findings before merge.

**Agent selection is by artifact, not by task verb.** Rust work → backend-dev. React/TS → frontend-dev. Docs → docs. Reviews → code-reviewer. Builds → devops.

### Dogfooding

This project IS CodeTerm. You're running inside CodeTerm. Every spawn via `codeterm agent spawn` is both productive work AND a test of the system. If something feels clunky — the codeterm CLI, the mesh, the reports, the panes — note it and tell the user. We improve CodeTerm by using it.

## Documentation

- `docs/API.md` — REST API, Socket API, CLI reference
- `docs/CONFIG.md` — Configuration options, themes, keybindings
- `docs/SKILLS.md` — Claude Code skill for CodeTerm control
- `docs/BUILD.md` — Build instructions
- `docs/MESH.md` — Mesh networking spec (flat peer model, no crown)

Each doc has a `<!-- revision: N -->` tag at the bottom. When you change API endpoints, config options, CLI commands, or skills — bump the revision in affected docs. This lets users detect when to update their local skill/config copies.

## Agent prompts

Prompt templates compiled into the binary via `include_str!()`. Edit `.md` files, recompile to apply.

- `src-tauri/prompts/general_agent.md` — General Agent system prompt (CLAUDE.md template)
- `src-tauri/prompts/bootstrap_instructions.md` — First-run onboarding instructions
- `src-tauri/prompts/worker_suffix.md` — Appended to Worker tasks (report instructions)
- `src-tauri/prompts/lead_suffix.md` — Appended to Lead tasks (spawn + no-sleep)
- `src-tauri/prompts/orchestrator_suffix.md` — Appended to Orchestrator tasks
- `src-tauri/prompts/codeterm_rule.md` — Installed to `~/.claude/rules/codeterm.md`
- `src-tauri/prompts/cron_complete_suffix.md` — Appended to cron tasks (completion instruction)
- `src-tauri/src/prompts.rs` — Rust module that loads all templates

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

## Code style: SOLID, DRY, no long comments

**SOLID + DRY are non-negotiable.** Single responsibility per module, dependencies flow through explicit interfaces, no repeated logic. Duplicated 20-line blocks across three files is a refactor signal, not a copy-paste signal.

**FORBIDDEN: comments longer than 1 line.** Hard limit: 1 line per comment, no exceptions for "context" or "explanation". If code needs more — refactor, rename, or split into functions. The ONLY allowed multi-line case is a literal external reference (issue/spec URL + one-line why). Paragraph comments, "Step 1/Step 2" breakdowns, and PR-description-style explanations inside functions are BANNED. When you feel the urge to write a long comment, that urge IS the bug — fix the code instead.

### Rust specifics

- `parking_lot::Mutex` for shared state. No `tokio::sync::Mutex` unless holding across `.await`. Follow lock order below.
- `Result<T, String>` for Tauri command errors. Custom error enums only when callers branch on variant.
- Modules have single responsibility. `mod.rs` >500 lines = split.
- Tests inline in `#[cfg(test)] mod tests` at bottom of the source file.
- `#[tauri::command]` and REST/Socket handlers stay thin — delegate to `ServiceContext` or `dispatch::dispatch`.
- No `unwrap()`/`expect()` in library code unless a documented invariant.
- Structural patterns over hardcoded strings for detection.

### TypeScript + React specifics

- Custom hook for any logic that touches refs, effects, or subscriptions. Components are mostly JSX + handlers.
- `useRef` for values that don't trigger re-render. Prefer refs over state for imperative APIs (xterm, mutex-like state).
- `useCallback`/`useMemo` only when there's a measurable perf problem.
- Effect dependencies: complete, no `eslint-disable`. If exhaustive-deps complains, the effect is buggy.
- StrictMode-safe: guard side-effectful `useEffect([])` with `useRef<boolean>` if you can't make it idempotent.
- No `any`. Use `unknown` + narrowing or a typed cast.
- Components >400 lines = extract hooks. Hooks >200 lines = extract pure functions to `lib/`.

### Architecture guardrails

- Adapters (REST, Socket, Tauri IPC, CLI) never contain business logic. They deserialize → dispatch → serialize.
- Business logic lives in `service/` or the target `dispatch::Command` layer.
- Core (`multiplexer`, `pty`, `mesh`, `config`) has no outward dependencies.
- Every operation reachable from 3 transports (REST + Socket + Tauri) should live in `dispatch::Command` — not duplicated 3 times.
- Plugin-ready = isolated module with explicit interface. See `docs/plans/plugin-architecture.md`.

## Lock ordering (deadlock prevention)

All parking_lot Mutexes must be acquired in this order. Never hold a higher-numbered lock while acquiring a lower-numbered one.

1. `mux` (MultiplexerState)
2. `output_buf` (OutputBuffer)
3. `screen_state` (ScreenState)
4. `agent_tracker` (PaneStatusTracker)
5. `pty` (PtyManager)

**Rules:**
- Never hold two locks simultaneously if possible — lock, read/write, drop, then lock the next.
- parking_lot Mutex is NOT reentrant — `mux.lock()` inside `mux.lock()` = instant deadlock.
- After parallel Rust refactors, grep for nested `.lock()` calls to verify ordering.

## Process safety

**NEVER kill prod CodeTerm.** Before killing any `codeterm.exe`, identify dev vs prod by port:
```bash
# Find dev PID (port 7686):
netstat -ano | grep :7686 | grep LISTENING | awk '{print $5}'
# Find prod PID (port 7685) — NEVER kill this:
netstat -ano | grep :7685 | grep LISTENING | awk '{print $5}'
```
Only kill the dev PID. If unsure — don't kill anything, ask the user.

## Git rules

**NEVER commit or push without explicit user request.** No auto-commits, no "let me commit this". Only commit when the user says "коммить", "commit", "запушь", etc.

## Cross-platform notes

**Always verify cross-platform correctness.** Every feature must work on macOS, Windows, and Linux. Never add platform-specific code without handling all targets.

- OSC 7 CWD tracking: injected into PowerShell prompt on Windows. On macOS/Linux, bash/zsh usually emit OSC 7 natively.
- PTY: uses `portable-pty` crate (ConPTY on Windows, native on Unix)
- Paths: use forward slashes in code, OS-native in user-facing output
- Keybindings: `Ctrl` in config maps to `Cmd` on macOS, `Ctrl` on Win/Linux. Use `e.code` (not `e.key`) for keyboard layout independence.
- Tauri plugins (dialog, notification, shell): cross-platform by design, prefer over custom solutions.
