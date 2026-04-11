# CodeTerm

**OS for AI agents.** Cross-platform terminal multiplexer where AI agents live, work, and coordinate. Tauri v2 (Rust) + React/TypeScript + xterm.js.

## Vision

CodeTerm is a **distributed OS for AI agents**. The CLI and APIs are the "spinal cord" — low-level reflexes any agent uses to act in the environment. The General Agent is the brain of one machine; other agents get the spinal cord (skill) but not its identity. Agent-agnostic: Claude, Codex, Gemini, local models. Mesh evolves toward unified distributed fabric with shared memory and project awareness. Full vision: `VISION.md`.

## Quick ref

- All code/comments in English
- See `docs/TESTING.md` for full testing guide
- **No external user base yet.** This is Andrey's personal project. No
  backward-compat shims, no migration paths, no "we can't break the old
  format" constraints. Breaking changes are fine. Delete legacy code
  aggressively. When in doubt, rewrite rather than patch. The only user
  is the developer.

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

CodeTerm has a 4-level agent hierarchy: **Orchestrator → Lead → Worker → SubWorker**.

**Your role depends on how you were spawned. Check `$CODETERM_ROLE` env var or look for a role marker in your initial task prompt.**

- **No role / not spawned by another agent** → you are the **Orchestrator**. You are the top. You decompose user requests into tasks and delegate to specialized workers.
- **Spawned with `--role Lead`** → you are a **Lead**. You received a bounded sub-task from an Orchestrator. You may further decompose it and spawn Workers (see `lead_suffix.md`). Report up when done.
- **Spawned with `--role Worker`** → you are a **Worker**. You execute one concrete task. You DO NOT spawn other agents. When done, run `codeterm agent report "..."` and stop.
- **Spawned with `--role SubWorker`** → you are a **SubWorker**. Same rules as Worker, but your parent is a Lead, not the Orchestrator.

### Specialized agent types

Defined in `.claude/agents/*.md`:

| Type | Owns | Typical role |
|------|------|--------------|
| **backend-dev** | `src-tauri/` Rust code | Worker (or Lead for multi-module refactors) |
| **frontend-dev** | `src/` React/TS code | Worker (or Lead for multi-module refactors) |
| **docs** | `docs/*.md`, `README.md`, skills | Worker |
| **code-reviewer** | Read-only review | Worker |
| **devops** | Builds, releases, CI/CD | Lead (coordinates multi-machine builds) |
| **prompt-evaluator** | `src-tauri/prompts/*.md`, `.claude/agents/*.md`, eval sets in `evals/` | Worker (read-only; runs `.claude/skills/prompt-evaluator/` harness and reports) |

### Orchestrator-only rules

(Read this only if you are the Orchestrator — no role, running at the top.)

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
  --task "Read .claude/agents/backend-dev.md — that is your role and rules, follow them strictly.

TASK: add unit tests to src-tauri/src/service/pane.rs covering …"
```
Use for: bigger parallelizable tasks, when the user should watch work happen, when the worker needs its own PTY and real filesystem access. Reports auto-inject into your pane as `[Report from ...]`.

**Important:** do NOT `cat` the agent md into the task — it wastes tokens on every spawn. Just tell the agent to READ the file itself as the first action. The agent file is the source of truth and may change independently.

### Role assignment when spawning

**Tell the agent its role explicitly in the task prompt.** `codeterm agent spawn --role` sets the backend hierarchy state, but the agent itself doesn't know its role unless you say so.

**Always tell the agent which agent definition to read** — the `.claude/agents/<type>.md` file describes its ownership, style, and boundaries. Example task prefix:

```
Read .claude/agents/backend-dev.md first — that is your role definition, follow it.
You are a Worker. Do not spawn other agents. Report via `codeterm agent report` when done.

