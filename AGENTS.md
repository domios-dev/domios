<!-- Shared repository instructions; CLAUDE.md imports this file. -->

# Domios

**Edit this repository's AGENTS.md only when a human explicitly requests it. Keep changes short and necessary; this restriction does not apply to other projects.**

**OS for AI agents.** Cross-platform terminal multiplexer where AI agents live, work, and coordinate. Tauri v2 (Rust) + React/TypeScript + xterm.js. Full vision: `essences/vision.md`.

## How Domios holds together

- Read `essences/begin_here.md`, then `vision.md`, `capabilities.md`, and `core.md`; preserve their observable contracts without forcing code to mirror the map.
- Verify observable behavior with focused checks per completed batch and one broad release gate.
- Orchestrators coordinate; Planners shape tasks; Workers implement in isolated lanes. Integrate through real merges and canonical verification.
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

- Finish a coherent batch of edits, then run applicable checks once. Repeat only checks affected by subsequent changes or failures; use a narrow reproducer during implementation when needed.
- Read the owning skill first: `dev-run`, `build`, `gates`, `testing`, or `github-release` under `.agents/skills/`.
- Use named `build.sh` operations; PowerShell uses `.\build.ps1`. All build logic belongs in `build.sh`; PowerShell only forwards. Update the owning skill with reusable recipes and failure recovery.
- Run builds/tests in Team utility tabs with `--timeout 30m`; use purpose `dev` without a timeout for servers. Follow `utility-shell-reports` for pushed results.
- Run `build.sh fmt` once per batch, then one verification pass (one `lane-check` call for all touched crates and affected non-Rust checks); on failure, fix all reported errors and recheck only what failed. Cold checks require Orchestrator GO; no worker builds, workspace or reverse-dependency checks without an exact assignment.
- Workers run no checks on a shared machine unless dispatch grants them; they report the exact checks to run. The Orchestrator runs them in canonical after merging, batched: one `lane-check` for all touched crates and one `./build.sh nextest <scope> "test(a) | test(b)"` per crate scope, never one `test-one` per selector. Rust tests use `scripts/test-capped.sh`; broad regression runs once on the release SHA.
- Run `fmt-check`, changed TS/JS tests, size/boundary guards, and `pnpm check:api-effects` when effects change. Require zero compiler errors/warnings and report unrun checks honestly.
- A busy gateway is a refusal: continue source work, never poll/retry. `--wait-build` requires intentional blocking. Stop foreground dev before canonical QA; do not evade the gateway with another target directory.
- Regenerate CLI/other generated contracts through the build skill; never edit generated files manually. Use `build.sh help` for commands.
- Self-install: Windows uses `install-local`; macOS uses `codeterm update install` with continuation per the build skill. Never replace an installed bundle by hand.

## Build speed (worktrees, tests, profiles)

- Keep machine-level sccache and tuned Cargo profiles. Never share final target directories across worktrees or hand-compose cache/gateway settings.
- `lane-check` alone selects shared lane intermediates; canonical gateway jobs own the stable `X:` mapping. Use `build.sh gc` for safe retention cleanup.
- Warm-build/QA machine selection lives in `codeterm mem`; record it before dispatch. Cache limits and timeout controls belong in the build/gates skills.
- After any Rust dependency change, run `cargo hakari generate` in `src-tauri`. A new app-only (GUI) dependency goes into the hakari final-excludes; prove the `domios-server` and `codeterm-cli` closures gain no crate and `check-crate-layering.sh` passes. Procedure: build skill, *Workspace-hack*.

## Lane integration preserves ancestry

- Never cherry-pick, squash, replay, or patch-copy lane work. Real-merge it; delete only after `git merge-base --is-ancestor <lane-tip> <target-tip>` succeeds.
- Canonical has one writer/integrator. Parallel source writers use separate lanes.
- Before debugging, compare `/api/health` build with the intended source SHA. Use the dev-run skill to stop/restart stale DEV; never prod. Keep one foreground dev/build owner.

## Testing

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

## Project skills

Repository-local operational skills live in `.agents/skills/`. Load the matching skill for dev, testing, prompt evaluation, gates, or release work instead of duplicating its procedure here.

Before running `build.ps1 test`, `release-preflight`, or a release, load `.agents/skills/gates/SKILL.md`. It owns which checks you run here and which you delegate, and the traps that make a gate lie about its own result.

## Orchestration failure visibility

Keep agents/utilities/crons in their Team. A failed dispatch, missing ACK/session/brief, vanished Worker, or lost assignment is a product incident: immediately tell the owner its impact, affected tasks, cleanup, and fallback. Never silently retry or claim success.

## Debug instrumentation

Use `log::debug!`/`trace!` for retained diagnostics; release strips them. Frontend diagnostics require `DEV_BUILD` from `src/lib/devBuild.ts`, not `import.meta.env.DEV`. No temporary info-level logs or bare console output in shipped code.

## Code style

SOLID and DRY. Prefer clear names over comments; WHY comments only, at most two lines (license/public API docs excepted). Read `.agents/skills/style/SKILL.md`.

## Architecture (agent providers, detection)

