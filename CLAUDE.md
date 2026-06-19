# CodeTerm

**OS for AI agents.** Cross-platform terminal multiplexer where AI agents live, work, and coordinate. Tauri v2 (Rust) + React/TypeScript + xterm.js. Full vision: `VISION.md`.

## Hard rules

- **English only** in code, comments, commit messages.
- **No external user base yet** — this is Andrey's personal project. No backward-compat shims, no migration paths. Breaking changes are fine. Delete legacy aggressively. When in doubt, rewrite rather than patch.
- **Never commit/push without explicit user request.** No auto-commits, no "let me commit this". Wait for "коммить" / "commit" / "запушь".
- **Never kill prod CodeTerm.** Prod = port 7685, dev = port 7686. See *Process safety* below.
- **Hot-reload over restart.** Subsystems should support stop/start without restarting the whole daemon when possible.

## Source file size discipline

**Hard limit: 1000 lines per source file** (`.rs`, `.ts`, `.tsx`, `.py`, `.swift`). When a file approaches this, split by responsibility. If a single concern legitimately needs more than 1000 lines, your concern is two concerns wearing a trenchcoat.

- `mod.rs` files target <800 lines — facade + light glue. Heavy logic belongs in concrete files inside the module.
- Excluded: `target/`, `node_modules/`, `dist/`, `gen/`, `references/`, `vendor/`, `src-tauri/pty-fork/`, data formats.
- Enforcement: `scripts/check-file-sizes.sh` (CI-wired). `scripts/file-size-allowlist.txt` lists pre-existing oversized files (baseline-locked, must shrink, never grow).

When splitting a file with inline `#[cfg(test)] mod tests`, keep `mod tests;` in source and move the body to `<name>/tests.rs`.

## Build, run, test

All commands go through `build.sh`. Safe on Windows: never kills production CodeTerm.

```bash
./build.sh dev              # incremental dev (port 7686, hot reload, auto-builds CLI)
./build.sh dev --wait       # same, but backgrounds the process and blocks until /api/health
                            # responds — use in scripts/benches/CI
./build.sh check            # cargo check (~2s incremental)
./build.sh test             # Rust unit tests
./build.sh build            # production build
./build.sh cli              # rebuild CLI only → ~/.codeterm/bin/ (no dev restart)
./build.sh kill             # safe kill of dev only
./build.sh e2e              # E2E tests against running dev
source build.sh env         # load MSVC env into shell (Windows, manual cargo)
```

**Before debugging, always verify the running build matches your code:**

```bash
curl -s http://127.0.0.1:7686/api/health | jq '.build'
```

If `build` (git commit hash) differs from `git rev-parse HEAD`, the old build is still running — restart with `./build.sh dev`. During iterative work the hash stays the same across restarts, so the real "did the restart take" signal is `./build.sh kill` (which guarantees the old process is gone) followed by a fresh `./build.sh dev`.

Dev logs: `~/.codeterm-dev/logs/codeterm.log`. Live tail: `tail -f ~/.codeterm-dev/logs/codeterm.log`.

Full testing guide (3 layers, remote E2E, fixtures): `docs/TESTING.md`.
UI automation/debugging uses Playwright over WebView2 CDP only; operational docs live in `tests/playwright/README.md`.

**Cargo tests on Windows — never lose 10 min to a hang:**
- From PowerShell, run repo commands through `.\build.ps1 ...`; it selects Git
  Bash directly. Do not call plain `bash` from PowerShell because that can hit
  `C:\Windows\System32\bash.exe` / WSL and fail on machines without a WSL distro.
  If you need Bash explicitly, call `C:\Program Files\Git\bin\bash.exe`.
