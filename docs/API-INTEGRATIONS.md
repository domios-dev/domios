# Integration API

REST endpoints for bridges, setup hooks, cron, and external integrations.

#### Bridge (Telegram / External Services)
| Method | Path | Body / Query | Description |
|--------|------|------|-------------|
| GET | `/api/bridge/state` | `?service=telegram` | Bridge runtime + config snapshot for live migration / health |
| POST | `/api/bridge/start` | `{service?, ensure_token?, wait_for_lock_ms?}` | Start the messaging supervisor; optionally pull token from vault and wait for the upstream lock |
| POST | `/api/bridge/stop` | `{service?, timeout_ms?}` | Stop the messaging supervisor and persist `telegram_enabled=false` |
| POST | `/api/bridge/announce` | `{service?, chat_id, text}` | One-shot outbound text via the running bridge (used by `bridge migrate --announce`) |
| POST | `/api/bridge/send` | `{text, chat_id?, service?, append?, type?, file?, caption?, filename?}` | Send message or media to external service (Telegram) |

**Concurrency contract.** `bridge_start`, `bridge_stop`, and the boot-time spawn all acquire the same `BridgeSupervisor::op_guard()` (tokio Mutex), so start/stop operations on the same daemon are serialized — no two of them ever race.

**GET /api/bridge/state** — current bridge runtime + config metadata. The `?service` query defaults to `telegram`; an unsupported value returns `400`. The `has_secret_in_vault` field is metadata-only via `SecretStore::has` and never decrypts the value.

```json
{
  "service": "telegram",
  "active": true,
  "config_enabled": true,
  "token_present": true,
  "has_secret_in_vault": true,
  "secret_name": "telegram_bot_token",
  "started_at": "2026-05-06T12:00:00Z",
  "last_poll_ok": "2026-05-06T12:01:30Z",
  "last_poll_error": null,
  "error_count": 0,
  "instance_name": "workstation"
}
```

| Field | Type | Description |
|-------|------|-------------|
| `service` | `string` | Service identifier (`"telegram"` today). |
| `active` | `bool` | Supervisor is running and the poller task is alive. |
| `config_enabled` | `bool` | `integrations.telegram_enabled` from `config.toml`. |
| `token_present` | `bool` | `~/.codeterm/secrets/telegram.token` exists on disk. |
| `has_secret_in_vault` | `bool` | The local secret store reports an entry named `secret_name`. Metadata only — value never decrypted. |
| `secret_name` | `string` | Vault key the bridge would resolve to (`"telegram_bot_token"`). |
| `started_at` | `string?` | RFC3339 timestamp of the last successful start. |
| `last_poll_ok` | `string?` | RFC3339 timestamp of the most recent successful Telegram `getUpdates`. |
| `last_poll_error` | `string?` | Last polling error message; `null` while healthy. |
| `error_count` | `int` | Lifetime polling-error counter (resets on supervisor restart). |
| `instance_name` | `string` | Local mesh `instance_name` for cross-host disambiguation. |

**POST /api/bridge/start** — start the supervisor. Body:

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `service` | `string` | `"telegram"` | Service to start. `400` if unsupported. |
| `ensure_token` | `bool` | `false` | If `true` and the local token file is missing, the daemon pulls `telegram_bot_token` from the local secret vault and writes it to `~/.codeterm/secrets/telegram.token` (mode `0600` on Unix). Returns `412` if the secret is not in the vault. |
| `wait_for_lock_ms` | `u64` | `10000` | How long to wait (ms) for the supervisor's `last_poll_ok` to advance past `started_at`, confirming the Telegram long-poll lock was claimed (no other poller fighting for it). |

Side effect: persists `integrations.telegram_enabled = true` to `config.toml` (so the supervisor auto-starts on next boot). Returns `200` once `last_poll_ok >= started_at`:
```json
{"ok": true, "started_in_ms": 1430, "claimed_lock": true}
```

If a polling error containing `409` is observed (Telegram refuses the lock — another poller still active), responds `409 Conflict` with the upstream error embedded. If the wait window expires without a successful poll, returns `200` with `ok: false, claimed_lock: false` and an `error` field carrying the most recent poll error (or `"no successful poll within wait window"`).

**POST /api/bridge/stop** — stop the supervisor and persist `telegram_enabled=false`. Body:

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `service` | `string` | `"telegram"` | Service to stop. `400` if unsupported. |
| `timeout_ms` | `u64` | `30000` | How long to wait for the poller task to exit gracefully. |

Behaviour:
- If the supervisor is already idle, persists `telegram_enabled=false` (idempotent) and returns `200`:
  ```json
  {"ok": true, "released_in_ms": 0, "was_active": false}
  ```
- If active, signals the supervisor and waits up to `timeout_ms`. On success returns `200` with `was_active: true`. The config flip to `telegram_enabled=false` happens **only after** the supervisor stop succeeds, so timeouts never leave config disagreeing with runtime.
- On timeout returns `408 Request Timeout` (`bridge stop timed out`); config is left untouched.

