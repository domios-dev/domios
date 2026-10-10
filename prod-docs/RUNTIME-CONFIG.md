# Runtime configuration

Configuration for autostart, themes, sessions, cron, UI gates, memory, and dev isolation.

## OS Autostart

Domios can launch at login, starting minimized to the system tray.

- **Windows:** HKCU registry key (`Software\Microsoft\Windows\CurrentVersion\Run`)
- **macOS:** LaunchAgent plist (`~/Library/LaunchAgents/com.codeterm.app.plist`)
- **Linux:** `.desktop` file (`~/.config/autostart/codeterm.desktop`)

Toggle via Settings UI (Agents tab) or Tauri IPC commands `get_autostart_enabled` / `set_autostart_enabled`. Also available in the Onboarding flow.

## Themes

| Built-in | Look |
|---|---|
| GitHub Dark (default) | Dark theme matching GitHub's dark mode |
| Dracula | Popular purple-tinted dark theme |
| Catppuccin Mocha | Warm pastel dark theme |
| One Dark | Atom-inspired dark theme |
| Nord | Arctic blue-tinted theme |
| GitHub Light | Light theme matching GitHub's light mode |
| Solarized Light | Ethan Schoonover's warm light color scheme |
| Pip-Boy | Retro green-on-black Fallout terminal theme; selecting it activates CRT effects: scanlines, phosphor glow, and screen vignette |

`codeterm theme list` shows built-in and custom themes; `theme apply NAME`, `theme dir` (custom themes directory), `theme save ID FILE.toml [--apply]` and `theme delete ID` manage them (REST: `codeterm docs API-TABS`).

A custom theme is a TOML file with `name` plus 20 color fields: `background`, `foreground`, `cursor`, `selection`, then `black`, `red`, `green`, `yellow`, `blue`, `magenta`, `cyan`, `white` and their `bright_` variants (`bright_black` … `bright_white`), each a `"#rrggbb"` string. UI chrome (sidebar, surfaces, borders) is derived from the background and palette, so light and dark themes need no extra fields.

## Session Persistence

Desk state—workspaces, tabs, layout, and tab identity—is committed through the instance's Storage owner in `storage/state.db` beneath its data directory. Ordinary mutations are persisted through Storage; `POST /api/session/save` provides an explicit flush barrier before a forced stop. The Cron Jobs section below describes Storage unavailable mode and the backup/rollback protocol.

## CWD Tracking

Domios tracks the current working directory of each tab via invisible OSC 7 shell integration. No visible commands are injected into the terminal.

Generated integration files need no manual edits.

Shell integration scripts are stored in the platform data directory:
- **macOS:** `~/Library/Application Support/codeterm/shell-integration/`
- **Linux:** `~/.local/share/codeterm/shell-integration/`
- **Windows:** `%APPDATA%/codeterm/shell-integration/`

Domios creates these scripts at startup.

## Cron Jobs

Cron jobs and run history belong to the instance's typed Storage state in `storage/state.db` beneath its data directory, not `config.toml`. They are managed through the APIs in `codeterm docs API-INTEGRATIONS`, while the schema is documented here. Closing a workspace deletes its cron jobs, run history, and log directories. Storage opens before cron restore or scheduler start.

If Storage cannot open or its one-time import fails verification, cron state is unavailable: dependent reads and writes report the Storage failure, scheduling does not fall back to an empty in-memory list, and no jobs execute from a substitute. The importer keeps recognized legacy inputs under `storage/legacy-originals/` after verifying the archive; those files are retained import evidence, not live configuration.

For backup, stop Domios cleanly and preserve the complete `storage/` directory, including the database, referenced assets, and import archive. Restore it only into an empty `storage/` root with a Storage-compatible build. Do not downgrade by starting an older binary against the retained legacy JSON: it does not contain post-cutover writes. No in-place downgrade or JSON rollback is supported.

### `CronJob` schema

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `id` | `string` | — | Stable job id (8-char hex) |
| `name` | `string` | — | Display name |
| `description` | `string?` | — | Optional free-text description |
| `workspace_id` | `string` | — | Owning workspace |
| `schedule` | `string` | — | Exactly 5-field cron (`min hour dom mon dow`) with a fire within the next 5 years, or a oneshot expression (`"in 5m"`, ISO 8601, unix ms) |
| `enabled` | `bool` | `true` | Whether the scheduler fires this job |
| `agent` | `string` | `"claude"` | Agent provider id (see `codeterm agent providers`) |
| `model` | `string?` | *(provider default)* | Model override within the provider (e.g. `"opus"`, `"haiku"`). Query valid values via `codeterm agent models <provider>`. |
| `task` | `string` | — | Prompt sent to the agent via `-p` |
| `cwd` | `string?` | *(workspace cwd)* | Working directory for the spawned agent |
| `role` | `string?` | — | Optional hierarchy role to assign on spawn |
| `report_target_tab_id` | `string?` | — | Tab that receives the completed cron run status and summary |
| `oneshot` | `bool` | `false` | One-shot job: auto-disabled after first fire, deleted on completion |
| `jitter_max_ms` | `u64?` | `null` | See jitter semantics below |
| `max_history` | `usize` | `100` | Per-job run history cap |
| `consecutive_failures` | `u32` | `0` | Runs failed in a row. Auto-disables at ≥ 5. |
| `protected` | `bool` | `false` | Protected jobs cannot be deleted, only toggled |
| `last_run` | `CronRunInfo?` | — | Last run summary (status, tab, timing) |
| `next_run` | `u64?` | — | Next scheduled fire (unix ms). `null` for disabled / exhausted oneshots. |