- Run via **PowerShell foreground**, NOT Bash. Bash auto-backgrounds long commands; shell wrapper exits, cargo runs orphan and holds `target/debug/.cargo-lock`, every next `cargo test` then waits forever on that lock. PowerShell tool keeps cargo in the foreground so the timeout actually fires.
- Always narrow first (`cargo test plugin::js_runtime::hot_reload`, not the whole `--lib`) and cap timeout at **90s**. If it hangs past 60s, that's almost always a deadlock in **your own code** (parking_lot is non-reentrant — never call `.lock()` on a Mutex while a previous guard on it is still alive on this thread), not a "linker is slow" infra problem. Incremental cargo on a warm cache compiles and links in seconds.
- Stuck cargo? `Get-Process cargo,rustc,link | Stop-Process -Force` + delete `target/debug/.cargo-lock` + re-run. Always.

## Bench safety: NEVER call raw `codeterm` against dev — use the lib

When iterating on behaviour benches under `evals/<name>/`, the
orchestrator usually runs in a Claude pane **inside prod codeterm**.
That parent pane exports `CODETERM_API_PORT=7685` and a prod socket
path. A bare `codeterm <subcommand>` in any bench shell inherits
that env and silently dispatches to **prod**, not dev. If the dev
daemon also happens to be down at that moment, the CLI's
socket-discovery falls back to the prod pipe — same outcome.

**Always** source the bench lib first and use its `dc` wrapper:

```bash
source "$(git rev-parse --show-toplevel)/evals/_lib/lib.sh"
dc workspace list           # locked to dev — fails with exit 7 if dev is down
```

Sourcing the lib auto-calls `dev_env` (port 7686 + dev token + dev
socket pipe). The `dc` function asserts dev is alive on every call
before invoking the CLI, and re-exports the dev env-vars into the
CLI's process so a leaked parent var can't override it. HTTP
helpers in the lib (`dev_get`, `dev_post`) are hardcoded to
`http://127.0.0.1:7686` for the same reason.

Hard rule: **no bench script under `evals/` may invoke `codeterm`
directly.** Use `dc`. If you really need a prod-targeted CLI call,
do it explicitly outside the bench machinery, after a clear
"this is prod" log line — never inside an automated drill.

## Behaviour benches — `evals/`

Behavioural changes (system prompts, session restore semantics, mem
recall quality, agent routing) are verified with bench scripts under
`evals/<name>/`. Existing sets:

- `evals/cli-agent/` — real-daemon LLM benchmark: drives Claude/Codex/Opencode through every CLI command (model-judged, replaces old headless general-agent/mem-protocol evals)
- `evals/mem-recall/` — mem search engine quality (Recall@K / MRR / latency)
- `evals/dreaming/` — REM-phase prompt (Stage A.1 hub decomposition, Stage A.2 island bridging)
- `evals/session-restore/` — kill+restart preserves workspace/pane/agent_type metadata; UI-mount step manual
- `evals/restore-matrix/` — one-workspace provider restore matrix: N panes per provider (claude/codex/opencode), pre-kill marker materialization gate in the provider store, hard restart, API sid identity + Playwright UI-visible marker oracle (`RM_PROVIDERS`/`RM_PER_PROVIDER`/`RM_MATERIALIZE_TIMEOUT` tunable)

Also present, experimental/historical: `evals/dreamer-repair/` (mem graph repair smoke test), `evals/seeds/`, `evals/v1_1_23_e2e/`.

**Before changing behaviour, find the matching eval set and run it
first to capture the baseline. Extend the eval set if a new scenario
isn't covered. Don't ship behavioural changes without a bench unless
the change is a hardcoded constant tweak.** Each `evals/<name>/`
folder has a README explaining how to run.

## Lock ordering (deadlock prevention)

Acquire in order, drop before awaiting. parking_lot is **not reentrant** — `mux.lock()` inside `mux.lock()` deadlocks.

1. `mux` (MultiplexerState)
2. `output_buf` (OutputBuffer)
3. `screen_state` (ScreenState)
4. `agent_tracker` (PaneStatusTracker)
5. `pty` (PtyManager)

After parallel Rust refactors, grep for nested `.lock()` to verify ordering.

## Async runtime & hot-path discipline (perf — learned the hard way)

