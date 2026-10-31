<!-- Shared repository instructions; CLAUDE.md imports this file. -->

# Domios

**Edit this repository's AGENTS.md only when a human explicitly requests it. Keep changes short and necessary; this restriction does not apply to other projects.**

**OS for AI agents.** Cross-platform terminal multiplexer where AI agents live, work, and coordinate. Tauri v2 (Rust) + React/TypeScript + xterm.js. Full vision: `essences/vision.md`.

## How Domios holds together

- Read `essences/begin_here.md`, then `vision.md`, `capabilities.md`, and `core.md`; preserve their observable contracts without forcing code to mirror the map.
- Verify observable behavior with focused checks per completed batch and one broad release gate.
- Orchestrators coordinate; Planners shape tasks; Workers share canonical with disjoint declared edit paths, using lanes for overlap or build-breaking work. Integrate lanes through real merges and canonical verification.
- Keep owner rules in `codeterm mem`, decisions/evidence in plan strides, stable contracts in `essences/`, procedures in `.agents/skills/`, and embedded help in `prod-docs/`.

## Hard rules

- Embedded agent prompts teach Domios usage and general working rules. Keep this repository's development commands, source paths and machine setup in project instructions or skills.
- **English only** in code, comments, commit messages.
- **No external user base yet** — this is Andrey's personal project. No backward-compat shims, no migration paths. Breaking changes are fine. Delete legacy aggressively. When in doubt, rewrite rather than patch.
- **Never kill prod Domios.** Prod = port 7685, dev = port 7686. See *Process safety* below.
- **Hot-reload over restart.** Subsystems should support stop/start without restarting the whole daemon when possible.
- **Internal crates are version `0.0.0`.** Only the shipped app and CLI crates carry the product version (set by `npm run sync:versions`); never read an internal crate's own version at runtime.
- **Windows reserved names are forbidden.** Never create a file or directory named `NUL` (or another Windows device name). Use ordinary temporary filenames instead; remove any accidental reserved-name artifact immediately with an extended `\\?\` path.

## Source file size discipline

- Source files must not exceed 1200 lines; `mod.rs` targets below 800. No allowlist; split by responsibility.
- Move inline test bodies to `<name>/tests.rs`, retaining `mod tests;` in source.
- Run `bash scripts/check-file-sizes.sh --fail`; its exclusions cover generated/vendor/build artifacts.

## Build, run, test

- Read the owning skill (`build`, `dev-run`, `testing`, `gates`, `github-release`) and use named `build.sh` operations; `build.ps1` only forwards. Keep recipes and recovery procedures in those skills.
- Workers run lightweight tests and scoped QA autonomously. Compilation and heavy builds stay centralized with one build owner; no parallel Worker Cargo/DEV builds or alternate targets to evade the gateway.
- Merge and finish edits before formatting/generating once. Never change sources while compiling. Batch selectors into one compile per crate/profile; fix reported failures together and recheck only failed or newly affected proof.
- Commit, then hand the exact SHA to live QA immediately; its candidate build is the compile check. Long regressions run on CI in parallel, with one broad release gate on the release SHA. Do not block live QA behind repeated local suites.
- Run owning focused checks, changed TS/JS tests, formatting and size/boundary guards; run `pnpm check:api-effects` when effects change. Require zero compiler errors/warnings; any unavoidable lint exception must be narrow and justified. Report unrun checks honestly.
- Use Team utility tabs with timeouts for builds/checks; DEV servers have no timeout. Follow `utility-shell-reports`. A busy gateway refuses: continue source work, never poll/retry or evade it. `--wait-build` requires intentional blocking.
- Never hand-edit generated files. Regenerate CLI contracts only when definitions change; use the owning skill for generators and self-install.

## Build speed (worktrees, tests, profiles)

- Keep machine-level sccache and tuned Cargo profiles. Never share final target directories across worktrees or hand-compose cache/gateway settings.
- One DEV build target per checkout: never create a new DEV instance name just to get a fresh target. Several DEVs may run from the one build; never rebuild it while a DEV from it is running — stop that DEV first.
- `lane-check` alone selects shared lane intermediates; canonical gateway jobs own the stable `X:` mapping. Use `build.sh gc` for safe retention cleanup.
- Warm-build/QA machine selection lives in `codeterm mem`; record it before dispatch. Cache limits and timeout controls belong in the build/gates skills.
- After any Rust dependency change, run `cargo hakari generate` in `src-tauri`. A new app-only (GUI) dependency goes into the hakari final-excludes; prove the `domios-server` and `codeterm-cli` closures gain no crate and `check-crate-layering.sh` passes. Procedure: build skill, *Workspace-hack*.

## Lane integration preserves ancestry

- Never cherry-pick, squash, replay, or patch-copy lane work. Real-merge it; delete only after `git merge-base --is-ancestor <lane-tip> <target-tip>` succeeds.
- Canonical has one integrator; shared Workers stage and commit only owned paths, and format with `build.sh fmt --paths <files>`. Use lanes for overlap or build-breaking work.
- Before debugging, compare `/api/health` build with the intended source SHA. Use the dev-run skill to stop/restart stale DEV; never prod. Keep one foreground dev/build owner.

## Testing

- Test product behavior, not test infrastructure. Keep `scripts/` for helpers and runners; do not add self-test suites or dedicated self-test fixtures for catalogues, discovery tools, QA helpers or runners.
- Extract decisions into pure models; test plain inputs/outputs with injected clocks and ports. Runtime/UI fixtures prove wiring only.
- UI checks cover accessible controls, handlers, and behavior, not CSS classes, copy, layout internals, or component implementation.
- Test prompt delivery/substitution mechanisms with opaque fragments, never baked-in wording or shared phrases.
- No sleeps, live agents, or real network in unit tests. Playwright is opt-in when it is the right oracle; report flakiness honestly.

## Bench safety: NEVER call raw `codeterm` against dev — use the lib

Benches must source `evals/_lib/lib.sh` and use `dc`, `dev_get`, `dev_post`, or `dev_run`; inherited connection variables can target prod. Never call raw `codeterm` from `evals/`. Keep explicitly authorized prod actions outside bench machinery and identify them clearly.

## Behaviour benches — `evals/`

Use the matching bench procedure for prompt, restore, recall, or routing changes. Keep test inputs isolated from owner sessions.

## Lock ordering (deadlock prevention)

Acquire `mux -> output_buf -> screen_state -> agent_tracker -> pty`. Locks are non-reentrant; inspect nested locks after parallel refactors. Drop every guard before `.await`; never suppress `clippy::await_holding_lock`. DEV `[LOCK-HOLD]` diagnostics flag holds above 50ms.

## Async runtime & hot-path discipline (perf — learned the hard way)

- Never block frontend input, scroll, stream, or render paths with large synchronous serialization, parsing, deep copies, or writes. Update incrementally; use asynchronous per-record storage for large/growing data, localStorage only for small bounded values.
- Keep blocking I/O, heavy computation, and subprocesses off the async runtime; use `spawn_blocking` or an appropriate worker. The daemon calls its own functions, never its CLI.
- Hot-path commands must be cheap and bounded. Rebuild expensive cached projections only when their data changes.
- Measure under realistic tab/stream load. `[SLOW-IPC]` includes waiting and frontend scheduling; correlate it with `[LONGTASK]` and server evidence before assigning cause.
- Use `DeferredLoader` for delayed loader visibility; elapsed time never proves readiness.

## No sleeps or timer-driven coordination

Use events, channels, process handles, DOM state, or protocol acknowledgements, never sleeps, polling, fixed warmups, or settle delays. Timeouts bound failure only. The sole retry-timing exception is bounded PTY submit-key recovery; it never proves readiness or delivery.

## PTY input delivery is CORE and FRAGILE — touch with extreme care

Read `.agents/skills/pty-delivery/SKILL.md` before any PTY/Chat send, ledger, recovery, or evidence change.

- Write each payload at most once. Written/uncertain sends never replay; post-write uncertainty is `submit_unconfirmed`, not a false pre-write failure.
- Delivery requires exact own-session append-window evidence; no screen scraping, guessed readiness, provider-ID branches, or payload retries.
- Hold only before initial readiness, then flush in order; busy agents accept new input. Recovery sends bounded submit keys only; exact evidence stops them and a new send cancels the previous recovery cycle.
- Clear the composer immediately; retain one bubble through status/echo/retry. Retry only proven non-delivery; preserve dismissal and identity across reload.
- Name the causal finding, touched symbols, rollback boundary, and focused invariant proof.

## Agent launch delivery

Deliver bootstrap input to the exact lifecycle-owned PTY. Never route pre-ready input through post-ready `send_text`, which can wait on that same lifecycle.

## Provider hook files are a danger zone

Provider hook files are machine-global and content-trusted; changes can require owner re-trust.

- Keep content instance-agnostic and byte-identical across prod/dev. Route via required `CODETERM_API_URL`, `CODETERM_API_TOKEN`, and `CODETERM_TAB_ID`; no baked-in endpoint or fallback.
- Modify only owned hooks. Ownership uses the command's `X-CodeTerm-Hook` marker, never a dropped sibling metadata field or bare route match.
- Refresh stale owned hooks at startup/provider reload only with recorded consent. Never rewrite for cosmetic cleanup; use `/dev/null`, never `NUL`.

## Process safety

Never kill prod (7685); DEV is 7686. Use `build.sh kill`, identify exact instance/process before cleanup, and ask if identity is uncertain.
Noncanonical dev requires explicit `CODETERM_DEV_INSTANCE` and `CODETERM_DEV_API_PORT`; never inherit parent prod selectors. Follow the dev-run skill.

## Cross-platform

Handle Windows, macOS, and Linux; report unverified platforms. Use portable PTY/Tauri facilities, forward slashes in code and native user-facing paths. Keyboard shortcuts use `e.code`; Ctrl maps to Cmd on macOS.

## Orchestration failure visibility

Keep agents/utilities/crons in their Team. A failed dispatch, missing ACK/session/brief, vanished Worker, or lost assignment is a product incident: immediately tell the owner its impact, affected tasks, cleanup, and fallback. Never silently retry or claim success.

## Debug instrumentation

Use `log::debug!`/`trace!` for retained diagnostics; release strips them. Frontend diagnostics require `DEV_BUILD` from `src/lib/devBuild.ts`, not `import.meta.env.DEV`. No temporary info-level logs or bare console output in shipped code.

## Code style

SOLID and DRY. Prefer clear names over comments; WHY comments only, at most two lines (license/public API docs excepted). Read `.agents/skills/style/SKILL.md`.

## Architecture (agent providers, detection)

Provider contracts live in `essences/core.md`; load `provider-authoring` before changing them.

- Core branches on capabilities, never provider IDs or display strings. Detection uses structural fingerprints from the tab's own claimed session; unknown stays unknown, never a guess from global/latest history.
- New or changed provider contracts require native/JS parity on shared fixtures. Audit every new plugin host API against its filesystem, SQLite and environment authority and QuickJS limitations.
- Official plugins version independently: manifest, package and channel metadata must agree; `minCodeterm` states the required host capability. Use `scripts/release-plugin.mjs` and the plugin release skill/procedure rather than changing generated bundles by hand. Startup must not downgrade a newer installed channel release.
- System plugins live in this repository's `plugins/` channel (`codeterm-core`). Other official capability plugins live in `~/Developer/codeterm-plugins` (`rollacode/codeterm-plugins`). Local installation does not authorize replacing reserved system IDs; use their official channel.

## Code navigation

Prefer cached SCIP indexes for definitions, references, and implementation graphs; use `rg` for exact text.

## GitHub and release

- Issue and commit titles use `<scope>: description`, never bracket tags.
- Load `.agents/skills/github-release/SKILL.md` for release and issue-linking procedures. Never direct-push `master`, move published tags, or release an unverified head.
- Link fixed issues in both commit and PR; use `Refs` for context only. Integration PRs use `Repository-Issue`, final release PRs use generated closing keywords, and public issues close only after verified publication; details are in the release skill.
- For CLI contract changes, load `.agents/skills/cli-contributor/SKILL.md` and update affected docs, prompts, skills, examples and contract tests.

## Documentation

Read `essences/begin_here.md` before editing docs. Keep one authoritative home:

- `essences/`: stable product contracts, not implementation recipes.
- `prod-docs/`: embedded help for agents using the installed product, not repository development instructions. Bump its revision tag when edited.
- `.agents/skills/`: development procedures and recoveries.
- `generated/cli/`: CLI contracts generated from definitions; never hand-edit.

Do not hardcode changing totals of files, tests, cases or runners in documentation; use discovery tools. Keep decisions/evidence in plan strides and owner rules in memory.

<!-- revision: 47 -->