Agent integration is abstracted via providers under `src-tauri/src/agent/`. Live: Claude, Codex, Opencode and Aider (PTY agents). Architecture-ready: Goose. Providers expose a unified capability descriptor (`transport` = pty | chat_backend, `behavior_config`, models, presets) — branch on capability, never a provider id. Detection uses **structural fingerprints**, never literal strings — vendors change copy without notice. Full breakdown: **`essences/core.md`**.

### Plugin & capability discipline (enforce on every provider change)

The provider/plugin boundary is the project's load-bearing seam. It stays clean only if every change crossing it pays the same tax. Non-negotiable when you add or extend a provider capability (a new `AgentProvider` trait method, a new host-API function, a new detection source):

1. **Core asks capabilities, never identities.** Branch on `supports_chat_ui()` / `available_models()` / a new capability fn — never on a hardcoded provider-id list or display string. If you're matching `"codex"` / `"opencode"` in core code, you've leaked the boundary.
2. **Native + JS parity in lockstep.** Any trait method a JS drop-in can implement MUST gain a case in `parity_test.rs` proving the native and JS paths agree on shared fixtures. A capability without a parity test will silently drift on drop-ins.
3. **Detection keys off the tab's OWN session.** Provider detection (model, session, status) reads only the record belonging to this tab's claimed session id — never "latest global" / "newest for cwd". Unknown → return null (honest) over a guess (contaminating). This is a real bug class we've shipped.
4. **Audit the host-API surface when it grows.** Every new host function a drop-in can call is a new confinement hole — justify it against the FS/sqlite/env sandbox, and remember QuickJS lacks bindings the native runtime has (a missing `console` once aborted parsing silently).
5. **Release plugins independently of the app.** Each official plugin owns its version; its manifest, package and `plugins/channel.json` entry must agree. `minCodeterm` declares the oldest host implementing the capabilities that release needs; raise it only when a host dependency changes. `npm run sync:versions` changes app and shared-package versions and validates plugin metadata without rewriting it. Prepare a plugin release with `node scripts/release-plugin.mjs bump <id> <version> [--min-codeterm <version>]`, merge its verified PR, then tag the master commit `plugin-<id>-v<version>`. `.github/workflows/plugin-release.yml` checks and publishes that plugin without building the app. Plugin-only PRs run plugin checks; host changes retain full host CI. Official `codeterm-core` installs stay trusted, and startup never downgrades a newer channel release to the embedded fallback.

Full rationale + the boundary map: **`essences/core.md`**.

**Where plugin source lives — not all of it is in this repo.** Bundled system providers and capabilities ship from the in-repo `plugins/` channel (`codeterm-core`) with independent plugin versions. The other **official, non-system capability plugins are NOT in this repo** — Bitwarden, Transcriber, and friends live in the separate **`codeterm-plugins`** repo (`~/Developer/codeterm-plugins`, channel `codeterm-plugins`, `github.com/rollacode/codeterm-plugins`), authored in TS against `@codeterm/plugin-sdk` and built to a QuickJS `plugin.js` + split UI bundles via `node scripts/build-plugin.mjs <id>`. Installed copies live under `~/.codeterm/plugins/<id>/`. Use the official channel to update reserved system ids; local directory installation does not authorize replacing them.

## Code navigation

Prefer cached SCIP indexes for definitions, references, and implementation graphs; use `rg` for exact text.

## GitHub and release

- Issue and commit titles use `<scope>: description`, never bracket tags.
- Load `.agents/skills/github-release/SKILL.md` for release and issue-linking procedures. Never direct-push `master`, move published tags, or release an unverified head.
- Link fixed issues in both commit and PR; use `Refs` for context only. Integration PRs use `Repository-Issue`, final release PRs use generated closing keywords, and public issues close only after verified publication; details are in the release skill.
- For CLI contract changes, load `.agents/skills/cli-contributor/SKILL.md` and update affected docs, prompts, skills, examples and contract tests.

## Documentation

Read `essences/begin_here.md` before editing any doc. Bump the `<!-- revision: N -->` tag at the bottom of any `prod-docs/` doc you change; essence docs carry no revision tag.

- `essences/` — internal stable product material. Start at `essences/begin_here.md`, then read `essences/capabilities.md`, `essences/core.md`, and `essences/vision.md`. Step-by-step procedures live in `.agents/skills/`.
- `generated/cli/` — committed CLI contract artifacts: `cli.md` and `cli-manifest.json`, generated from the clap tree by `codeterm cli-manifest --write`.
- `prod-docs/` — exactly the docs embedded in the binary and read with `codeterm docs <NAME>`, one `prod-docs/<NAME>.md` per registry entry: for example `prod-docs/API.md`, `prod-docs/CONFIG.md`, `prod-docs/MESH.md`, `prod-docs/MEMORY.md`, `prod-docs/TAB-LIFECYCLE.md`. Its audience is a production agent using Domios; a doc about Domios's own source (prompt sources, detection internals, file layout) is useless there because that agent cannot edit the binary, so it lives in `.agents/skills/` or `essences/`.
- `.agents/skills/` — step-by-step procedures (dev run, dev testing, gates, release, prompt evaluation, lane cleanup); not a docs area.

<!-- revision: 45 -->
