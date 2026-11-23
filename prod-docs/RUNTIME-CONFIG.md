# Runtime configuration

Configuration for autostart, themes, sessions, cron, UI gates, memory, and dev isolation.

## OS Autostart

Domios can launch at login, starting minimized to the system tray.

- **Windows:** `Domios-Autostart` scheduled task; supervisor failures retry up to three times, 60 seconds apart (the Task Scheduler minimum).
- **macOS:** LaunchAgent (`~/Library/LaunchAgents/com.domios.app.plist`); unsuccessful supervisor exits retry with a 30-second throttle.
- **Linux:** user systemd unit (`~/.config/systemd/user/domios.service`); `Restart=on-failure`, 30-second backoff, at most five starts per 300 seconds.

Toggle via Settings UI (Agents tab) or Tauri IPC commands `get_autostart_enabled` / `set_autostart_enabled`. Also available in the Onboarding flow.

Settings and `domios system enable-autostart` install the same supervisor registration. Successful supervisor exits, including a normal user shutdown, do not restart it. The supervisor holds an exclusive OS file lock for its lifetime; `recovery/supervisor.lock` may remain after exit or reboot, and its PID is informational. A leftover file never blocks a new supervisor unless another process currently holds its lock. Re-enable autostart to install an updated restart policy.

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

`domios theme list` shows built-in and custom themes; `theme apply NAME`, `theme dir` (custom themes directory), `theme save ID FILE.toml [--apply]` and `theme delete ID` manage them (REST: `domios docs API-TABS`).

A custom theme is a TOML file with `name` plus 20 color fields: `background`, `foreground`, `cursor`, `selection`, then `black`, `red`, `green`, `yellow`, `blue`, `magenta`, `cyan`, `white` and their `bright_` variants (`bright_black` … `bright_white`), each a `"#rrggbb"` string. UI chrome (sidebar, surfaces, borders) is derived from the background and palette, so light and dark themes need no extra fields.

### Chat background

Open Settings › General › Appearance Lab for the complete wallpaper editor. General also offers quick wallpaper picks for the current machine. The machine's saved wallpaper takes precedence over the active theme's optional `[chat_background]` table. With neither set, chat shows the plain theme background. The terminal, composer and sidebar keep their own surfaces. High-contrast and print modes show a solid transcript background. See `domios docs APPEARANCE` for complete look proposals and background strength.

The owner can run `domios wallpaper clear` to restore the theme wallpaper and `domios wallpaper set --from-pad tile.svg` to apply a static SVG. Use `--from-pad -` to read SVG from stdin. For an ordinary chat request or an owner-only write refusal, read `domios docs APPEARANCE`, create the wallpaper proposal, and return its native `<ds-appearance>` card in the same chat for the owner to apply. For a wallpaper draft with a one-time request ID, the destination General session can run:

```sh
domios wallpaper set --request REQUEST_ID --from-pad tile.svg
```

The request applies one sanitized tile to that machine, provided its wallpaper setting has stayed the same. Undo in the picker restores the previous wallpaper. To author a complete theme, continue to use `domios theme save`.

| Field | Values |
|---|---|
| `pattern` | `none` (default), `dots`, `grid`, `icon`, `svg`, or the bundled patterns `domios`, `doodles`, `space`, `math`, `botanical`, `retro`, `geometry`, `sketch`, and `circuits` |
| `scale` | Tile spacing in whole pixels, from 8 to 512. Defaults to 20 for dots and 360 for bundled artwork |
| `opacity` | Pattern opacity from 0 to 1. Dots use 0.035 in light themes and 0.028 in dark themes when chosen without an intensity. Domios displays pattern opacity up to 0.24 |
| `color` | `"#rrggbb"` color, derived from the theme foreground when omitted |
| `colors` | One to three `"#rrggbb"` stroke colors, used in place of `color` when supplied |
| `rotation` | Artwork or grid rotation from −360 to 360 degrees |
| `icon` | `cat` (default), `diamond`, or `star` when using `pattern = "icon"` |
| `svg` | Inline SVG string when using `pattern = "svg"`. Use a `viewBox`, static shapes or paths, and optional static transforms. SVG is limited to 64 KiB before and after sanitization and rejects scripts, styles, animation, external references and `url()` |
| `gradient` | Optional table with `kind = "linear"` or `"radial"`, 2–4 evenly spaced `stops` as `"#rrggbb"` strings, and optional `angle` (−360 to 360 degrees, default 135 for linear). Radial gradients are centered. |

The gradient sits beneath the pattern and is blended lightly with the theme background. Set `pattern = "none"` for a gradient alone, or omit the gradient too for a solid background.

```toml
[chat_background]
pattern = "icon"
icon = "cat"
scale = 48
opacity = 0.10
rotation = -12

[chat_background.gradient]
kind = "linear"
angle = 35
stops = ["#5359a8", "#419a80"]
```

Add the table to your custom theme file and run `domios theme save my-chat my-chat.toml --apply`. Invalid backdrop settings refuse the save with reason `invalid_chat_background` and a field-specific message.

To draw a custom cat wallpaper, author a square SVG with the cats at different positions within one tile. Use `fill="none"` and `stroke="currentColor"` so the theme controls the line color. Set `color` for a custom tint or omit it to follow the foreground in light and dark themes. Put the SVG into the TOML as a multiline literal and apply the file with `domios theme save cat-wallpaper cat-wallpaper.toml --apply`.

