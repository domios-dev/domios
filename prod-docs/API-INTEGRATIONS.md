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
| socket | `bridge.thread.open` | `{caller_tab_id, tab_id?, service?, chat_id?}` | Open or reuse a tab's Telegram topic; returns `{tab_id, chat_id, thread_id, topic_name, link, created}`; `link` is null unless the chat kind has a topic link. A foreign target from a non-General caller fails with `-32003`. Socket `bridge.send` with `caller_tab_id` routes through that tab's topic. Contract: `TELEGRAM-SETUP` |

**Concurrency.** Start, stop, and the boot-time spawn share one operation guard, so start/stop on one daemon never race.

**GET /api/bridge/state** — runtime and config snapshot. `?service` defaults to `telegram`; an unsupported value returns `400`. `has_secret_in_vault` is metadata only; the value is never decrypted.

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

An idle supervisor persists `telegram_enabled=false` and returns `200 {"ok": true, "released_in_ms": 0, "was_active": false}`. An active one is signalled and awaited up to `timeout_ms`: success returns `200` with `was_active: true` and only then flips the config; a timeout returns `408` (`bridge stop timed out`) with the config untouched.

**POST /api/bridge/announce** — one-shot text (HTML parse mode) through the running bridge, used by `bridge migrate --announce`. Body `{service? = "telegram", chat_id (i64, required), text (required)}`. Returns `200 {"ok": true}`, or `503` (`Bridge not active`) while the bridge has no outbound channel.

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

**Behavior.** Without `--append` every send is a new message. `--append` joins the text to the previous message with a paragraph break and edits it in place while that message is under 60 seconds old; otherwise it sends a new one. Text over Telegram's 4096-character limit is split on UTF-8 character boundaries. Sending works from any tab. Each inbound message shows a typing indicator, and inbound messages within 300 ms merge into one delivery. A dead polling service restarts after a 5-second backoff; the manager never exits.

Returns `{"ok": true}`, or `503` when the bridge is not active.

#### Inbound webhooks (Pebble)
| Method | Path | Body | Description |
|--------|------|------|-------------|
| POST | `/api/plugins/pebble/webhook` | Multipart form or JSON, at most 1 MiB; an audio-capable receiver also accepts one audio part of at most 8 MiB | Hand an authenticated Pebble event to the plugin that registers a `webhookReceiver` of kind `pebble`; a delivery reaches that receiver's live bound tab, otherwise the General Agent |

**Authentication.** Only `Authorization: Bearer <secret>`, compared in constant time with the secret-store entry `pebble_webhook_secret` (see `codeterm docs SECRETS`). The route does not use the daemon API token, and no URL, path, or query token is accepted. Nothing reaches the plugin before authentication succeeds. Non-loopback callers additionally need mesh networking enabled.

**Plugin contract.** The receiver's synchronous `webhookReceive({payload, receivedAt, audio?})` runs on a blocking thread and returns `{text, eventId?, transcript?}`, `null`, or throws. `audio` is `{mediaType, size}` metadata only; the bytes never enter the plugin. `transcript` is the sender's own transcript, which the host renders labelled (see *Audio*).

