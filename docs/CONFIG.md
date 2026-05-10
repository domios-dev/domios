# CodeTerm Configuration

Config file: `%APPDATA%/codeterm/config.toml` (Windows) or `~/.config/codeterm/config.toml` (Linux/macOS).

Auto-created on first run with defaults.

## Full Reference

```toml
# CodeTerm Configuration

[theme]
name = "github-dark"
# Available: github-dark, github-light, dracula, catppuccin-mocha, one-dark, nord, solarized-light, pip-boy
# Override individual colors:
# background = "#0d1117"
# foreground = "#c9d1d9"
# cursor = "#58a6ff"

[font]
family = "JetBrains Mono, Cascadia Code, Fira Code, Consolas, monospace"
size = 14
ligatures = true

[terminal]
cursor_style = "block"  # block, underline, bar
cursor_blink = true
scrollback = 10000
copy_on_select = false

[notifications]
desktop = true
sound = false
toast_duration = 5  # seconds

[keybindings]
new_tab = "Ctrl+T"
close_tab = "Ctrl+W"
next_tab = "Ctrl+Tab"
prev_tab = "Ctrl+Shift+Tab"
split_right = "Ctrl+Shift+ArrowRight"
split_down = "Ctrl+Shift+ArrowDown"
close_pane = "Ctrl+Shift+Q"
focus_pane_left = "Alt+ArrowLeft"
focus_pane_right = "Alt+ArrowRight"
focus_pane_up = "Alt+ArrowUp"
focus_pane_down = "Alt+ArrowDown"
new_workspace = "Ctrl+Shift+N"
copy = "Ctrl+Shift+C"
paste = "Ctrl+Shift+V"
search = "Ctrl+Shift+F"
zoom_in = "Ctrl+="
zoom_out = "Ctrl+-"
zoom_reset = "Ctrl+0"
open_browser = "Ctrl+Shift+B"
open_settings = "Ctrl+,"

[socket]
# Auto-generated, override if needed:
# path = "\\\\.\\pipe\\codeterm-custom"

[agent]
# Agent config controls AI agent integration (e.g. Claude Code).
# poll_interval_ms: how often the agent polls for new commands.
# idle_timeout_s: seconds of idle before treating agent as idle (used for output streaming).
# resume_sessions: auto-resume agent sessions on restart. When the JSONL referenced
#                  by the saved session id is missing on disk (`AgentProvider::session_exists`
#                  returns false), CodeTerm clears the stale id and launches fresh
#                  instead of looping on "No conversation found".
# resume_skip_permissions: add --dangerously-skip-permissions to resumed sessions.
poll_interval_ms = 500
idle_timeout_s = 2
resume_sessions = true
resume_skip_permissions = true

[api]
enabled = true
host = "0.0.0.0"
port = 7685
# token = "your-secret-token"  # Uncomment to require auth
cors = true

[general_agent]
enabled = true
agent_provider = "claude"       # Agent command to use
auto_restart = true             # Auto-restart on crash
max_restart_attempts = 3
restart_delay_s = 60

[integrations]
telegram_enabled = false
telegram_allowed_users = []     # Telegram user IDs (strings), empty = allow all
discord_enabled = false
discord_allowed_users = []
webhook_enabled = false
webhook_port = 7687

[integrations.secrets]
backend = "local"                # "local" (default), "bitwarden", or "mock" (dev only)
default_organization = ""        # Default org id for new items (Bitwarden); empty = personal vault
default_collection = ""          # Default collection id for new items (Bitwarden)

[integrations.secrets.bitwarden]
server_url = "https://vault.bitwarden.com"   # Public vault, or self-hosted Vaultwarden URL
persist_master_password = false              # Save master password in OS keychain on unlock (auto-unlock after restart). Off by default.
```

## Config API

Read and write config values at runtime via REST or CLI.

**REST:**
```bash
# Read full config
curl http://host:7685/api/config

# Read a specific key (dot-notation)
curl "http://host:7685/api/config?key=integrations.telegram_enabled"

# Set a value
curl -X PATCH http://host:7685/api/config \
  -H "Content-Type: application/json" \
  -d '{"key": "integrations.telegram_enabled", "value": true}'
```

**CLI:**
```bash
codeterm config get integrations.telegram_enabled
codeterm config set integrations.telegram_enabled true
codeterm config get general_agent              # read an entire section
```

Values are JSON-compatible: `true`, `false`, `42`, `"string"`, `["array"]`. Changes are saved to disk immediately and trigger a `config-changed` event.

## General Agent

The General Agent is an always-on Claude Code instance that lives in the "General" workspace. It handles Telegram messages, cron tasks, and acts as the default orchestrator.