```toml
[chat_background]
pattern = "svg"
scale = 192
opacity = 0.10
svg = '''
<svg viewBox="0 0 192 192" fill="none" stroke="currentColor" stroke-width="2"
     stroke-linecap="round" stroke-linejoin="round">
  <g transform="translate(30 54) rotate(-16)">
    <path d="M-14 0v-13l10 7h8l10-7V0a14 14 0 1 1-28 0Z"/>
    <path d="M-5 3h.01M5 3h.01M-2 7l2 2 2-2"/>
  </g>
  <g transform="translate(132 127) rotate(18)">
    <path d="M-14 0v-13l10 7h8l10-7V0a14 14 0 1 1-28 0Z"/>
    <path d="M-5 3h.01M5 3h.01M-2 7l2 2 2-2"/>
  </g>
  <path d="M94 47v15m-7-7h14M63 136v11m-5-5h10"/>
</svg>
'''

[chat_background.gradient]
kind = "linear"
angle = 135
stops = ["#615eae", "#456d94"]
```

## Session Persistence

Desk state—workspaces, tabs, layout, and tab identity—is committed through the instance's Storage owner in `storage/state.db` beneath its data directory. Ordinary mutations are persisted through Storage; `POST /api/session/save` provides an explicit flush barrier before a forced stop. The Cron Jobs section below describes Storage unavailable mode and the backup/rollback protocol.

## CWD Tracking

Domios tracks the current working directory of each tab via invisible OSC 7 shell integration. No visible commands are injected into the terminal.

Generated integration files need no manual edits.

Shell integration scripts are stored in `shell-integration/` beneath the active instance data directory (`~/.domios/` for production, `~/.domios-dev/` for development) on every platform.

Domios creates these scripts at startup.

## Cron Jobs

Cron jobs and run history belong to the instance's typed Storage state in `storage/state.db` beneath its data directory, not `config.toml`. They are managed through the APIs in `domios docs API-INTEGRATIONS`, while the schema is documented here. Closing a workspace deletes its cron jobs, run history, and log directories. Storage opens before cron restore or scheduler start.

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
| `agent` | `string` | `"claude"` | Agent provider id (see `domios agent providers`) |
| `model` | `string?` | *(provider default)* | Model override within the provider (e.g. `"opus"`, `"haiku"`). Query valid values via `domios agent models <provider>`. |
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

### Late and missed occurrences

Cron history shows the original scheduled occurrence, the scheduler fire time, and the admission time. Manual runs have no scheduled occurrence. Runs admitted more than 5 seconds after their planned fire time, including any configured jitter, are marked late.

A pending recurring occurrence expires 10 minutes after its planned fire time. The scheduler records it as missed without launching an agent and advances to the next future occurrence. It processes at most one pending occurrence per job when resuming. One-shot jobs remain eligible after the window and carry the late marker when they fire.

Recurring failures count toward automatic disabling and leave the next occurrence on its configured cadence.

### Hidden spawn

Cron runs spawn agents in **hidden tabs** — no separate visible tab. Observe progress via the cron tabl in the sidebar, the frontend log viewer, or `domios cron history ID-or-name`. Use `cron_read_log` (Tauri IPC) / the live tab fallback to tail a running run's output.

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

Mem state lives in `~/.domios/mem/knowledge.db` (dev: `~/.domios-dev/mem/knowledge.db`). Config knobs that affect mem are under `[memory]` in `config.toml`.

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
| `memory.l1_budget_tokens` | `usize` | `300` | Token budget for the L1 critical-context layer in `domios mem wake-up` (and `GET /api/mem/wake-up` when no `?budget=` is passed). L0 identity always renders first (~100 tok); L1 fills the remainder. Pinned items beat the score-ranked fill but still respect this budget — when the pinned set alone overflows it, the rendered output starts with `<!-- WARN: N pinned items did not fit L1 budget … -->` and a `[wake-up] pinned-eviction` log line fires. |

```toml
[memory]
l1_budget_tokens = 300
```

### Dreamer schedule

The bundled `mem-dreaming-auto` cron runs the dreamer at `0 6 * * *` (6 AM local time) with a 0..30 min delay-only jitter window — so it fires between 6:00 and 6:30 AM. Schedule and jitter are not config keys. Custom schedules a user has set explicitly are left alone. The dreamer additionally runs **STAGE D — Memory calibration** on Sundays only (server-gated); the endpoints are documented in `domios docs API-MEMORY`. Every night it can also archive year-untouched items, only when `[memory.dream_hygiene] stale_archive = true` (default `false`); `stale_archive_cap` (default 20) and `merge_cap` (default 10) bound the nightly stale archives and Dreamer merges (`domios docs CONFIG`).

### Supporter Agent

The Supporter Agent runs memory re-rank and tab-state whenever its selected adapter is connected. Automatic plan reminders are the exception to deterministic fallback: without the connected adapter's approving choice none is sent (`domios docs ORCHESTRATION`). Memory search can force re-rank with `supporter=true|false`; adapter failures return plain recall with a `supporter.status = "fallback"` envelope. The shared `domios supporter status` and `/api/supporter/status` response reports the selected adapter and model, token status, `connected`, `last_activity`, `calls_last_hour`, `calls_last_day`, `last_verdict`, and `budget` (reached, calls_per_hour, free_at). `spend` holds today/month token and estimated USD totals, exhaustion and next probe time; prices are adapter-declared, never assumed for unpriced adapters. The token is set in Settings > Agents > Supporter Agent or through bearer-only `/api/supporter/token`, and is stored under `<data dir>/supporter/`, never in `config.toml`.

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

Saved membership is the union of the connections table, the active adapter and adapters with stored tokens. Switching keeps other tokens and models. `domios supporter disconnect <adapter>` removes that connection and token, preserving usage history; removing the active connection stops the Supporter without selecting another.

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

There is no per-session config: each provider owns where its sessions live. The `domios sessions` commands and `/api/sessions/*` find and resume them; `domios docs API-STATE` has the contract.

<!-- revision: 39 -->