**POST /api/bridge/announce** — enqueue a one-shot text message via the existing outbound channel. Used by `codeterm bridge migrate --announce` to post a "now serving from <host>" sentinel after a successful migration. Body:

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `service` | `string` | `"telegram"` | Service to announce on. `400` if unsupported. |
| `chat_id` | `i64` | *(required)* | Target Telegram chat/user id. |
| `text` | `string` | *(required)* | Message text (HTML parse mode). |

Returns `200 {"ok": true}` on success or `503 Service Unavailable` (`Bridge not active`) if the local bridge has no outbound channel yet.

**POST /api/bridge/send** — sends an outbound message or media through the bridge. Parameters:

| Param | Type | Default | Description |
|-------|------|---------|-------------|
| `text` | `string` | *(required for text)* | Message text (HTML parse mode). Required when `type` is `text` or omitted. Used as caption fallback for media types if `caption` is not set. |
| `chat_id` | `number?` | last inbound | Telegram user/chat ID. Defaults to the last inbound sender. |
| `service` | `string?` | `"telegram"` | Target service (currently only `telegram`) |
| `append` | `bool` | `false` | Append text to previous message (edits via `editMessageText`) instead of sending new |
| `type` | `string?` | `"text"` | Message type: `text`, `photo`, `document`, `voice`, `animation`, `video`, `sticker` |
| `file` | `string?` | — | Absolute path to the file to send. Required for all media types. |
| `caption` | `string?` | — | Caption for media messages. If not set, positional `text` is used as caption. Stickers do not support captions. |
| `filename` | `string?` | — | Custom filename for `document` type (overrides the original filename). |

**Behavior:** `bridge send` (without `--append`) always sends a **new** message — no auto-edit. With `--append`, text is concatenated to the previous message (with an automatic `\n\n` paragraph break) and edited in-place, provided the previous message was sent within the last 60 seconds. If the 60-second window has expired, `--append` sends a new message instead.

**Typing indicator:** On every inbound Telegram message, CodeTerm sends a `sendChatAction("typing")` to show the typing bubble.

**Inbound debounce:** Rapid-fire Telegram messages within a 300ms window are merged into a single delivery to the agent PTY.

**UTF-8 safe chunking:** Messages exceeding Telegram's 4096-character limit are split into chunks. The chunker uses `floor_char_boundary()` to avoid slicing multi-byte UTF-8 characters (Cyrillic, emoji, CJK), preventing panics on non-ASCII text.

**Any-tab sending:** `bridge send` works from any tab, not just the General Agent tab. The CLI routes through the socket API without tab restrictions.

**Service resilience:** If the Telegram polling service dies (network error, API failure), the bridge manager automatically restarts it after a 5-second backoff. The manager never exits — it loops indefinitely, logging panics instead of dying silently.

Returns `{"ok": true}` on success. Returns `503 Service Unavailable` if bridge is not active.

#### Setup (Rules, Hooks)
| Method | Path | Body | Description |
|--------|------|------|-------------|
| GET | `/api/setup/rules` | — | Check if project rules are installed |
| POST | `/api/setup/rules/install` | — | Install project rules |
| GET | `/api/setup/hooks` | `?provider=<id>` | Read provider integration target/status (`codeterm integration status --provider <id>`). Codex reports `repair_requires_consent: true` when its owned SessionStart/Stop contract is incomplete. |
| POST | `/api/setup/hooks/install` | `?provider=<id>&precompact_memory_export=<bool>` | Authenticated install is consent: records `integration_consent_granted`, `integration_consent_recorded_at`, `integration_target` exactly like the Settings install, installs CodeTerm's own marked entries and grants their provider trust in the same step (foreign entries untouched). Returns `{ok, provider, status}`; `404` unknown provider. CLI: `codeterm integration install --provider <id>` (works with `--mesh PEER`). |

With consent recorded, daemon start and provider plugin reload reconcile a missing, stale, or untrusted (`untrusted_hash`) integration by reinstalling with the trust grant. Without recorded consent nothing is rewritten or trusted.
| POST | `/api/setup/hooks/uninstall` | `?provider=<id>` | Remove provider-owned integration entries while preserving foreign configuration |

#### Cron Jobs
| Method | Path | Body / Query | Description |
|--------|------|------|-------------|
| GET | `/api/crons` | `?workspace_id=ID` | List all cron jobs (optionally filter by workspace) |
| POST | `/api/crons` | `{name, schedule, task, workspace_id, agent?, model?, cwd?, description?, oneshot?, jitter_max_ms?, tz?, timeout_seconds?}` | Create cron job |
| GET | `/api/crons/:id` | — | Get cron job by ID |
| PUT | `/api/crons/:id` | `{name?, schedule?, task?, cwd?, description?, enabled?, agent?, model?, jitter_max_ms?, tz?, timeout_seconds?}` | Update cron job (prefer over delete+recreate). `tz: ""` clears back to UTC; `timeout_seconds: 0` clears the per-cron override. |
| DELETE | `/api/crons/:id` | — | Delete cron job |
| POST | `/api/crons/:id/enable` | — | Enable cron job |
| POST | `/api/crons/:id/disable` | — | Disable cron job |
| POST | `/api/crons/:id/trigger` | — | Manually trigger cron job |
| POST | `/api/crons/:id/cancel` | — | Stop any currently-running executions of this cron job. Returns `{cancelled: N}` — number of tabs closed. Does NOT delete the job; future scheduled fires continue normally. Running history entries flip status to `Cancelled`. |
| GET | `/api/crons/:id/history` | — | Get run history for cron job |

