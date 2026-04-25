# CodeTerm Changelog

All notable changes to CodeTerm. Format loosely follows [Keep a
Changelog](https://keepachangelog.com/), versioning is
[SemVer](https://semver.org/).

This file starts at the v1.0.1 → v1.1.0 cutover; pre-v1.0.1 history
lives in `git log` with no aggregated narrative.

## [Unreleased] — 2026-04-25 develop

The "resilience + features" push. All seven feature branches plus
three cleanup followups landed in `develop` on 2026-04-25; design
rationale for each item lives in the mem decisions chain
(`fact_design_decisions_chain_2026_04_25_codeterm_resilience_featur`).

### Added

- **Self-update via `tauri-plugin-updater`** — `codeterm update --check`,
  `--apply`, `--channel <stable|rc|nightly>`. Manifests are signed with
  ed25519 (public key embedded in `tauri.conf.json`). Auto-install runs
  the 9-step graceful shutdown (mem WAL checkpoint, supervisor stop,
  signed binary swap) and panes auto-resume after restart. CI release
  workflow ships Win + macOS Intel + macOS ARM signed assets +
  channel-aware manifests. (Phases 1–4: 9215040, f1dfc7f, 5a6658b,
  fe8e738; merged in 540d42e.) See
  [`docs/contributing/auto-update-testing.md`](docs/contributing/auto-update-testing.md)
  and [`docs/plans/auto-update.md`](docs/plans/auto-update.md).

- **Agent restart CLI / API** — `codeterm agent restart --pane ID
  [--force] [--grace-secs N]` and `POST /api/panes/{id}/agent/restart`.
  Reuses the existing `/exit` + auto-resume pane lifecycle (no new
  restart logic). Returns `202` with `session_was_stale` to signal
  whether the recorded session id was still valid; `429` rate-limits
  per-pane to avoid restart loops. (f7e130a, merged in d9577d8.)

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