### Jitter semantics (`jitter_max_ms`)

- `null` (omitted) → **auto**: 10% of the inter-fire interval, up to 30m. Default.
- `0` → **disabled**: jobs fire exactly on schedule.
- positive integer → requested maximum random delay in milliseconds, capped at half the inter-fire interval.

Jitter is applied on top of recurring cron fire times. A job with `0 * * * *` and `jitter_max_ms=5000` fires between `:00.000` and `:05.000` each hour. Auto-mode exists so users on `*/5 * * * *` schedules don't all hammer LLM providers at the same minute boundary.

### Missed slots

A recurring job whose scheduled (jittered) fire time passed more than 10 minutes ago, for example because the machine slept or Domios started late, does not fire late: it moves to its next future slot and the scheduler logs the skip. A slot missed by 10 minutes or less still fires once. A oneshot job always fires, however late.

### Hidden spawn

Cron runs spawn agents in **hidden tabs** — no separate visible tab. Observe progress via the cron tabl in the sidebar, the frontend log viewer, or `codeterm cron history ID-or-name`. Use `cron_read_log` (Tauri IPC) / the live tab fallback to tail a running run's output.

## UI feature gates

Controls which sidebar buttons are visible. Set in `[ui]` in `config.toml`.

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `ui.memory_tab_enabled` | `bool` | `true` | Show/hide the Memory sidebar button |
| `ui.mesh_tab_enabled` | `bool` | `true` | Show/hide the Mesh sidebar button |

```toml
[ui]
memory_tab_enabled = true
mesh_tab_enabled = true
```

## Memory

Mem state lives in `~/.codeterm/mem.sqlite` (dev: `~/.codeterm-dev/mem.sqlite`). Config knobs that affect mem are under `[memory]` in `config.toml`.

Saves can finish before semantic indexing; text search remains available meanwhile.

### Auto-extraction concurrency

Saving a mem item triggers an extraction cascade that pulls structured entities and relationships out of the body. Extraction runs in hidden workers and is limited to **4 concurrent workers**; a fifth concurrent save waits for a free slot.

To suppress extraction on a specific save — used by seed loaders to avoid re-extracting bundled items on every boot — pass `skip_extract: true` on the `POST /api/mem/items` body (or via the matching socket method). Items tagged `auto-extracted` or sourced from `extracted_from:*` are also skipped automatically.

### Worker timeouts

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `memory.worker_timeout_seconds` | `u64` | `300` | Hard timeout for non-dreaming hidden agent runs (extraction workers, generic cron). The auto-complete watcher marks the run `Failed` if the agent has not emitted a stream-json result event within this many seconds. |
| `memory.dreaming_timeout_seconds` | `u64` | `1800` | Hard timeout for the Dreamer (REM phase) specifically. Hub decomposition + diary + classification in one session needs room to think. Lower if your dreaming model is fast. |

### Wake-up budget

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `memory.l1_budget_tokens` | `usize` | `300` | Token budget for the L1 critical-context layer in `codeterm mem wake-up` (and `GET /api/mem/wake-up` when no `?budget=` is passed). L0 identity always renders first (~100 tok); L1 fills the remainder. Pinned items beat the score-ranked fill but still respect this budget — when the pinned set alone overflows it, the rendered output starts with `<!-- WARN: N pinned items did not fit L1 budget … -->` and a `[wake-up] pinned-eviction` log line fires. |

```toml
[memory]
l1_budget_tokens = 300
```

### Dreamer schedule

The bundled `mem-dreaming-auto` cron runs the dreamer at `0 6 * * *` (6 AM local time) with a 0..30 min delay-only jitter window — so it fires between 6:00 and 6:30 AM. Schedule and jitter are not config keys. Custom schedules a user has set explicitly are left alone. The dreamer additionally runs **STAGE D — Memory calibration** on Sundays only (server-gated); the endpoints are documented in `codeterm docs API-MEMORY`. Every night it can also archive year-untouched items, only when `[memory.dream_hygiene] stale_archive = true` (default `false`); `stale_archive_cap` (default 20) and `merge_cap` (default 10) bound the nightly stale archives and Dreamer merges (`codeterm docs CONFIG`).

### Supporter Agent