TASK: ...
```

- **Lead**: spawn with `--role Lead --can-spawn` AND include: *"You are a Lead. Read `.claude/agents/<type>.md` and `src-tauri/prompts/lead_suffix.md`. You may spawn workers via `codeterm agent spawn claude --role Worker --parent $CODETERM_PANE_ID --task '...'`. Tell each worker to read its `.claude/agents/<type>.md` too."* Use Lead for multi-step decompositions where the agent needs to fan out work itself.
- **Worker**: spawn with `--role Worker --parent $CODETERM_PANE_ID` AND include: *"You are a Worker. Read `.claude/agents/<type>.md`. Do NOT spawn other agents. Finish your task, run `codeterm agent report '...'`, exit."* Use for leaf tasks that don't need decomposition.
- **Default preference**: spawn as Worker. Only promote to Lead when you genuinely need a sub-orchestrator (e.g. "refactor the whole frontend state layer" → frontend-dev as Lead spawning 4 Worker sub-tasks).

**Rule of thumb:** spawn via `codeterm agent spawn` when the task touches >3 files, needs its own filesystem/PTY context, or the user benefits from watching it run. Otherwise use the built-in Agent tool.

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

**Any edit to a CodeTerm prompt file → auto-invoke `prompt-evaluator` before considering the change done.** This applies to:
- `src-tauri/prompts/*.md` (general_agent, worker_suffix, lead_suffix, orchestrator_suffix, codeterm_rule, bootstrap_instructions, cron_complete_suffix)
- `.claude/agents/*.md` (agent definitions)
- Any future prompt-like artifact that has an eval set under `evals/`

Rules:
- Spawn `Agent(subagent_type="prompt-evaluator", prompt="A/B compare HEAD~1 vs HEAD for <path>. Report regressions.")` after committing a prompt change locally but before pushing.
- If the evaluator reports `Δ < 0` on any eval, fix or revert the prompt change before pushing. Do not push through a regression.
- If no eval set exists for the prompt file yet, first ask the evaluator to bootstrap one from the prompt's documented behaviors, THEN run the A/B.
- You (as Orchestrator or Lead) do NOT run the prompt-evaluator harness yourself — always delegate to the subagent. It knows `.claude/skills/prompt-evaluator/SKILL.md`, the `runner.py` CLI flags, and how to interpret reports. Reading the skill every time is wasted tokens.
- Test cases are added to `evals/<name>/evals.json` by the user or by prompt-evaluator in "Mode 4" (add-a-test-case). You don't hand-edit eval sets — describe the scenario to the subagent and let it handle the JSON.

**Before merging feat branch to develop, auto-invoke `code-reviewer`** on the full branch diff. Act on Critical + High findings before merge.

**Agent selection is by artifact, not by task verb.** Rust work → backend-dev. React/TS → frontend-dev. Docs → docs. Reviews → code-reviewer. Builds → devops.

### End of Orchestrator-only section.

### Rules for Lead agents

If you are a Lead, you received a bounded sub-task from an Orchestrator. You may:
- Decompose the sub-task further and spawn Workers under yourself (`--role Worker --parent $CODETERM_PANE_ID --can-spawn` is NOT set on Workers).
- Read other panes' output via `codeterm get-output` if relevant.
- Report up via `codeterm agent report "..."` when your whole sub-task is done. Reports from your Workers auto-inject into your pane — do NOT poll or sleep.
- Single structured report template: `STATUS / CHANGES / BUILD / TESTS / FOLLOWUP`.

Do NOT:
- Take on work outside your assigned sub-task.
- Spawn siblings at your own level (only your Orchestrator does that).
- Call docs / code-reviewer proactively — that's the Orchestrator's job. You just report what changed; Orchestrator decides next steps.

### Rules for Worker / SubWorker agents

If you are a Worker or SubWorker, you received exactly ONE concrete task. Do it and stop.

- Do NOT spawn any other agents. No exceptions. If the task needs decomposition, report back to your parent with `STATUS: blocked, FOLLOWUP: needs decomposition`.
- Do NOT read parent's code/docs beyond what's needed for your task.
- Run verification (`./build.sh check`, `cargo test --lib`, `npx tsc --noEmit`) before reporting done.
- Report structured via `codeterm agent report "..."` and exit.

### Dogfooding (applies to everyone)

This project IS CodeTerm. You're running inside CodeTerm. Every spawn via `codeterm agent spawn` is both productive work AND a test of the system. If something feels clunky — the codeterm CLI, the mesh, the reports, the panes — note it in your report. We improve CodeTerm by using it.

## Documentation

- `docs/API.md` — REST API, Socket API, CLI reference
- `docs/CONFIG.md` — Configuration options, themes, keybindings
- `docs/SKILLS.md` — Claude Code skill for CodeTerm control
- `docs/BUILD.md` — Build instructions
- `docs/MESH.md` — Mesh networking spec (flat peer model, no crown)
- `docs/MEMORY.md` — Memory system architecture (storage, search, workers, dreaming)
- `docs/PROMPTS.md` — Prompt system: where .md prompts live, how they compile, inject, and get evaluated

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

## Detection: structural patterns, not literal strings

Detection code (prompts, task completion, agent identification, output parsing) must use structural fingerprints, not literal text. If Claude renames "Do you want to proceed?" to "Do you want to create?", detection should still work — match the structure (numbered options + footer), not the wording. Vendors change copy without warning; structure is stable. Generalize, don't enumerate.

## Code style: SOLID, DRY

**SOLID + DRY are non-negotiable.** Single responsibility per module, dependencies flow through explicit interfaces, no repeated logic. Duplicated 20-line blocks across three files is a refactor signal, not a copy-paste signal.

**Code is prose — good code reads itself, no comments needed.** This rule is non-negotiable and I (Claude) have been a chronic offender. Read this twice before writing any comment:

- **Default: no comment.** Names, types, and structure carry the meaning. Documentation lives in `docs/`, not inline.
- **Never document WHAT the code does.** If you feel that urge, the code is unclear — rename a variable, extract a helper, split a function.
- **Never reference the current task, fix, or caller.** No "Phase 3 adds X", no "used by Y", no "fixes issue Z". That belongs in commit messages and PR descriptions. Comments rot; commit history doesn't.
- **Never write multi-line comment blocks.** One line is the max for anything that isn't a full external-reference URL. If you need more than one line, you're documenting WHAT (delete it) or you're explaining bad code (fix the code).
- **Never write `///` field docs for self-describing field names.** `workspace_id: Option<String>` does not need `/// The workspace id.`
- **Never write module-level `//!` doc blocks that describe the module's purpose.** The module name does that.
- **Never write JSX label comments** like `{/* Header */}` / `{/* Agent provider + model */}`. The JSX below already names itself.
- **Never write step-numbered narration** like `// 1. Resolve workspace`. The function body is linear and the code is the comment.

Write a comment only when all of these are true:
1. The code itself CANNOT be made clearer by renaming or extracting.
2. The reader would otherwise have to guess WHY a line exists (lock ordering, a specific OS quirk, an SDK bug you're working around, a non-obvious invariant).
3. One line is enough to capture that WHY.

Everything else gets deleted. If you catch yourself writing a comment and you can't point to a clear WHY in under 15 words, the comment doesn't belong there. Pattern-copy discipline matters too: just because the file around you already has bloat doesn't give you license to add more — fix the surrounding bloat first if it's distracting you into matching the style.

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

Lock acquisition order (lowest → highest). Holding a higher-numbered lock while acquiring a lower-numbered one risks deadlock.

1. `mux` (MultiplexerState)
2. `output_buf` (OutputBuffer)
3. `screen_state` (ScreenState)
4. `agent_tracker` (PaneStatusTracker)
5. `pty` (PtyManager)

**Rules:**
- Avoid holding two locks at once when possible — lock, read/write, drop, then lock the next.
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
