# Runtime configuration

Configuration for autostart, themes, sessions, cron, UI gates, memory, and dev isolation.

## OS Autostart

CodeTerm can launch at login, starting minimized to the system tray.

- **Windows:** HKCU registry key (`Software\Microsoft\Windows\CurrentVersion\Run`)
- **macOS:** LaunchAgent plist (`~/Library/LaunchAgents/com.codeterm.app.plist`)
- **Linux:** `.desktop` file (`~/.config/autostart/codeterm.desktop`)

Toggle via Settings UI (Agents tab) or Tauri IPC commands `get_autostart_enabled` / `set_autostart_enabled`. Also available in the Onboarding flow.

## Themes

### GitHub Dark (default)
Dark theme matching GitHub's dark mode.

### Dracula
Popular purple-tinted dark theme.

### Catppuccin Mocha
Warm pastel dark theme.

### One Dark
Atom-inspired dark theme.

### Nord
Arctic blue-tinted theme.

### GitHub Light
Light theme matching GitHub's light mode.

### Solarized Light
Ethan Schoonover's warm light color scheme.

### Pip-Boy
Retro green-on-black Fallout terminal theme. Selecting this theme automatically activates CRT effects: scanlines, phosphor glow, and screen vignette.

## Session Persistence

Sessions auto-save every 30 seconds and when you close the window. Saved to:
- **Windows:** `%APPDATA%/codeterm/sessions/last.json`
- **Linux:** `~/.config/codeterm/sessions/last.json`
- **macOS:** `~/Library/Application Support/codeterm/sessions/last.json`

Contains workspace layout and tab arrangement metadata.

## CWD Tracking

CodeTerm tracks the current working directory of each tab via invisible OSC 7 shell integration. No visible commands are injected into the terminal.

**How it works per shell:**
- **zsh** (macOS default): ZDOTDIR trick -- a temporary `.zshrc` in the integration directory sources the user's real config, then adds a `precmd` hook that emits OSC 7.
- **bash** (Linux default): `PROMPT_COMMAND` env var is set at PTY spawn time, plus `BASH_ENV` points to an integration script that sources `~/.bashrc` first.
- **PowerShell** (Windows default): Prompt injection after spawn (same as before).

Shell integration scripts are stored in the platform data directory:
- **macOS:** `~/Library/Application Support/codeterm/shell-integration/`
- **Linux:** `~/.local/share/codeterm/shell-integration/`
- **Windows:** `%APPDATA%/codeterm/shell-integration/`

Scripts are created at startup by `pty::shell_integration::ensure_scripts()`. Users should not need to edit these files.

## Cron Jobs

Cron jobs live in `~/.codeterm/crons.json` (dev: `~/.codeterm-dev/crons.json`), not in `config.toml`. They are managed through the APIs in `codeterm docs API-INTEGRATIONS`, while the schema is documented here. Closing a workspace deletes its cron jobs, run history, and log directories.

A `crons.json` that fails to load is preserved as `crons.json.corrupt-<UTC timestamp>`,
reported by one ERROR line and one action-needed notification, and recovered from
the newest valid backup in `state-backups/crons.json/` (written on every save).
Without a backup the store runs on an empty in-memory list: built-in jobs such as
`mem-dreaming-auto` run in memory only, and every save is refused until a fixed
`crons.json` is restored and CodeTerm restarts.

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

### Hidden spawn

Cron runs spawn agents in **hidden tabs** — no separate visible tab. Observe progress via the cron tabl in the sidebar, the frontend log viewer, or `codeterm cron history ID`. Use `cron_read_log` (Tauri IPC) / the live tab fallback to tail a running run's output.

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

### Embedding indexer (`EmbedQueue`)

Vector indexing uses a **single-worker** pattern: one background thread owns the sqlite writer and drains a bounded `sync_channel` (capacity 1024). Every `mem save` that needs embedding enqueues a job and returns immediately — no thread-per-save, no writer contention. Jobs dropped when the queue is full are logged and the item stays searchable via FTS5; it just won't appear in semantic-recall results until re-indexed.

### Auto-extraction concurrency

Saving a mem item triggers an extraction cascade that pulls structured entities and relationships out of the body. Extraction runs in hidden tabs spawned through `workers::spawn_headless`, and is capped at **4 concurrent workers** globally via `ServiceContext::extract_semaphore` (a `tokio::sync::Semaphore` created at startup). A fifth concurrent save queues until one of the running extractions releases its permit.

To suppress extraction on a specific save — used by seed loaders to avoid re-extracting bundled items on every boot — pass `skip_extract: true` on the `POST /api/mem/items` body (or via the matching socket method). Items tagged `auto-extracted` or sourced from `extracted_from:*` are also skipped automatically.

