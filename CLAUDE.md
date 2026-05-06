# CodeTerm

**OS for AI agents.** Cross-platform terminal multiplexer where AI agents live, work, and coordinate. Tauri v2 (Rust) + React/TypeScript + xterm.js. Full vision: `VISION.md`.

## Hard rules

- **English only** in code, comments, commit messages.
- **No external user base yet** — this is Andrey's personal project. No backward-compat shims, no migration paths. Breaking changes are fine. Delete legacy aggressively. When in doubt, rewrite rather than patch.
- **Never commit/push without explicit user request.** No auto-commits, no "let me commit this". Wait for "коммить" / "commit" / "запушь".
- **Never kill prod CodeTerm.** Prod = port 7685, dev = port 7686. See *Process safety* below.
- **Hot-reload over restart.** Subsystems should support stop/start without restarting the whole daemon when possible.

## Source file size discipline

**Hard limit: 800 lines per source file** (`.rs`, `.ts`, `.tsx`, `.py`, `.swift`). When a file approaches this, split by responsibility. If a single concern legitimately needs more than 800 lines, your concern is two concerns wearing a trenchcoat.

- `mod.rs` files target <300 lines — facade only (re-exports, trait declarations). Logic belongs in concrete files inside the module.
- Excluded: `target/`, `node_modules/`, `dist/`, `gen/`, `references/`, `vendor/`, `src-tauri/pty-fork/`, data formats.
- Enforcement: `scripts/check-file-sizes.sh` (CI-wired). `scripts/file-size-allowlist.txt` lists pre-existing oversized files (baseline-locked, must shrink, never grow).

When splitting a file with inline `#[cfg(test)] mod tests`, keep `mod tests;` in source and move the body to `<name>/tests.rs`.

## Build, run, test

All commands go through `build.sh`. Safe on Windows: never kills production CodeTerm.

```bash
./build.sh dev              # incremental dev (port 7686, hot reload, auto-builds CLI)
./build.sh check            # cargo check (~2s incremental)
./build.sh test             # Rust unit tests
./build.sh build            # production build
./build.sh cli              # rebuild CLI only → ~/.codeterm/bin/ (no dev restart)
./build.sh kill             # safe kill of dev only
./build.sh e2e              # E2E tests against running dev
./build.sh webdriver        # dev with WebDriver for UI tests
source build.sh env         # load MSVC env into shell (Windows, manual cargo)
```

**Before debugging, always verify the running build matches your code:**

```bash
curl -s http://127.0.0.1:7686/api/health | jq '{build, build_number}'
```

If `build` (git commit hash) differs from `git rev-parse HEAD`, the old build is still running — restart with `./build.sh dev`. Don't waste time investigating stale binaries.

Dev logs: `~/.codeterm-dev/logs/codeterm.log`. Live tail: `tail -f ~/.codeterm-dev/logs/codeterm.log`.

Full testing guide (3 layers, remote E2E, fixtures): `docs/TESTING.md`.

## Lock ordering (deadlock prevention)

Acquire in order, drop before awaiting. parking_lot is **not reentrant** — `mux.lock()` inside `mux.lock()` deadlocks.

1. `mux` (MultiplexerState)
2. `output_buf` (OutputBuffer)
3. `screen_state` (ScreenState)
4. `agent_tracker` (PaneStatusTracker)
5. `pty` (PtyManager)

After parallel Rust refactors, grep for nested `.lock()` to verify ordering.

## Process safety

**NEVER kill prod CodeTerm.** Identify by port:

```bash
netstat -ano | grep :7686 | grep LISTENING | awk '{print $5}'   # dev — safe to kill
netstat -ano | grep :7685 | grep LISTENING | awk '{print $5}'   # PROD — NEVER kill
```

Prefer `./build.sh kill` (only touches dev). When unsure, ask the user.

## Cross-platform

Verify on macOS, Windows, Linux. Never add platform-specific code without handling all targets.

- PTY: `portable-pty` (ConPTY on Windows, native on Unix).
- Paths: forward slashes in code, OS-native in user-facing output.
- Keys: `e.code` (not `e.key`) for keyboard layout independence; config `Ctrl` maps to `Cmd` on macOS.
- Tauri plugins (dialog, notification, shell): cross-platform by design — prefer over custom.
- OSC 7 CWD: injected into PowerShell on Windows; bash/zsh emit it natively.

## Agent roles (top of mind)

CodeTerm has a 4-level hierarchy: **Orchestrator → Lead → Worker → SubWorker**. Your role comes from `$CODETERM_ROLE` or the spawn task prompt. **No role = Orchestrator.**

- **Orchestrator** — top of the chain. Delegates to specialized workers (`.claude/agents/*.md`).
- **Lead** — has a bounded sub-task, may decompose and spawn Workers.
- **Worker / SubWorker** — does ONE concrete task, runs `codeterm agent report`, exits. **NEVER spawns other agents.** If decomposition needed → report blocked.

Specialized types: `backend-dev`, `frontend-dev`, `docs`, `code-reviewer`, `devops`, `prompt-evaluator`. Each is defined in `.claude/agents/<type>.md`.

**Full delegation manual** — when to delegate vs DIY, how to spawn (`Agent` tool vs `codeterm agent spawn`), role assignment, recursion limits, proactive `docs` / `prompt-evaluator` / `code-reviewer` hooks: see **`docs/contributing/agents.md`**.

## Code style

SOLID + DRY non-negotiable. **Default: NO comments** — names + types + structure carry meaning. Only comment WHY (non-obvious invariant, OS quirk, deliberate workaround). One line max. Detailed Rust + TypeScript guidance and architecture guardrails: **`docs/contributing/style.md`**.

## Architecture (agent providers, detection)

Agent integration is abstracted via providers (`src-tauri/src/agent/`, `src/components/ChatView/parser.ts`). Currently: Claude. Architecture-ready: Codex, Aider, Goose. Detection uses **structural fingerprints**, never literal strings — vendors change copy without notice. Full breakdown: **`docs/contributing/architecture.md`**.

## Documentation

Bump the `<!-- revision: N -->` tag at the bottom of any doc you change.

- `docs/API.md` — REST / Socket / CLI reference
- `docs/CONFIG.md` — config options, themes, keybindings
- `docs/SKILLS.md` — Claude Code skill for CodeTerm control
- `docs/BUILD.md` — build instructions
- `docs/MESH.md` — mesh networking (flat peer model) + bridge migration
- `docs/MEMORY.md` — memory subsystem (storage, search, workers, dreaming)
- `docs/PROMPTS.md` — prompt templates: where they live, how they compile
- `docs/TESTING.md` — full test guide
- `docs/contributing/` — agents, style, architecture, per-feature testing recipes
