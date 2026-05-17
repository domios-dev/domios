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

- `mod.rs` files target <600 lines — facade + light glue. Heavy logic belongs in concrete files inside the module.
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
./build.sh webdriver        # dev with WebDriver for UI tests
source build.sh env         # load MSVC env into shell (Windows, manual cargo)
```

**Before debugging, always verify the running build matches your code:**

```bash
curl -s http://127.0.0.1:7686/api/health | jq '.build'
```

If `build` (git commit hash) differs from `git rev-parse HEAD`, the old build is still running — restart with `./build.sh dev`. During iterative work the hash stays the same across restarts, so the real "did the restart take" signal is `./build.sh kill` (which guarantees the old process is gone) followed by a fresh `./build.sh dev`.

Dev logs: `~/.codeterm-dev/logs/codeterm.log`. Live tail: `tail -f ~/.codeterm-dev/logs/codeterm.log`.

Full testing guide (3 layers, remote E2E, fixtures): `docs/TESTING.md`.

**Cargo tests on Windows — never lose 10 min to a hang:**
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

- `evals/general-agent/` — General Agent prompt behaviour (21 cases, A/B via `prompt-evaluator` skill)
- `evals/mem-recall/` — mem search engine quality (Recall@K / MRR)
- `evals/mem-protocol/` — slim `bridge_mem.md` rule fires the right protocol calls
- `evals/session-restore/` — kill+restart preserves workspace/pane/agent_type metadata; UI-mount step manual

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

- **Orchestrator** — top of the chain. Delegates by spawning generic claude agents with explicit role prompts (see below).
- **Lead** — has a bounded sub-task, may decompose and spawn Workers.
- **Worker / SubWorker** — does ONE concrete task, runs `codeterm agent report`, exits. **NEVER spawns other agents.** If decomposition needed → report blocked.

### We dogfood our own `codeterm mem` instead of `.claude/agents/`

CodeTerm is the project that builds the mem system — so we **use mem as the single source of truth for role definitions and procedural skills**, instead of `.claude/agents/*.md`. The `.claude/agents/` folder has been emptied on purpose. Reasons:

- One canonical place per role. No drift between an `.md` file and a mem entry.
- Mem can be fetched **fragmentally** via multiple `mem search` calls — agents pull only what they need, when they need it, instead of pre-loading a giant system prompt.
- Updates propagate across all CodeTerm instances on this machine automatically (mem is global).
- Any peer or sub-agent on any mesh peer can pull the same role guidance.

Role skills currently in mem (search by role name or fetch by id):

| Role | Mem id | Use for |
|------|--------|---------|
| Backend dev | `skill_backend_dev_role_rust_src_tauri_for_codeterm` | Rust changes in `src-tauri/` |
| Frontend dev | `skill_frontend_dev_role_react_typescript_src_for_codeterm` | React/TS changes in `src/` |
| Docs sync | `skill_docs_sync_role_keep_docs_in_sync_with_code_for_codeterm` | Keep docs in sync after backend/frontend work |
| Code reviewer | `skill_code_reviewer_role_read_only_review_for_codeterm` | Read-only diff review, severity ranking |
| Prompt evaluator | `skill_prompt_evaluator_role_a_b_and_regression_testing_for_codeterm` | A/B regression test prompt changes via `.claude/skills/prompt-evaluator/runner.py` |
| End-to-end release | `skill_end_to_end_github_release_for_codeterm` | Full release pipeline (bump→tag→build→sign→notarize→publish→mirror) |

**How to spawn:** instead of `Agent(subagent_type="backend-dev")`, use `Agent(subagent_type="general-purpose", prompt="codeterm mem get skill_backend_dev_role_rust_src_tauri_for_codeterm — that is your role definition. Read it then: <concrete task>")`. Same for `codeterm agent spawn claude --task "first run codeterm mem get <skill_id>, then: <task>"`.

**`.claude/skills/`** still exists for skills with runnable code (e.g. `prompt-evaluator/runner.py`). Mem doesn't store executable scripts — only markdown content. Skills with code stay on disk; their `SKILL.md` is the entry point the role-skill in mem points to.

### Where the codeterm-specific skill folders live

CodeTerm-project skills (release pipeline, prod build, Windows self-install) live in **`<repo>/.codeterm/skills/`** — version-controlled, comes with the checkout, evolves with the codebase via PRs. Currently:

- `.codeterm/skills/github-release/` — release pipeline procedure
- `.codeterm/skills/prod-build/` — local prod build
- `.codeterm/skills/self-install-local/` — Windows local NSIS install

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
