# CodeTerm Changelog

All notable changes to CodeTerm. Format loosely follows [Keep a
Changelog](https://keepachangelog.com/), versioning is
[SemVer](https://semver.org/).

This file starts at the v1.0.1 → v1.1.0 cutover; pre-v1.0.1 history
lives in `git log` with no aggregated narrative.

## [Unreleased]

### Added

- **`codeterm bridge migrate --to PEER`** — hot-relocate the active
  Telegram poller (and any future single-poller bridge) from the
  current host to a paired mesh peer without restarting either daemon.
  Orchestrates a six-step flow over `/api/bridge/{state,start,stop,announce}`
  + the existing mesh relay: source preflight → release source →
  enable + claim target via the target's own vault → verify both →
  optional `--announce` sentinel → auto-rollback on any failure.
  Exit codes: `0` success, `1` generic failure (rollback OK), `5`
  preflight (target unreachable / missing peer / no token + no vault),
  `6` usage error (running via `--mesh` is rejected — must run on the
  source machine), `7` split-brain (rollback also failed). Supports
  `--service` (default `telegram`), `--announce`,
  `--announce-chat-id` (default `186809246`), `--no-rollback`,
  `--force`, and `--claim-timeout-ms` (default `10000`). Each daemon
  resolves its own vault — secrets never cross the mesh, matching
  the existing `--mesh PEER mem secret …` local-only contract.
- **REST API: bridge runtime control.** Four new endpoints under
  `/api/bridge/*`:
  - `GET /api/bridge/state` — runtime + config snapshot
    (`active`, `config_enabled`, `token_present`,
    `has_secret_in_vault`, `started_at`, `last_poll_ok`,
    `last_poll_error`, `error_count`, `instance_name`).
    `has_secret_in_vault` uses `SecretStore::has` and never decrypts.
  - `POST /api/bridge/start` — `{service?, ensure_token?, wait_for_lock_ms?}`.
    With `ensure_token: true` the daemon pulls
    `telegram_bot_token` from the local vault when the token file is
    missing (412 if not in vault). Persists
    `integrations.telegram_enabled=true`, then waits for
    `last_poll_ok >= started_at` to confirm the upstream lock was
    claimed. 409 on Telegram conflict (another poller still active).
  - `POST /api/bridge/stop` — `{service?, timeout_ms?}`. Persists
    `telegram_enabled=false` **only after** the supervisor stop
    succeeds, so timeouts don't desync config from runtime.
    408 on timeout, idempotent when already idle.
  - `POST /api/bridge/announce` — `{service?, chat_id, text}`. One-shot
    outbound text via the existing channel; 503 if the bridge isn't
    active locally.
- **`BridgeSupervisor::op_guard()`** (tokio Mutex) serializes start /
  stop / boot-time spawn so concurrent operations on the same daemon
  never race.

### Changed

- **Docs (`docs/API.md`, `docs/MESH.md`, `docs/SKILLS.md`)** updated
  to cover the new REST endpoints and the migration orchestration
  including secret-locality guarantee and exit codes. Codeterm skill
  (`skills/codeterm/SKILL.md` + `references/telegram.md`) gained a
  `bridge migrate` quick-reference.

### Fixed

- **Team bootstrap no longer burns a turn on a readiness ping.** A freshly
  paired Planner read "say you are ready" as "tell the Orchestrator", wrote a
  pad file and sent it across, which forced the Orchestrator to wake, read and
  reply before the user had stated any goal. Both idle wake-up surfaces
  (`prompts/team_bootstrap_starter.md`, `prompts/planner_suffix.md`) now keep
  the acknowledgement inside the agent's own tab and name the counterpart
  message as reserved for a draft, decision, evidence or state change. The
  Planner surface prices every later message the same way, so a bare
  acknowledgement or restatement is not sent at all. Two prompt-surface
  regression tests guard the wake-up and the standing rule.

## [1.1.14] — 2026-05-05

Patch: stop the periodic console-window flash users were seeing on
Windows every ~15 seconds. Mesh discovery was shelling out to
`tailscale.exe ip -4` to learn its CGNAT IP, and each spawn briefly
attached a fresh `conhost` window before the GUI app got control back.

### Fixed

- **No more console flashing on Windows** — replaced the periodic
  `tailscale.exe` invocation in mesh discovery with a programmatic
  scan of local network interfaces (`if-addrs`) filtering for the
  Tailscale CGNAT range `100.64.0.0/10`. Same result, no subprocess,
  no flicker. The `/api/network` endpoint that exposed the same probe
  now reuses the helper.
- **Audited every other external-binary spawn** for the same
  conhost-flash failure mode and routed each through the existing
  `process_util::no_window_std` / `no_window_tokio` helper:
  `schtasks` (self-update + orchestrator), `reg` (PATH install),
  `where`/`which` (CLI probe), `opencode` (model list), and the
  `codeterm mem run-worker` subprocess.

## [1.1.1] — 2026-04-29

Patch: nightly memory dreamer was being marked Failed in cron history
even though the REM worker spawned successfully and produced
`daily_dream_<date>` mem items each night. Plus per-platform CI
plumbing on the develop branch.

### Fixed

- **Cron status race after explicit `cron complete`** — when an agent
  inside a cron tab called `codeterm cron complete`, the backend
  marked the run Success and closed the tab, but the
  `RunWatcherRegistry` watchdog kept its 30-second idle timer alive.
  After the tab closed there was no more output, the timer tripped,
  and `finish_run` overwrote the Success with Failed. `cron_complete`
  now calls `run_watcher.mark_completed_externally` on the tab's pty
  before `close_tab`, so the watchdog exits cleanly. Confirmed
  against the dreamer's 2026-04-25 → 2026-04-29 run logs.

### Added

- **Per-platform release workflows** — `.github/workflows/release.yml`
  was split into `release-windows.yml`, `release-macos.yml`, and
  `release-linux.yml`. Each builds independently on a tag push and can
  be suppressed via repo Variables `RELEASE_WINDOWS_ENABLED` /
  `RELEASE_MACOS_ENABLED` / `RELEASE_LINUX_ENABLED` (set to `false`).
  Secret rename `TAURI_PRIVATE_KEY` → `TAURI_SIGNING_PRIVATE_KEY`
  matches the actual repo secret.
- **Documentation review pass** — README translations carry
  TRANSLATION-TODO markers for the three sections that drifted from
  English (top-of-features GIFs, per-OS install split, "Canvas Tabls"
  rename). Stale references to deleted plans / endpoints in `docs/`,
  `.claude/agents/`, and prompt files are corrected.
- **Stronger orchestrator suffix** — `prompts/orchestrator_suffix.md`
  now mandates a final `codeterm agent report` call so spawning
  parents see completion immediately instead of polling status.

### Notes

- v1.1.1 ships **Windows-only** assets directly from a local BLACKSTAR
  build. The new per-platform workflows landed but went un-validated
  in CI because Actions free minutes were exhausted; revisit after the
  monthly quota resets. macOS DMGs are still produced manually from
  MacBook per `github-release` skill.

## [1.1.0] — 2026-04-29

The "resilience + features" push. Seven feature branches plus three
cleanup followups landed in `develop` on 2026-04-25; a pty cleanup fix
merged on 2026-04-26 and the wake-up auto-injection on 2026-04-29.
Design rationale for the 04-25 batch lives in the mem decisions chain
(`fact_design_decisions_chain_2026_04_25_codeterm_resilience_featur`).

### Added

- **General Agent wake-up auto-injection** — the L0 identity + L1
  critical-context payload from `codeterm mem wake-up` is now rendered
  directly into the GA system prompt under a new `## Live mem snapshot`
  section, so the agent boots already aware of identity, rules, peers,
  and critical facts without spending a turn on the CLI. Wake-up
  failures fall back to an empty section so GA bootstrap never blocks
  on mem. (4ea3e3f, merged in 4eb6088.)

- **Self-update via `tauri-plugin-updater`** — `codeterm update --check`,
  `--apply`, `--channel <stable|rc|nightly>`. Manifests are signed with
  ed25519 (public key embedded in `tauri.conf.json`). Auto-install runs
  the 9-step graceful shutdown (mem WAL checkpoint, supervisor stop,
  signed binary swap) and tabs auto-resume after restart. CI release
  workflow ships Win + macOS Intel + macOS ARM signed assets +
  channel-aware manifests. (Phases 1–4: 9215040, f1dfc7f, 5a6658b,
  fe8e738; merged in 540d42e.) See
  [`docs/contributing/auto-update-testing.md`](docs/contributing/auto-update-testing.md)
  and [`docs/plans/auto-update.md`](docs/plans/auto-update.md).

- **Agent restart CLI / API** — `codeterm agent restart --tab ID
  [--force] [--grace-secs N]` and `POST /api/tabs/{id}/agent/restart`.
  Reuses the existing `/exit` + auto-resume tab lifecycle (no new
  restart logic). Returns `202` with `session_was_stale` to signal
  whether the recorded session id was still valid; `429` rate-limits
  per-tab to avoid restart loops. (f7e130a, merged in d9577d8.)

- **Voice transcription in the Telegram bridge** — when
  `[integrations.voice_transcribe] enabled = true`, the bridge POSTs
  audio attachments to a local Whisper daemon (`daemon_url`, default
  `http://127.0.0.1:7891`) and substitutes the returned text inline
  before the message reaches any agent PTY. Covers Claude, Codex, and
  future agents with one upstream change instead of per-client hooks.
  (f994f48, merged in 6f99c38.) See
  [`docs/contributing/voice-transcription-testing.md`](docs/contributing/voice-transcription-testing.md).

- **Telegram HTML sanitizer** (`src-tauri/src/bridge/sanitize.rs`) —
  whitelists Bot-API HTML tags (`<b>`/`<strong>`, `<i>`/`<em>`, `<u>`,
  `<s>`, `<code>`, `<pre>`, `<a>`, `<tg-spoiler>`, `<blockquote>` and
  synonyms) and HTML-escapes everything else, including stray
  `<word>` substrings (e.g. `<date>` placeholders) that previously
  broke Telegram's parser. Long messages are split tag-aware so a
  `<code>` block can't be cut mid-tag. The General Agent prompt was
  updated to document the new whitelist. (f644a06, merged in 6b5a5ee.)

- **Mem subprocess supervisor with circuit breaker** — dreaming and
  librarian extraction now run as child processes of the daemon, gated
  by a circuit breaker (3 failures in 60s opens the circuit, manual
  reset). Worker death no longer takes the API down. Routed via
  `codeterm mem run-worker`. (0c94077, 4e29b75, eaa414e, merged in
  400778e.) See [`docs/RESILIENCE.md`](docs/RESILIENCE.md).

- **Mem heavy-ops semaphore + 503 timeout** — bounded concurrency on
  the heavy mem path; API connection-acquisition returns `503` after
  a short timeout instead of blocking indefinitely. Plus per-batch
  lock release in dreaming so API requests always win contention.
  (b54d5c8, 8a2d003, f67cf85; merged in 400778e.)

- **`codeterm agent reports` listing fix** — the parent-side reports
  CLI now correctly resolves the read-key against the writer's key
  (extracted as the free fn `resolve_report_target` with regression
  tests). Added `--limit N` and reverse-output ordering. (a5fe019,
  merged in 5505465.)

### Fixed

- **PTY child process termination on `PtyManager::kill`** — closing a
  tab now explicitly kills the child process instead of relying on
  shell-side cleanup, fixing leaked subprocesses when the user closed
  a tab mid-command. (be0d045, merged in 8227b5e.)

### Changed

- **`resolve_cli_path` resolution chain** — 5 steps:
  `CODETERM_CLI_PATH` env override → `data_dir/bin/codeterm` →
  `current_exe`-as-CLI → sibling lookup of `current_exe` → `PATH`.
  Replaces the 2-step heuristic that broke on Tauri MSI installs and
  macOS `.app` bundles. (24bd8a8.)

- **General Agent prompt** (`src-tauri/prompts/general_agent.md`) —
  new sections for self-update, agent restart, and voice transcription;
  Telegram-formatting section now documents the sanitizer.

### Internal cleanup

- Dropped dead in-process dreaming/librarian wrappers, gated the
  remaining helpers as `#[cfg(test)]` (5d3ca26).
- Annotated dreaming async test fixtures + the CLI SQL mirror so the
  sync/async split is visible at the call site (f1ea1f9).
- Test verifying in-process vs subprocess `history_log` parity
  (5d53208) — closes the cleanup followup #4 from the resilience plan.

### Documentation

- `docs/API.md` revision 30 → 31: documented agent restart endpoint
  + body, expanded the Updates section with channel endpoints +
  auto-install + manifest_url override semantics, and reorganised the
  cron paragraphs that had drifted under the wrong header.
- New contributor playbooks: `docs/contributing/voice-transcription-testing.md`,
  `docs/contributing/agent-restart-testing.md`,
  `docs/contributing/auto-update-testing.md`,
  `docs/contributing/auto-update-signing.md`.
- `docs/plans/auto-update.md` flagged as **shipped 2026-04-25**.
- README mirrored to all translation locales (auto-translated drafts)
  with the new agent restart, bridges, voice transcription,
  self-update, and supervisor sections. (1fb2bab, 12ea9a1.)