```toml
[general_agent]
enabled = true                  # Enable the General Agent
agent_provider = "claude"       # Agent command (default: claude)
auto_restart = true             # Auto-restart if agent crashes
max_restart_attempts = 3        # Max restart attempts before giving up
restart_delay_s = 60            # Seconds between restart attempts
```

When enabled, CodeTerm starts with a single "General" workspace instead of the default "Main" workspace. The General Agent pane is marked as protected (cannot be closed via UI).

Settings UI: Settings > Agents > General Agent section (enable, auto-restart toggles).

## Integrations

External service integrations are configured under `[integrations]`.

### Telegram Bridge

Connect a Telegram bot to the General Agent. Messages are routed bidirectionally:
- **Inbound** (Telegram to Agent): Messages arrive as `[Telegram: user_id] text` in the PTY. Rapid messages within 300ms are debounced into a single delivery. A typing indicator is sent on every inbound message.
- **Outbound** (Agent to Telegram): The agent sends replies via CLI: `codeterm bridge send --to USER_ID --service telegram "text"`. Without `--append`, always sends a new message. Use `--append` to concatenate text to the previous message (auto-inserts `\n\n` paragraph break, edits in-place within 60s window). Works from any pane, not just the General Agent.

```toml
[integrations]
telegram_enabled = false
telegram_allowed_users = []     # Telegram user IDs as strings, empty = allow all
```

**Setup:**
1. Create a bot via @BotFather on Telegram, get the token
2. Save token to `~/.codeterm/secrets/telegram.token` (Windows: `%APPDATA%/codeterm/secrets/telegram.token`)
3. Enable: `codeterm config set integrations.telegram_enabled true`
4. Optionally restrict users: `codeterm config set integrations.telegram_allowed_users '["123456789"]'`
5. Restart CodeTerm (hot-reload not yet supported)

Connected services are shown as icons next to the pane title in the sidebar (`connected_services` field on the pane object).

Settings UI: Settings > Integrations > Telegram (toggle + allowed users list).

### Discord (coming soon)

```toml
discord_enabled = false
discord_allowed_users = []
```

### Webhooks (integration bridge)

```toml
webhook_enabled = false
webhook_port = 7687
```

### Pre-forward text hooks (since v1.2.0)

Generic intercept point for inbound bridge messages BEFORE they reach the General Agent pane. An external daemon receives the message via HTTP and decides one of three outcomes: forward unchanged, replace (daemon already responded — bridge skips the pane), or annotate with metadata that the agent can act on.

Same shape as `voice_transcribe` (health-cache + timeout + fallback) but routed for text/command messages.

```toml
[integrations.text_hooks]
enabled = false
daemon_url = "http://127.0.0.1:7892"
timeout_ms = 2000
health_check_ttl_secs = 60
fallback_on_error = "forward"      # "forward" | "drop"
```

**Daemon protocol:**

```
GET  /healthz                      → 200 "ok"

POST /pre_forward
Body: {
  "text":         "включи свет на 1 этаже",
  "channel":      "telegram",
  "user_id":      "186809246",
  "msg_id":       "12345",
  "pane_id":      "<target pane>",
  "attachments":  [{"kind": "voice", "path": "/tmp/...", "filename": "..."}]
}

Response (one of):
  {"action": "forward"}                            // standard pass-through
  {"action": "replace",
   "respond": [
     {"channel": "telegram", "to": "186809246",
      "text": "🦝 включил свет на 1 этаже"}
   ]}                                              // bridge sends reply, skips GA
  {"action": "forward_with_metadata",
   "metadata": {"router_label": "light_cmd",
                "extracted_action": "ha.mjs floor 1 on"}}
                                                   // GA gets text + [Hook metadata: …] line
```

**Use cases:**

- **Trivial command shortcut** — regex first, then Groq llama-3.1-8b (~65ms): 80%+ of TG commands resolved in <100ms instead of 2-5s Claude latency, no token burn.
- **FAQ/auto-reply** — embedding match against curated answers + cached weather/time API.
- **Rate-limit / debounce** — drop duplicates within an N-second window without touching agent prompts.
- **Channel routing** — annotate mesh-relay pings as routine in metadata so GA logs them and moves on.

**Safety / guarantees:**

- Bridge does NOT execute user code. Daemon is a separate process the user runs (parallel to `voice-transcribe`); bridge just talks HTTP.
- Health-cache TTL prevents pummeling a known-down daemon during inbound bursts.
- `fallback_on_error = "forward"` (default) means the message reaches GA when the hook is unreachable, broken, or times out — fail-open. `"drop"` is fail-closed (silence on hook error).
- For `replace`, only `channel: "telegram"` is wired today. Mesh / Discord routes will be added when those bridges land replies.
- The hook only sees `service`-prefixed messages — relayed mesh /send and webhook events bypass it for now (they take a different inject path).