Blocking/heavy work on the Tauri async runtime starves ALL IPC and freezes the WHOLE UI (every pane + sidebar), not just the caller. A 3-4s full-UI freeze on every tab switch was `plugin_manager_snapshot` forking CLI subprocesses (including a network `channel diff`) on the async runtime.

- **The daemon NEVER shells out to its own `codeterm` CLI.** Call the in-process function. A daemon forking its own CLI that reconnects to it is an architecture smell — and slow (process start + IPC + network per call).
- **Blocking / IO / heavy / subprocess work in a Tauri command → `tokio::task::spawn_blocking`.** Synchronous `dispatch::dispatch`, disk reads, network, `Command::new` all qualify. A plain sync `fn` command runs on the runtime too — same rule applies.
- **Hot-path commands (fire on tab switch / mount / render) must be cheap and bounded.** Cache expensive snapshots; rebuild on change (file-watcher / explicit action), never on every call.
- **Measure before optimizing.** The dev build logs `[SLOW-IPC] <cmd> <ms>` + `[LONGTASK]` (frontend `invoke` wrapper → dev log). A trivial command (e.g. `get_config`) blocking seconds means the runtime is starved; the slow command that resolves FIRST is the cause, the rest are queued victims — don't chase the victims. Test under realistic load (many panes), not a clean 2-tab dev instance.

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

CodeTerm has a 2-level hierarchy: **Orchestrator → Worker**. Your role comes from `$CODETERM_ROLE` or the spawn task prompt. **No role = Orchestrator.**

- **Orchestrator** — top of the chain, no parent. Delegates by spawning generic claude agents with explicit role prompts (see below).
- **Worker** — does ONE concrete task, runs `codeterm agent report`, exits. **NEVER spawns other agents.** If decomposition is needed → report blocked, and the Orchestrator splits it further or spins up a separate Orchestrator in its own workspace.

### Agent roles: `docs/contributing/agents.md`, assigned inline at spawn