The Supporter Agent runs memory re-rank and tab-state whenever its selected adapter is connected. Automatic plan reminders are the exception to deterministic fallback: without the connected adapter's approving choice none is sent (`codeterm docs ORCHESTRATION`). Memory search can force re-rank with `supporter=true|false`; adapter failures return plain recall with a `supporter.status = "fallback"` envelope. The shared `codeterm supporter status` and `/api/supporter/status` response reports the selected adapter and model, token status, `connected`, `last_activity`, `calls_last_hour`, `calls_last_day`, `last_verdict`, and `budget` (reached, calls_per_hour, free_at). `spend` holds today/month token and estimated USD totals, exhaustion and next probe time; prices are adapter-declared, never assumed for unpriced adapters. The token is set in Settings > Agents > Supporter Agent or through bearer-only `/api/supporter/token`, and is stored under `<data dir>/supporter/`, never in `config.toml`.

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `supporter.adapter` | `string?` | absent | Active adapter; absent means the Supporter is stopped with deterministic fallback. |
| `supporter.connections.<id>.endpoint` | `string?` | adapter default | Per-adapter endpoint override, read at daemon start by adapters that support it. |
| `supporter.connections.<id>.model` | `string?` | first catalogue model | Per-adapter saved model; changing it immediately updates that adapter without activating it. |
| `supporter.calls_per_hour` | optional integer | absent | Shared hourly cap (`0..=100000`); absent or 0 is unlimited. |
| `supporter.deadline_ms` | `u64` | `3000` | Failure deadline for a model stage (`100..=30000`). |
| `supporter.memory_rerank.docs_hybrid` | `bool` | `false` | Use cached vectors when ranking embedded docs candidates. |
| `supporter.memory_rerank.sources.recall` | `usize` | `30` | Recall candidates in the pool (`1..=50`); never fewer than the request limit. |
| `supporter.memory_rerank.sources.graph` | `usize` | `3` | Extra graph-linked candidates the recall pool lacks (`0..=20`); `0` leaves the pool recall-only. |
| `supporter.memory_rerank.sources.docs` | `usize` | `3` | Extra embedded doc sections and CLI command sections (`0..=10`), selected by keyword match; `0` disables docs candidates. |
| `supporter.memory_rerank.sources.sessions` | `usize` | `0` | Top past-session snippets from the in-process sessions search (`0..=5`); skipped on error or deadline, and `0` disables session candidates. |
| `supporter.memory_rerank.candidates` | `usize` | `50` | Maximum pooled candidates (`1..=50`). |
| `supporter.memory_rerank.max_concurrency` | `usize` | `16` | Parallel per-candidate calls (`1..=32`); shared-state uses one call. |
| `supporter.memory_rerank.request_mode` | enum | `per_candidate` | Diagnostic A/B path: `per_candidate` or `shared_state`; promote fan-out only after live quality qualification. |
| `supporter.memory_rerank.low_confidence_threshold` | `f64` | `0.15` | `low_confidence` is reported when the best relevance is below this (`0..=1`). |
| `supporter.memory_rerank.max_body_chars` | `usize` | `2000` | Maximum body characters sent to the adapter (`256..=100000`); titles are included without truncation. |

```toml
[supporter]
adapter = "typesafe-jev"
deadline_ms = 3000
calls_per_hour = 0 # Unlimited

[supporter.connections.typesafe-jev]
model = "jev-latest"

[supporter.memory_rerank]
request_mode = "per_candidate"
docs_hybrid = false

[supporter.memory_rerank.sources]
recall = 30
graph = 3
docs = 3
sessions = 0

```

Requests are counted without a cap by default. An explicit positive cap counts requests, not questions: batched shared-state searches and multi-question tab observations each count as one physical HTTP request. Sequential adapters may use multiple requests per shared-state search; local decisions cost no request. Status reports a null cap when unlimited and live hourly/daily counts. Usage and spend counters persist across restarts. HTTP 402 or explicit credit exhaustion enters `credit_exhausted`, writes one failure alert and serves deterministic fallback. A later request probe may recover; probes are triggered by normal requests, never a timer.

Saved membership is the union of the connections table, the active adapter and adapters with stored tokens. Switching keeps other tokens and models. `codeterm supporter disconnect <adapter>` removes that connection and token, preserving usage history; removing the active connection stops the Supporter without selecting another.

### Librarian

The Librarian is a background skill vectorizer that watches skill folders and re-indexes `SKILL.md` files when they change on disk.

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `memory.librarian_enabled` | `bool` | `true` | Enable the Librarian background vectorizer |

```toml
[memory]
librarian_enabled = true
```

**Note:** as of v1.2.0 the runtime gate is stored in config but not yet wired to the Librarian startup path. Setting `librarian_enabled = false` persists the preference but does not stop the Librarian in the current release.

## Past Agent Sessions

There is no per-session config: each provider owns where its sessions live. The `codeterm sessions` commands and `/api/sessions/*` find and resume them; `codeterm docs API-STATE` has the contract.

## Dev/Prod Isolation

Dev builds use separate paths and ports to avoid conflicts with production:

| Setting | Dev | Prod |
|---------|-----|------|
| Identifier | `com.codeterm.dev` | `com.codeterm.app` |
| API port | 7686 | 7685 |
| Data dir | `~/.codeterm-dev/` | `~/.codeterm/` |
| Socket prefix | `codeterm-dev` | `codeterm` |
| Single-instance | Disabled | Enabled |

Both builds can run simultaneously.

<!-- revision: 30 -->