Schedule format: 5-field cron expressions (`min hour dom mon dow`). Examples: `*/30 * * * *` (every 30 min), `0 9 * * *` (daily 9am), `0 9 * * 1` (Mondays 9am).

**Oneshot jobs:** Set `oneshot: true` to create a one-time scheduled task. Oneshot jobs accept relative time (`"in 5m"`, `"in 1h30m"`, `"in 2d"`), ISO 8601 (`"2026-04-05T14:30:00Z"`), or unix milliseconds instead of cron expressions. After firing, oneshot jobs are disabled (`next_run=null`) and auto-deleted on completion. The `CronJob` object includes `oneshot: bool` (default `false`, omitted from JSON when `false`).

**`model`** — optional per-job override of the agent's model (e.g. `"opus"`, `"haiku"`, `"gpt-5.4-mini"`). Valid values depend on the `agent` provider; query `/api/agents/providers` for the list. Unset → use provider default.

**`jitter_max_ms`** — max random delay added to each scheduled fire. `null`/omitted → auto (10% of interval, clamped to `[1m, 30m]`). `0` → disabled (fires exactly on schedule). Positive → cap in milliseconds.

**`tz`** — IANA timezone string (e.g. `"Europe/Riga"`, `"UTC"`, `"America/New_York"`) the schedule is interpreted in. `null`/omitted → UTC. Invalid zones are rejected on create/update. DST transitions are handled by `chrono-tz`.

**`timeout_seconds`** — per-cron run timeout override. When set, this overrides the global `memory.worker_timeout_seconds` for runs of this job. `null`/omitted → fall back to global default. `0` (on update) → clear the override. Use a higher value for long-running tasks like memory dreaming (e.g. `1800` for 30 minutes).

**`CronRunStatus` enum:** `Running`, `Success`, `Failed`, `Cancelled`. `Cancelled` is set by `POST /api/crons/:id/cancel` on any runs currently in `Running` state.

**Hidden spawn:** cron runs no longer create visible tabs. Each run spawns a **hidden tab** (no UI surface); observe progress via the cron tabl / `cron_read_log`, not as a regular tab. When done, the agent calls `codeterm cron complete --run-id UUID` (socket method `cron.complete`). Logs are saved to `~/.codeterm/crons/<job_id>/<run_id>.log` with SUMMARY and RAW OUTPUT sections. Successful spawns do not generate notifications — only errors do.

**Workspace and Team resolution:** The CLI `cron create` command resolves the workspace the same way `codeterm workspace current` does — the calling tab's own workspace, then a validated `$CODETERM_WORKSPACE_ID`, then the focused workspace. A live Team caller's job inherits that Team unless `--no-team` is explicit. Override placement with `--workspace ID` or the membership-checked `--team ID`.

**Updating cron jobs:** Use `PUT /api/crons/:id` (CLI: `codeterm cron edit ID`) to modify a cron job in-place. Do NOT delete and recreate -- that loses run history and changes the ID.

**Deleting a cron job** cascades: kills all running tabs/PTYs for that job, closes their tabs, and removes the entire log directory (`~/.codeterm/crons/<job_id>/`).

**Tauri events:**
- `cron-state-changed` — emitted on any cron CRUD operation
- `cron-fired` — emitted when a cron fires, payload: `{cron_id, run_id, tab_id, tab_id}`

## Related documents

Service registration, health, usage, logs, and activity: `codeterm docs API-SERVICES`. Notifications and webhooks: `codeterm docs API-TABS`. Secret storage used by bridge startup: `codeterm docs API-STATE`.

#### Updates
| Method | Path | Body / Query | Description |
|--------|------|------|-------------|
| GET | `/api/update/status` | — | Current updater state and last update result |
#### Git
| Method | Path | Body / Query | Description |
|--------|------|------|-------------|
| GET | `/api/git/graph` | — | Commit graph for configured repositories |
| GET | `/api/git/commit/{sha}/files` | — | Files changed by a commit |
| GET | `/api/git/commit/{sha}/diff` | — | Diff for a commit |
| GET | `/api/git/workdir-diff` | — | Current working tree diff |
| GET | `/api/git/workdir-files` | — | Current working tree changed-file list |
| POST | `/api/git/revert-file` | `{path}` | Revert one file through the API guardrails |
| GET | `/api/git/repos` | — | Known repositories |

<!-- revision: 7 -->