**Setup:**

1. Run a daemon listening on `daemon_url` that handles `GET /healthz` and `POST /pre_forward`.
2. `codeterm config set integrations.text_hooks.enabled true`
3. `codeterm config set integrations.text_hooks.daemon_url http://127.0.0.1:7892` (or wherever your daemon listens)
4. Restart bridge (`codeterm bridge restart`) — picks up the new config.

### Secrets

Pluggable secret store for the `codeterm mem secret` command family and the Settings → Secrets UI. Configure the active backend, (for Bitwarden) the default org/collection that new items land in, and (for Bitwarden) the server URL plus optional auto-unlock.

```toml
[integrations.secrets]
backend = "local"                # "local" (default), "bitwarden", or "mock"
default_organization = ""        # Default org id for new items (Bitwarden); empty = personal vault
default_collection = ""          # Default collection id for new items (Bitwarden)

[integrations.secrets.bitwarden]
server_url = "https://vault.bitwarden.com"   # Public vault, or self-hosted Vaultwarden URL
persist_master_password = false              # Auto-unlock after daemon restart by saving the master password in the keychain alongside the session token. Off by default.
```

Backends:
- `local` (default) — one entry per secret in the OS keychain (macOS Keychain / libsecret / Windows Credential Manager) under service `codeterm-secrets`. A non-secret index of names + ids + folders + timestamps lives at `~/.codeterm/secrets/local-index.json`; values are never written there. Offline, no external service.
- `bitwarden` — shells out to the official `bw` CLI. Install with `brew install bitwarden-cli` (macOS), `npm i -g @bitwarden/cli` (Linux/cross-platform), or `scoop install bitwarden-cli` (Windows). When `bw` is missing the Settings → Secrets page shows the install hint, an "Install Bitwarden CLI" button, and the CLI returns exit code 6 (backend unavailable). Supports `vault.bitwarden.com` and self-hosted Vaultwarden via `server_url`.
- `mock` — in-memory only. Anything you save is lost on daemon restart. Use for dev / screenshots / tests.

The unlocked Bitwarden session token is stored in the keychain under service `codeterm.secrets.bitwarden` (account `session`). With `persist_master_password = true`, the master password lives in the same keychain (account `master_password`) so the daemon can auto-unlock after a restart — opt-in trade-off because that's one more secret on the same disk surface as the session. In dev builds the "keychain" is a `0600` JSON file at `~/.codeterm-dev/keychain/<service>.json` (signed-binary-changes-on-rebuild make the OS keychain unusable); release builds use the real OS keychain. On Linux without libsecret the keychain probe fails and the backend reports `Unavailable` — we never silently fall back to plaintext on disk in production builds.

**Secrets do not cross the mesh.** `codeterm --mesh PEER mem secret …` exits with code 7 and a clear error. The vault is local to the machine that holds the keychain.

Full reference: `docs/SECRETS.md` (CLI, storage internals, error mapping, plugin guide for adding new backends).

Settings UI: Settings > Secrets — backend picker, server URL + Apply (Bitwarden), Install Bitwarden CLI button (when `bw` missing), login (password or API key), Remember-master-password toggle, default scope, recently saved items, lock / sign-out.

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

Contains workspace layout, tab arrangement, and split tree structure.

## CWD Tracking

CodeTerm tracks the current working directory of each pane via invisible OSC 7 shell integration. No visible commands are injected into the terminal.

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

Cron jobs live in `~/.codeterm/crons.json` (dev: `~/.codeterm-dev/crons.json`), not in `config.toml`. They are managed via the REST/CLI/Socket APIs (see `docs/API.md`), but the schema is documented here.

### `CronJob` schema

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `id` | `string` | — | Stable job id (8-char hex) |
| `name` | `string` | — | Display name |
| `description` | `string?` | — | Optional free-text description |
| `workspace_id` | `string` | — | Owning workspace |
| `schedule` | `string` | — | 5-field cron (`min hour dom mon dow`) or oneshot expression (`"in 5m"`, ISO 8601, unix ms) |
| `enabled` | `bool` | `true` | Whether the scheduler fires this job |
| `agent` | `string` | `"claude"` | Agent provider id (see `codeterm agent providers`) |
| `model` | `string?` | *(provider default)* | Model override within the provider (e.g. `"opus"`, `"haiku"`). Query valid values via `codeterm agent models <provider>`. |
| `task` | `string` | — | Prompt sent to the agent via `-p` |
| `cwd` | `string?` | *(workspace cwd)* | Working directory for the spawned agent |
| `role` | `string?` | — | Optional hierarchy role to assign on spawn |
| `creator_pane_id` | `string?` | — | Pane that receives reports from the cron agent |
| `oneshot` | `bool` | `false` | One-shot job: auto-disabled after first fire, deleted on completion |
| `jitter_max_ms` | `u64?` | `null` | See jitter semantics below |
| `max_history` | `usize` | `100` | Per-job run history cap |
| `consecutive_failures` | `u32` | `0` | Runs failed in a row. Auto-disables at ≥ 5. |
| `protected` | `bool` | `false` | Protected jobs cannot be deleted, only toggled |
| `last_run` | `CronRunInfo?` | — | Last run summary (status, pane, timing) |
| `next_run` | `u64?` | — | Next scheduled fire (unix ms). `null` for disabled / exhausted oneshots. |