`.claude/agents/*.md` is NOT used — that folder is empty on purpose. The role
hierarchy (Orchestrator → Worker) and each role's remit (backend-dev,
frontend-dev, code-reviewer, docs-sync, release) live in
**`docs/contributing/agents.md`**. When you spawn a worker, give it its role
**inline in the task prompt** (e.g. "You are a Rust backend engineer working in
`src-tauri/`; your task: …") — there are no per-role `codeterm mem` entries to
fetch (do not try `codeterm mem get skill_<role>_role` — those do not exist).

**`.claude/skills/`** holds skills with runnable code (e.g.
`prompt-evaluator/runner.py`) — their `SKILL.md` is the on-disk entry point.

### Where the codeterm-specific skill folders live

CodeTerm-project skills (release pipeline) live in **`<repo>/.codeterm/skills/`** — version-controlled, comes with the checkout, evolves with the codebase via PRs. Currently:

- `.codeterm/skills/github-release/` — release pipeline procedure

Prod build lives in `./build.sh build`; Windows local NSIS self-install in `scripts/self-install-local.ps1`.

On a fresh clone, register them with mem so the Librarian indexes them:

```bash
for s in .codeterm/skills/*/; do codeterm mem skill add "$(pwd)/$s"; done
```

This is one-time per machine. After registration, the Librarian re-indexes whenever a `SKILL.md` changes — edits flow through mem search automatically.

User-scoped or cross-project skills (not codeterm-specific) live in `~/.codeterm/skills/` (canonical watcher path that auto-mirrors to `~/.claude/skills/` and `~/.agents/skills/`).

**Full delegation manual** — when to delegate vs DIY, how to spawn, role assignment, recursion limits, proactive doc/eval/review hooks: see **`docs/contributing/agents.md`**.

## Code style

SOLID + DRY non-negotiable. **Default: NO comments** — names + types + structure carry meaning. Only comment WHY (non-obvious invariant, OS quirk, deliberate workaround). One line max. Detailed Rust + TypeScript guidance and architecture guardrails: **`docs/contributing/style.md`**.

## Architecture (agent providers, detection)

Agent integration is abstracted via providers (`src-tauri/src/agent/`, `src/components/ChatView/parser.ts`). Currently: Claude. Architecture-ready: Codex, Aider, Goose. Detection uses **structural fingerprints**, never literal strings — vendors change copy without notice. Full breakdown: **`docs/contributing/architecture.md`**.

### Plugin & capability discipline (enforce on every provider change)

The provider/plugin boundary is the project's load-bearing seam. It stays clean only if every change crossing it pays the same tax. Non-negotiable when you add or extend a provider capability (a new `AgentProvider` trait method, a new host-API function, a new detection source):

1. **Core asks capabilities, never identities.** Branch on `supports_chat_ui()` / `available_models()` / a new capability fn — never on a hardcoded provider-id list or display string. If you're matching `"codex"` / `"opencode"` in core code, you've leaked the boundary.
2. **Native + JS parity in lockstep.** Any trait method a JS drop-in can implement MUST gain a case in `parity_test.rs` proving the native and JS paths agree on shared fixtures. A capability without a parity test will silently drift on drop-ins.
3. **Detection keys off the pane's OWN session.** Provider detection (model, session, status) reads only the record belonging to this pane's claimed session id — never "latest global" / "newest for cwd". Unknown → return null (honest) over a guess (contaminating). This is a real bug class we've shipped.
4. **Audit the host-API surface when it grows.** Every new host function a drop-in can call is a new confinement hole — justify it against the FS/sqlite/env sandbox, and remember QuickJS lacks bindings the native runtime has (a missing `console` once aborted parsing silently).
5. **Keep core provider plugin metadata in lockstep.** Until provider plugins split into their own release stream, their versions mirror the CodeTerm app version. After bumping `package.json`, run `npm run sync:versions` and verify with `npm run check:versions`; this syncs provider manifests/packages, internal `packages/*` package versions, and `plugins/channel.json`. Official `codeterm-core` channel installs are trusted system plugins, startup repairs same-version embedded hash drift, and newer channel versions must not be downgraded.

Full rationale + the boundary map: **`docs/contributing/architecture.md`**.

## Mem self-repair primitives (Dreamer interface)

Graph cleanup goes through `src-tauri/src/mem/graph_repair.rs` — whitelist-bounded typed primitives, never raw SQL. The Dreamer chains these via the prompt at `src-tauri/prompts/dreaming_rem.md` Stage A.4.

CLI: `codeterm mem graph {find-duplicate-relations,delete-relations,merge-relations,find-duplicate-entities,augment-entity,delete-entity,alias,repair-orphan-vectors,repair-orphan-blobs}`. REST: `POST /api/mem/graph/repair/*`. Every destructive op writes a full payload to `graph_purge_log` for reversibility. See `docs/MEMORY.md` for the full table.

When extending the surface: add a typed enum variant (mirror `RelationColumn` / `EntitySimilarityStrategy`) instead of free-form strings; never let LLM-controlled input reach `format!`-built SQL.

## GitHub Issues

Issue titles use `<scope>: description` — same scopes as commit prefixes:

```
fix:      bug fix
feat:     new feature
mem:      memory subsystem
mesh:     mesh networking
research: spikes, investigations, benchmarks
docs:     documentation only
build:    build / CI
```

No `[Tag]`, `[Idea]`, `[Bug]` brackets — just the scope prefix.

## Documentation

Bump the `<!-- revision: N -->` tag at the bottom of any doc you change.

- `docs/API.md` — REST / Socket / CLI reference
- `docs/CONFIG.md` — config options, themes, keybindings
- `docs/SKILLS.md` — Claude Code skill for CodeTerm control
- `docs/BUILD.md` — build instructions
- `docs/MESH.md` — mesh networking (flat peer model) + bridge migration + Mesh tab (service registry, health probes, activity log, push tokens)
- `docs/MEMORY.md` — memory subsystem (storage, search, workers, dreaming)
- `docs/PROMPTS.md` — prompt templates: where they live, how they compile
- `docs/TESTING.md` — full test guide
- `docs/contributing/` — agents, style, architecture, per-feature testing recipes