### Worker timeouts

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `memory.worker_timeout_seconds` | `u64` | `300` | Hard timeout for non-dreaming hidden agent runs (extraction workers, generic cron). The auto-complete watcher marks the run `Failed` if the agent has not emitted a stream-json result event within this many seconds. Was `400` before v1.2.0; lowered because real-world extractions complete well under 5 min and the longer wait was just delaying retries on stuck workers. |
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

The bundled `mem-dreaming-auto` cron runs the dreamer at `0 6 * * *` (6 AM local time) with a 0..30 min delay-only jitter window — so it fires between 6:00 and 6:30 AM. Schedule and jitter are managed by `ensure_dreaming_cron` (`src-tauri/src/mem/dreaming.rs`) and not exposed as config keys. Legacy installs that still hold the v1.1 `0 3 * * *` schedule with 0..90 min delay-only jitter are auto-migrated to the new values on the next daemon startup. Custom schedules a user has set explicitly are left alone. The dreamer additionally runs **STAGE D — Memory calibration** on Sundays only (server-gated via `strftime('%w','now') == '0'`); the endpoints are documented in `codeterm docs API-MEMORY`.

### Decision re-rank

An optional second stage over `mem search`: recall fetches a candidate pool, a registered decision adapter grades each candidate, and hits are re-sorted by that absolute relevance. Off by default; a request can force it either way with `decision_rerank=true|false`. Any adapter failure returns the plain recall hits unchanged, with a `decision_rerank.status = "fallback"` envelope naming the reason. Whenever the stage is effective (applied, fallback or skipped), only the mem hits actually returned count as accessed, once each; pool-only candidates are not touched. Adapter tokens are set through `POST /api/mem/decision/token` and stored under `<data dir>/decision-models/`, never in `config.toml`.

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `memory.decision_rerank.enabled` | `bool` | `false` | Apply the stage to every search that does not override it. |
| `memory.decision_rerank.adapter` | `string` | `"typesafe-jev"` | Registry id of the decision adapter. An unregistered id falls back with reason `no_adapter`. |
| `memory.decision_rerank.sources.recall` | `usize` | `30` | Recall candidates in the pool (`1..=50`); never fewer than the request limit. |
| `memory.decision_rerank.sources.graph` | `usize` | `3` | Extra graph-linked candidates the recall pool lacks (`0..=20`); `0` leaves the pool recall-only. |
| `memory.decision_rerank.sources.docs` | `usize` | `3` | Extra embedded prod-docs sections (`0..=10`), selected by an in-memory BM25 index built once; `0` disables docs candidates. |
| `memory.decision_rerank.max_concurrency` | `usize` | `16` | Parallel adapter calls (`1..=32`). |
| `memory.decision_rerank.deadline_ms` | `u64` | `3000` | Failure deadline for the whole stage (`100..=30000`). |
| `memory.decision_rerank.low_confidence_threshold` | `f64` | `0.15` | `low_confidence` is reported when the best relevance is below this (`0..=1`). |
| `memory.decision_rerank.max_body_chars` | `usize` | `2000` | Maximum body characters sent to the adapter (`256..=100000`); titles are included without truncation. |
| `memory.decision_rerank.endpoint` | `string?` | adapter default | Adapter endpoint override. Read at daemon start. |
| `memory.decision_rerank.model` | `string?` | adapter default | Adapter model override. Read at daemon start. |

```toml
[memory.decision_rerank]
enabled = false

[memory.decision_rerank.sources]
recall = 30
graph = 3
docs = 3
```

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

There is no per-session config — every registered `AgentProvider` decides where its session files live and how to parse them. Today:

- **Claude:** `~/.claude/projects/<encoded-cwd>/<uuid>.jsonl`. The encoded directory name is lossy (drops leading dots, mangles case), so CodeTerm reads the real cwd out of the JSONL content itself when enumerating sessions.
- **Codex:** `~/.codex/sessions/YYYY/MM/DD/rollout-<iso>-<uuid>.jsonl`. UUID v7 in the filename is used as the session id; first-line `session_meta` carries cwd.

Search and resume from the CLI with `codeterm sessions search|get|resume`. REST endpoints under `/api/sessions/*` mirror the CLI. See `codeterm docs API-STATE` for the full contract and the [core essence](../essences/core.md#agents-and-providers) for the binding model.

## Dev/Prod Isolation

Dev builds use separate paths and ports to avoid conflicts with production:

| Setting | Dev | Prod |
|---------|-----|------|
| Identifier | `com.codeterm.dev` | `com.codeterm.app` |
| API port | 7686 | 7685 |
| Data dir | `~/.codeterm-dev/` | `~/.codeterm/` |
| Socket prefix | `codeterm-dev` | `codeterm` |
| Single-instance | Disabled | Enabled |

Dev config is in `src-tauri/tauri.dev.conf.json`. Both builds can run simultaneously.

<!-- revision: 15 -->