### Jitter semantics (`jitter_max_ms`)

- `null` (omitted) → **auto**: 10% of the inter-fire interval, clamped to `[1m, 30m]`. Default.
- `0` → **disabled**: jobs fire exactly on schedule.
- positive integer → hard cap in milliseconds of random delay added to each scheduled fire.

Jitter is applied on top of the cron/oneshot fire time. A job with `0 * * * *` and `jitter_max_ms=5000` fires between `:00.000` and `:05.000` each hour. Auto-mode exists so users on `*/5 * * * *` schedules don't all hammer LLM providers at the same minute boundary.

### Hidden spawn

Cron runs spawn agents in **hidden panes** — no visible tab or split. Observe progress via the cron panel in the sidebar, the frontend log viewer, or `codeterm cron history ID`. Use `cron_read_log` (Tauri IPC) / the live pane fallback to tail a running run's output.

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

Saving a mem item triggers an extraction cascade that pulls structured entities and relationships out of the body. Extraction runs in hidden panes spawned through `workers::spawn_headless`, and is capped at **4 concurrent workers** globally via `ServiceContext::extract_semaphore` (a `tokio::sync::Semaphore` created at startup). A fifth concurrent save queues until one of the running extractions releases its permit.

To suppress extraction on a specific save — used by seed loaders to avoid re-extracting bundled items on every boot — pass `skip_extract: true` on the `POST /api/mem/items` body (or via the matching socket method). Items tagged `auto-extracted` or sourced from `extracted_from:*` are also skipped automatically.

### Worker timeouts

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `memory.worker_timeout_seconds` | `u64` | `300` | Hard timeout for non-dreaming hidden agent runs (extraction workers, generic cron). The auto-complete watcher marks the run `Failed` if the agent has not emitted a stream-json result event within this many seconds. Was `400` before v1.2.0; lowered because real-world extractions complete well under 5 min and the longer wait was just delaying retries on stuck workers. |
| `memory.dreaming_timeout_seconds` | `u64` | `1800` | Hard timeout for the Dreamer (REM phase) specifically. Hub decomposition + diary + classification in one session needs room to think. Lower if your dreaming model is fast. |

### Wake-up budget

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `memory.l1_budget_tokens` | `usize` | `800` | Token budget for the L1 critical-context layer in `codeterm mem wake-up` (and `GET /api/mem/wake-up` when no `?budget=` is passed). L0 identity always renders first (~100 tok); L1 fills the remainder. Pinned items beat the score-ranked fill but still respect this budget — when the pinned set alone overflows it, the rendered output starts with `<!-- WARN: N pinned items did not fit L1 budget … -->` and a `[wake-up] pinned-eviction` log line fires. |

```toml
[memory]
l1_budget_tokens = 800
```

### Dreamer schedule

The bundled `mem-dreaming-auto` cron runs the dreamer at `0 6 * * *` (6 AM local time) with a `±30 min` jitter window — so it fires anywhere in 5:30–6:30 AM. Schedule and jitter are managed by `ensure_dreaming_cron` (`src-tauri/src/mem/dreaming.rs`) and not exposed as config keys. Legacy installs that still hold the v1.1 `0 3 * * *` schedule with `±90 min` jitter are auto-migrated to the new values on the next daemon startup. Custom schedules a user has set explicitly are left alone. The dreamer additionally runs **STAGE D — Memory calibration** on Sundays only (server-gated via `strftime('%w','now') == '0'`); see `POST /api/mem/calibration/sweep` and `GET /api/mem/calibration/log` in `docs/API.md`.

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

Search and resume from the CLI with `codeterm sessions search|get|resume`. REST endpoints under `/api/sessions/*` mirror the CLI. See `docs/API.md` for the full contract and `docs/plans/session-tracking.md` for the architecture.

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

<!-- revision: 18 -->