**CoreApp format.** Pebble sends `multipart/form-data` with `transcription`, `recordedAt` (epoch ms), and `client`; test events add `test`, and a recording is an `audio` file part (`audio/mp4`). Send `recording + transcription` to an audio-capable receiver and the transcription only to a text-only one; configure only the Authorization header. Text parts reach the plugin as a JSON object; duplicate names and invalid UTF-8 are rejected; forms allow 16 parts and 64 KiB per text part; other file parts are skipped unsaved. Source: [Pebble webhook format](https://help.repebble.com/en/articles/15724406-index-advanced-features-mcp-webhook).

**Audio.** A receiver opts in with `capabilities.webhookReceiver.audio: true`;
the host branches only on that normalized flag. The route authenticates from the
headers before it reads the body, then reads at most 1 MiB, or 9 MiB for an
audio-capable receiver. Within that, the form accepts one `audio/*` file part of
at most 8 MiB; everything else in the form still counts against the 1 MiB text
limit, JSON bodies never get the audio allowance, and a second audio part is
`invalid_form_fields`. A text-only receiver skips audio parts as before. The
receiver runs first; only when it returns a delivery is the audio stored with
the ordinary upload rules (sanitized file name, the uploads directory's 7-day
and 512 MiB cleanup) and passed to the same voice stage as bridge voice notes,
using the active installed Transcriber. The delivered text is the receiver's
text followed by labelled lines:

| Situation | Lines after the receiver text |
|-----------|-------------------------------|
| No audio | `Transcript (sender): …` when the receiver returned a transcript |
| Host transcript preferred | `Transcript (CodeTerm): …`, then `For comparison, sender transcript: …` |
| Sender transcript preferred | `Transcript (sender): …`, then `For comparison, CodeTerm transcript: …` |
| No Transcriber, transcription error, or audio not storable | `Transcript (sender): …` when present, then `CodeTerm transcription failed: <reason>` |

Every audio case ends with `Audio file: <stored path>` once the file is stored.
The preference is mechanical, never model-judged: a transcript is degraded when
it has no words, contains a recognizer failure marker (`<unk>`, `[blank_audio]`,
`[inaudible]`, `(inaudible)`, `[silence]`, `(silence)`, `[music]`, `[no speech]`,
case-insensitive), or has fewer than a quarter of the other transcript's words
when the other has at least eight. The host transcript is primary unless it is
degraded and the sender transcript is not. Both transcripts are sanitized like
the event text; the host transcript is clipped to 64 KiB.

| Status | Body `status` / `error.code` | Meaning |
|--------|------------------------------|---------|
| `200` | `delivered` + `eventId` | The plugin returned a delivery and its text reached the bound tab or the General Agent |
| `200` | `noop` | The plugin returned `null`, or this daemon already delivered that `eventId`; nothing is delivered |
| `400` | `invalid_json` | Body is not valid JSON |
| `400` | `invalid_multipart` / `invalid_form_fields` | Broken boundary/form, duplicate or invalid field names, or invalid UTF-8 |
| `401` | `unauthorized` | Missing, malformed, or wrong bearer secret |
| `404` | `webhook_receiver_not_found` | No plugin registers a `pebble` receiver |
| `413` | `body_too_large` | Body exceeds 1 MiB (9 MiB for an audio-capable receiver), the audio part exceeds 8 MiB, or the non-audio form exceeds 1 MiB |
| `415` | `unsupported_media_type` | Missing or unsupported Content-Type; use multipart/form-data, application/json or application/*+json |
| `500` | `webhook_internal_error` | The receiver task did not complete |
| `502` | `webhook_receiver_failed` | The plugin threw, returned a malformed delivery, text or transcript over 64 KiB, text empty after sanitizing, or an `eventId` over 256 bytes |
| `502` | `delivery_failed` | Delivery to the bound tab or the General Agent failed; it is never retried into another tab |
| `503` | `authentication_unavailable` | `pebble_webhook_secret` is missing, empty, or the vault is locked or logged out |
| `503` | `webhook_receiver_ambiguous` | More than one plugin registers a `pebble` receiver |
| `503` | `general_agent_unavailable` | The receiver has no live bound tab and no General Agent tab exists |

Success: `{"ok": true, "status": "delivered"}` (`eventId` only when supplied by the receiver). Failure: `{"ok": false, "error": {"code": "...", "message": "..."}}`. Handler rejections, including 400/415, log status and code under `[pebble-webhook]` without the secret, headers, or payload.

**Delivery.** Before delivery the text loses ANSI and control characters (newline and tab stay) and every embedded `-=-codeterm:` marker opening. The message gateway delivers it to the receiver's live bound tab (see *Receiver targets* below), otherwise to the General Agent, as `-=-codeterm:pebble source=Pebble plugin=<plugin-id>-=-` followed by the text on its own line, with no reply address. The stored chat-origin metadata carries the receiver id and its host-sanitized plugin SVG; the marker carries identity only. Optional event ids are remembered in memory for the latest 512 deliveries of this daemon run; replaying an event whose delivery failed returns the same failure. CoreApp's unsigned form has no event id, so the plugin omits it and repeated requests are not deduplicated.

#### Receiver targets and plugin tab menus

`GET /api/plugins/tab-menu` returns `{items: [{pluginId, itemId, label, action, iconSvg?, iconColor?}], receiverTargets: {<pluginId>: <tabId>}}` for every manifest `tabMenu` item; `iconSvg` is the host-sanitized SVG from the plugin bundle, or absent when the item and plugin have no accepted asset, and `iconColor` is the plugin's validated manifest color. `POST /api/plugins/tab-menu/action` `{pluginId, itemId, tabId}` runs the declared host-owned action on a local tab (`receiverTarget` binds the tab, or clears it when that tab is already bound). `GET|PUT {tabId}|DELETE /api/plugins/{id}/receiver-target` reads, binds (id or alias, replacing any previous binding) or clears one tab and returns `{pluginId, tabId, live}`; `tabId: null` or `live: false` means events take the General Agent path.

The host owns one binding per receiver plugin id, persisted in the data directory and restored on restart. Closing the bound tab clears it. `live` is true only while the tab exists and its PTY or chat-backend session is open; the webhook route reads the same observation. Set and get require a plugin with an active `webhookReceiver`; clear does not. Errors are `{"ok": false, "error": {"code", "message"}}`: `404` `receiver_not_found`, `tab_not_found` (including a peer's tab), or `menu_item_not_found`; `500` `persist_failed` (nothing changed). Changes publish `contributions-changed` with kind `receiver_target`; plugin loads that add or remove `tabMenu` items publish kind `tab_menu`. Socket: `plugin.receiver_target.get|set|clear` with `{plugin_id, tab_id?}` returns the same view; refusals carry `data.code`. CLI: `codeterm plugin receiver-target <id> [--set <tab> | --clear]`. Plugins read or clear only their own binding through their host API; see the [core essence](../essences/core.md#plugin-host).

#### Setup (Rules, Hooks)
| Method | Path | Body | Description |
|--------|------|------|-------------|
| GET | `/api/setup/rules` | — | Check if project rules are installed |
| POST | `/api/setup/rules/install` | — | Install project rules |
| GET | `/api/setup/hooks` | `?provider=<id>` | Read provider integration target/status (`codeterm integration status --provider <id>`). Codex reports `repair_requires_consent: true` when its owned SessionStart/Stop contract is incomplete. |
| POST | `/api/setup/hooks/install` | `?provider=<id>[&precompact_memory_export=<bool>]` | Authenticated install is consent. Omit `precompact_memory_export` to preserve the current CodeTerm PreCompact hook or recorded choice; pass `true` or `false` to change it. The install records consent and its timestamp like Settings, installs CodeTerm's marked entries and grants their provider trust (foreign entries untouched). Returns `{ok, provider, status}`; `404` unknown provider. CLI: `codeterm integration install --provider <id>` (works with `--mesh PEER`). |
| POST | `/api/setup/hooks/uninstall` | `?provider=<id>` | Remove provider-owned integration entries while preserving foreign configuration |
| POST | `/api/integration/sweep-profiles` | `?provider=<id>&confirm=<bool>&include_legacy=<bool>` | Preview this instance's dead provider launch artifacts and unmarked legacy files. Both flags default false; deletion requires `confirm=true`. `include_legacy=true` with confirmation deletes every unmarked file regardless of tab liveness. CLI: `codeterm integration sweep-profiles --provider <id>`. |

With consent recorded, daemon start and provider plugin reload reconcile a missing, stale, or untrusted (`untrusted_hash`) integration by reinstalling with the trust grant. An existing CodeTerm-owned install with no consent record is treated as consent, recorded once with its timestamp, and reconciled. When neither recorded consent nor a CodeTerm ownership marker exists, nothing is rewritten or trusted.

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

Recurring schedules must contain exactly five fields and have a fire time within the next five years. Impossible dates and schedules with no fire in that horizon are rejected. Invalid `agent`, `model`, and `cwd` values are rejected when a job is created or updated.

**Oneshot jobs:** Set `oneshot: true` to create a one-time scheduled task. Oneshot jobs accept relative time (`"in 5m"`, `"in 1h30m"`, `"in 2d"`), ISO 8601 (`"2026-04-05T14:30:00Z"`), or unix milliseconds instead of cron expressions. After firing, oneshot jobs are disabled (`next_run=null`) and auto-deleted on completion. The `CronJob` object includes `oneshot: bool` (default `false`, omitted from JSON when `false`).

**Fields.** `model` overrides the provider's default model (valid ids from `/api/agents/providers`). `jitter_max_ms`: omitted = 10% of the interval up to 30 min, `0` = exact, positive = requested cap in ms, limited to half the schedule interval. `tz`: IANA zone, omitted = UTC, invalid zones rejected, DST handled. `timeout_seconds` overrides `memory.worker_timeout_seconds` for this job; `0` on update clears it.

**`CronRunStatus` enum:** `Running`, `Success`, `Failed`, `Cancelled`. `Cancelled` is set by `POST /api/crons/:id/cancel` on any runs currently in `Running` state.

**Hidden spawn:** cron runs no longer create visible tabs. Each run spawns a **hidden tab** (no UI surface); observe progress via the cron tabl / `cron_read_log`, not as a regular tab. When done, the agent calls `codeterm cron complete --run-id UUID` (socket method `cron.complete`). Logs are saved to `~/.codeterm/crons/<job_id>/<run_id>.log` with SUMMARY and RAW OUTPUT sections. Successful spawns do not generate notifications — only errors do.

**Workspace and Team resolution:** The CLI `cron create` command resolves the workspace the same way `codeterm workspace current` does — the calling tab's own workspace, then a validated `$CODETERM_WORKSPACE_ID`, then the focused workspace. A live Team caller's job inherits that Team unless `--no-team` is explicit. Override placement with `--workspace ID` or the membership-checked `--team ID`. Closing a workspace deletes its cron jobs, run history, and log directories. **Updating cron jobs:** Use `PUT /api/crons/:id` (CLI: `codeterm cron edit ID`) to modify a cron job in-place. Do NOT delete and recreate -- that loses run history and changes the ID. **Deleting a cron job** cascades: kills all running tabs/PTYs for that job, closes their tabs, and removes the entire log directory (`~/.codeterm/crons/<job_id>/`).

**Tauri events:** `cron-state-changed` on any cron CRUD operation; `cron-fired` when a cron fires, payload `{cron_id, run_id, tab_id, tab_id}`.

## Permission auto-confirm

| Method | Path | Body | Description |
|--------|------|------|-------------|
| POST | `/api/tabs/{id}/permission-auto-confirm` | `{enabled}` | Opt the tab in/out of auto-confirming provider permission prompts the provider positively recognizes (`permissionPromptResponse`) and a hook PermissionRequest backs. Persisted in the session snapshot; journaled. Returns `{ok, tab_id, enabled}`. CLI: `codeterm tab auto-confirm on\|off`. |
| GET | `/api/tabs/{id}/permission-auto-confirm` | — | `{tab_id, enabled, recent}` — `recent` holds the last auto-confirmations (`tool_name`, `option_label`, `rule`, `question`, `options`, `prompt_fingerprint`, `session_id`, `confirmed_at_ms`). CLI: `codeterm tab auto-confirm status`. |

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

## Related documents

Service registration, health, usage, logs, and activity: `codeterm docs API-SERVICES`. Notifications and webhooks: `codeterm docs API-TABS`. Secret storage and updates: `codeterm docs API-STATE`.

<!-- revision: 23 -->
