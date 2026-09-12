# State, observability, and update API

Secrets, sessions, web mirror, diagnostics, usage, logs, configuration, and update endpoints.

#### Secrets

Pluggable secret-store layer. Active backend is selected by `integrations.secrets.backend` in `config.toml`; built-in backends are `local` (OS keychain, default), `bitwarden` (Bitwarden CLI, optional Vaultwarden server), and `mock` (in-memory, dev only). All endpoints require the standard Bearer token. **Local-only by design** — secret routes are never relayed across the mesh; the CLI rejects `--mesh` on `mem secret` subcommands with exit code 7.

| Method | Path | Body / Query | Description |
|--------|------|------|-------------|
| GET  | `/api/secrets/status` | — | `{backend, status:{status: "logged_out"\|"locked"\|"unlocked"\|"unavailable", user?, transient?, endpoint?, reason?}, auto_unlock_capable, master_password_saved}`. `auto_unlock_capable` is the active backend's manifest declaration (`secretBackend.autoUnlock`); `master_password_saved` is true only when the capability is declared **and** the backend's own bucket holds its credential. See [SECRETS.md](SECRETS.md#the-auto-unlock-contract). |
| POST | `/api/secrets/unlock` | `{master_password, email?, two_factor_token?, api_key_client_id?, api_key_client_secret?, persist_for_auto_unlock?}` | Unlock the active backend. Master password is consumed and dropped immediately. `persist_for_auto_unlock` (default `false`) is the explicit consent to remember the credential for headless unlock; it is refused with `400 bad_request` when the active backend declares no auto-unlock capability. See [SECRETS.md](SECRETS.md#the-auto-unlock-contract). |
| POST | `/api/secrets/lock` | — | Drop in-process session; persisted creds (OS keychain) are kept. |
| POST | `/api/secrets/logout` | — | Drop session AND every persisted credential. |
| POST | `/api/secrets/auto-unlock` | — | Re-run the active backend's `init`, asking it to restore its own session from a credential it persisted itself. Returns `{ok: bool, backend, status}`; `ok=false` means the backend did not reach `unlocked`. No-op for a backend that declares no auto-unlock capability. |
| POST | `/api/secrets/sync` | — | Pull latest from the remote vault. Bitwarden runs `bw sync`. No-op for `local` and `mock`. |
| POST | `/api/secrets/backend` | `{backend, bitwarden_server_url?}` | Atomic backend swap. Persists the choice to `config.toml` immediately, runs `init()` on the new store, and replaces the active store. `bitwarden_server_url` is honored only when `backend == "bitwarden"`. Returns the new `{ok, backend, status}`. |
| GET  | `/api/secrets/items` | `?q=...&folder=...&organization=...&collection=...&limit=N` | List or search items. With `q` runs a search; without it lists everything matching the filter. Returns metadata only — never the value. |
| GET  | `/api/secrets/items/{name}` | — | Item metadata by name (or backend item id). |
| POST | `/api/secrets/items/{name}/value` | — | **Reveal endpoint.** Returns `{item_id, name, value, notes}`. POST not GET so the item name doesn't land in URL access logs. |
| POST | `/api/secrets/items` | `{name, value, folder?, organization?, collection?, notes?, overwrite?}` | Save a secret. `overwrite: false` (default) errors with HTTP 409 if `name` exists. When `organization` / `collection` are omitted, defaults from `integrations.secrets.default_organization` / `default_collection` are applied. |
| DELETE | `/api/secrets/items/{name}` | — | Delete by name. Permanent — no trash. |
| GET  | `/api/secrets/organizations` | — | List Bitwarden organizations the signed-in user belongs to. Empty for `local` / `mock`. |
| GET  | `/api/secrets/collections` | `?organization=ORG_ID` | List collections, optionally scoped to one organization. Empty for `local` / `mock`. |

**Status codes:** `200/201/204` success, `400` bad input, `401` logged out, `404` not found, `409` ambiguous match or already-exists, `423` vault locked, `503` backend unavailable (e.g. `bw` not installed).

**Error body** (every non-2xx from these endpoints):
```json
{"error": {"kind": {"kind": "locked"}, "message": "vault is locked"}}
```
`kind` is the `SecretError` JSON (snake_case `kind` discriminator: `logged_out`, `locked`, `not_found`, `ambiguous`, `already_exists`, `bad_request`, `backend`; payload fields where present — `candidates` for `ambiguous`, `message` for `bad_request` / `backend`). `message` is a human string with C0 control characters stripped (defends against ANSI/OSC injection when the CLI prints the error verbatim).

**Logging discipline:** the daemon never logs the value field, the master password, or session tokens. The reveal endpoint is the only path that emits plaintext.

#### Session Snapshot
| Method | Path | Body / Query | Description |
|--------|------|------|-------------|
| POST | `/api/session/save` | — | Snapshot the multiplexer (workspaces/tabs) into the session store immediately. Durability is otherwise the 30s auto-save timer + close-to-tray hook; e2e harnesses call this before force-killing the daemon so restart-persistence is deterministic. A save that would drop workspaces/tabs vs the persisted `last.json` is refused unless it carries authorized-reduction intent (close verbs, Team close, policy closes, spawn rollbacks, boot cleanup of ghost/duplicate entries): the fuller persisted desk stays authoritative, the rejected snapshot is quarantined as `sessions/last.json.reduced-<epoch-ms>`, and the refusal is logged at error with drop counts. Refusal is protection, not failure — the endpoint still returns 200; only a genuine persist failure (I/O, serialization) errors. |
| GET | `/api/session/backups` | — | List rotated full-desk snapshots from `sessions/backups/`, newest first: `{backups: [{name, saved_at_secs, workspaces, tabs}]}`. CLI: `codeterm sessions backups list`. |
| POST | `/api/session/backups/{name}/restore` | — | Additive desk recovery from one backup: recreates workspaces/tabs missing from the live desk under their original ids; existing ones are never modified or closed. Returns `{restored, workspaces_added, tabs_added, tabs_already_present}`; 404 for an unknown/invalid name. CLI: `codeterm sessions backups restore <name>`. |

#### Web Mirror
| Method | Path | Body / Query | Description |
|--------|------|------|-------------|
| GET | `/` | — | The CodeTerm web mirror: the same React bundle as the desktop app, served static at the daemon root (public route; every API call it makes stays token-gated). `?a11y=1` enables the screen-reader terminal DOM. |
| GET | `/api/mux/state` | — | Full multiplexer snapshot (workspaces/tabs + focus) — the browser bundle's `mux_get_state`. **Bounded:** at most `MAX_CONCURRENT_STATE_SNAPSHOTS` (`api::snapshot_bound`) snapshots build at once; over that, or while a lifecycle writer owns the mux/tracker lock, the request is answered **503** immediately rather than queued. A `mux-state-changed` event drives the next read; callers must not poll. See *Full-state snapshot overload* below. |
| POST | `/api/workspaces/{id}/tabs/{tab_id}/focus` | — | Switch active tab. |
| POST | `/api/workspaces/{id}/tabs/{tab_id}/close` | — | Close a tab. |
| POST | `/api/tabs/{id}/chat-view` | `{enabled}` | Toggle a tab's chat view flag. |
| GET | `/api/mesh/peers/{id}/services` | — | Peer services view (same cache+failover as the Mesh tab). |
| GET | `/api/events/stream` | `?since=<boot>:<seq>` | SSE of system events (`data: {seq, boot, event_type, data}`) - mux/mesh/tab-status changes; the web bundle's `listen()`. `since` replays the retained ring then goes live; absent, it starts live. Loss is signalled by one `{event_type:"overflow"}` frame carrying `{reason: boot-changed|ring-evicted|lagged, boot, next_seq, oldest?}`, never by a silent gap. `?tab_id=` filters to one tab (the web mirror does not use it). |
| GET | `/api/output/stream` | `?since=<b64url cursor>` | Multiplexed SSE: ONE connection carries every Tab's PTY output as tab-tagged frames (`{tab_id, output, offset}`, plus per-tab `{truncated, from}` on ring eviction, `{ended}` on genuine PTY death, and a one-shot `{boot_changed, boot}` on daemon restart), replacing the per-tab long-poll below. `since` is a b64url `{boot, tabs:{id:offset}}` cursor. Killswitch: browser localStorage `codeterm-web-output-stream=off` falls back to `/api/pty/{id}/read`. With a mesh relay target set, the browser instead streams via `/api/mesh/relay-stream?path=/api/output/stream` — same frames, same cursor. |
| GET | `/api/pty/{id}/read` | — | Long-poll one chunk of PTY output (JSON byte array) — the browser terminal's read loop; fallback when the output-stream killswitch is off. |
| POST | `/api/pty/{id}/write` | `{data}` | Write input to a PTY. |
| POST | `/api/pty/{id}/resize` | `{cols, rows}` | Resize a PTY. |


**Snapshot refetch cost and its bound.** `mux-state-changed` is a bare signal
(`{"event":"mux_state_changed"}`), not a snapshot, so each one costs the client a
full `GET /api/mux/state`: one refetch per event, measured 1:1 over 1405 switches.
Bursts and the ring replay after an `overflow` therefore multiply, and the browser
once held >1000 concurrent snapshot requests. The bundle now coalesces to **one
snapshot fetch in flight** (`src/store/multiplexer/refreshCoalescer.ts`): a request
arriving mid-fetch never joins it -- that fetch may predate the request's event --
so it waits for the next one and a burst of any size costs at most two round trips.
The daemon independently refuses past its own concurrency bound with `503`; it also
refuses immediately when a lifecycle writer owns either snapshot lock. Both are
back-pressure, not failure: the provider keeps its state in a not-yet-reconciled
state and the next `mux-state-changed` or `mesh-state-changed` event schedules one
coalesced read. There is no timer or polling retry.

**Known defect (`rollacode/codeterm#428` family).** The "never by a silent gap"
guarantee above does not hold today. `EventBus::send_bus` allocates the seq under
the ring lock but publishes to the broadcast after releasing it, so concurrent
publishers can reach the bus out of order and `events_stream`'s monotonic
`replayed_max` guard then drops the lower seq with no `overflow` frame. The ring
still holds it in order, so a reconnect with `since` recovers it. Observed as 9
silent seq holes in a 9739-frame 60 s capture at high publish rate, none at ~4.5x
lower rate. The relay is not implicated: it copies bytes through unmodified.

#### Debug (test-only capabilities)
| Method | Path | Body / Query | Description |
|--------|------|------|-------------|
| GET | `/api/debug/clock` | — | `{mocked, now_ms}` — whether cron runs on a mock clock, and its current time. |
| POST | `/api/debug/clock/advance` | `{ms}` | Advance the mock cron clock and wake the scheduler so overdue jobs fire immediately. **409** unless the daemon is a debug build started with `CODETERM_TEST_CLOCK=1` (release builds never activate the mock). Powers the 07-cron e2e suite's time travel. |

#### Usage Tracker

Per-provider usage snapshots come from each provider plugin's quota-free `fetchUsage` capability. Claude reads its OAuth usage endpoint, Codex reads its account rate-limit endpoint, and Cursor reads its dashboard endpoint. The poller never starts an agent or sends a metered prompt to observe usage. Plugins reduce account responses to meter percentages and reset timestamps before the core persists them; credentials and unrelated account fields are never stored. The snapshots drive the in-app Usage bubble and history modal.

| Method | Path | Body / Query | Description |
|--------|------|------|-------------|
| GET | `/api/usage/latest` | `?provider=claude&account_id=…` | Latest snapshot for one provider/account. `null` when no data yet. |
| GET | `/api/usage/history` | `?provider=…&since=<ms>&limit=200&account_id=…` | Raw snapshots since `since` (epoch ms). `limit` clamped to `[1, 5000]`. |
| GET | `/api/usage/rollup/daily` | `?provider=…&since=<ms>&account_id=…` | Per-day max-used aggregates for the history chart. |
| GET | `/api/usage/projection` | `?provider=…&account_id=…` | Median-baseline projection: per-DOW shape + scale + overspend ETA. |
| POST | `/api/usage/refresh` | `{provider, account_id?}` | Trigger an out-of-band probe immediately. Returns `{queued, probe_id}`; `503` if the poller is not running. |

**Adaptive polling.** A single background scheduler ticks every 30 s and decides which provider/account is due. The cadence steps through `[600 s, 1200 s, 1800 s, 2400 s, 3000 s]` with ±10% jitter. A tab is active when `TabActivity::mark` records a recent keypress, send, or focus. Providers without a quota-free `fetchUsage` capability are not scheduled.

**Projection algorithm.** `src-tauri/src/usage/projection.rs` builds per-day-of-week medians from the last four weeks, projects today's curve from the elapsed shape, and rejects pair-noise jumps wider than four hours with more than 25% change. The frontend's `activeWeeklyCycle()` discards stale weekly-reset timestamps that lag the latest reset.

#### Logs

CodeTerm pipes daemon `tracing` output into a structured ring and SQLite store. Settings → Logs is the canonical UI; the REST shape is identical.

| Method | Path | Body / Query | Description |
|--------|------|------|-------------|
| GET | `/api/logs` | `?level=info\|warn\|error\|debug&source=<substr>&since=<ms>&limit=500` | Read recent app-log rows. `limit` is clamped to `[1, 5000]`. |
| DELETE | `/api/logs` | `?before=<ms>` | Purge rows older than `before` in epoch milliseconds. Returns `{deleted: N}`. |

Each row is `{id, ts, level, source, message, context_json}`. `source` is the `tracing` target. A 1000-entry in-memory ring drains into `logs/app_logs.db` once the WAL-mode SQL backend opens. The `log-emitted` Tauri event fires per record so the Logs tab can auto-tail without polling.

#### Past Agent Sessions
| Method | Path | Body / Query | Description |
|--------|------|------|-------------|
| GET | `/api/sessions/search` | `?q=...&provider=claude\|codex\|all&project=...&limit=N&max_snippets=N` | Cross-provider session content search |
| GET | `/api/sessions/{id}` | — | Resolve a session id to its provider metadata (cwd, file path, timestamps) |
| POST | `/api/sessions/{id}/resume` | `{title?, workspace_id?, workspace_name?}` or no body | Atomic resume with persisted orchestration identity or explicit roleless recovery actions |

Provider-agnostic: every registered `AgentProvider` contributes its own session enumerator + parser. Today Claude reads `~/.claude/projects/*.jsonl` (cwd extracted from JSONL content, not the lossy directory name); Codex walks `~/.codex/sessions/YYYY/MM/DD/*.jsonl` and parses `response_item` messages. Providers without on-disk session storage silently contribute nothing.

**GET /api/sessions/search** — whitespace-splits `q` into terms; every term must appear (case-insensitively) in at least one non-meta message of a session for it to match. Hits are sorted most-recently-modified first. `provider=all` (default) walks every registered provider; pass an id (`claude`, `codex`) to scope. `limit` defaults to 10, `max_snippets` to 3. Snippets carry ~80 chars of context before and ~120 after the first matching position, sliced on UTF-8 char boundaries so non-ASCII queries don't panic.

The scan is bounded, not exhaustive. Session metadata is enumerated first and ordered by recency; files are parsed in that order and the walk stops as soon as `limit` hits are collected, so a common query reads a handful of sessions rather than the whole corpus. Any single session file larger than 8 MiB is skipped rather than loaded — real corpora hold multi-gigabyte rollouts, and parsing one to answer a text query is unbounded work. Content in a skipped session is therefore not searchable; `sessions get`/`resume` still resolve it, since those read metadata only. The walk runs on the blocking pool, so it never occupies the async runtime.

Response shape:
```json
{
  "hits": [
    {
      "meta": {
        "session_id": "0193e1a4-7b2c-7d4e-9f80-1234567890ab",
        "provider": "claude",
        "project_path": "/Users/me/code/foo",
        "title": null,
        "summary": null,
        "file_path": "/Users/me/.claude/projects/-Users-me-code-foo/0193e1a4-....jsonl",
        "created_ts": 1744300000,
        "modified_ts": 1744387200
      },
      "snippets": [
        {"role": "user", "text": "…fix the webpack config so HMR works…", "ts": 1744300050}
      ],
      "match_count": 4
    }
  ]
}
```

**GET /api/sessions/{id}** — returns the `SessionMetadata` for whichever provider claims this id. Returns `404` if no provider knows it (gone from disk, never existed, wrong machine).

**POST /api/sessions/{id}/resume** — single call replacing the error-prone "create workspace at guessed cwd → spawn agent with `--resume <id>`" dance. Steps:
1. Locate the session across providers (`AgentRegistry::find_session`) → get cwd + provider id from disk metadata (Claude reads cwd out of the JSONL itself; the encoded directory name is NOT trusted because it loses leading-dot folders and case).
2. Reuse the workspace at `workspace_id` if given; otherwise create one at the recorded cwd, named `workspace_name` or the cwd basename.
3. Restore authoritative role, profile, parent, and spawn authority from the exact provider session binding. A Worker or Manager is restored only while its Orchestrator parent is live.
4. Spawn an identity-only agent tab with the provider resume command and `NoStarter`. Historical tasks, starter prompts, launch assignments, fallback input, and wake signals are never replayed.
5. If authoritative identity is unavailable, return `identity.status = "roleless"` with exact promotion and relink commands. The CLI prints `resumed roleless` rather than claiming restoration.

Response:
```json
{
  "tab_id": "d5e6f7a8",
  "workspace_id": "b4e2c8d1",
  "provider": "claude",
  "session_id": "0193e1a4-7b2c-7d4e-9f80-1234567890ab",
  "cwd": "/Users/me/code/foo",
  "resume_command": "claude --resume 0193e1a4-...",
  "identity": {
    "status": "restored",
    "role": "Worker",
    "role_profile": "executor",
    "parent_tab_id": "a3f1b2c7",
    "can_spawn": false
  }
}
```

Roleless responses instead include `identity.actions.promotion` and `identity.actions.relink` as directly executable CLI guidance.

Errors: `404` if the session id isn't found; `400` if the located session has no `project_path` (can't decide cwd); `500` on workspace create / agent spawn failure.

Session binding model: `docs/contributing/architecture.md` §3 (provider-reported binding).

#### Configuration
| Method | Path | Body / Query | Description |
|--------|------|------|-------------|
| GET | `/api/config` | `?key=dot.path` | Get full config or a single key (dot-notation) |
| PATCH | `/api/config` | `{key, value}` or partial merge | Set a config value by dot-notation key, or merge a partial config object |

**GET /api/config** — without `key`, returns the full `AppConfig` as JSON. With `?key=integrations.telegram_enabled`, returns `{"key": "...", "value": ...}`.

**PATCH /api/config** — two modes:
1. **Dot-notation key+value:** `{"key": "integrations.telegram_enabled", "value": true}` — sets a single key.
2. **Partial merge:** send top-level config sections as JSON — they are merged into the existing config.

The config is saved to disk and a `config-changed` Tauri event is emitted.

**Where notifications go is not writable here.** Anything under the `notifications`
subtree decides which surfaces a notification may reach, and switching a desktop
toast or a push channel on is a capability the user never granted by typing a
config key. Two shapes are refused with `403`: a dot-notation write naming any
`notifications` path, and a partial merge carrying a `notifications` section from
a caller that sent `X-CodeTerm-Tab-Id` (i.e. from inside a tab). An agent OFFERS
the change instead — `codeterm notify send --offer <surface> --offer-accept <label>`
— and it applies only when that label is tapped.

#### Updates
| Method | Path | Body / Query | Description |
|--------|------|------|-------------|
| GET | `/api/update/check` | `?force=true` | Check for available updates on the active channel. Cached for 1 hour unless `force=true`. |
| POST | `/api/update/install` | `?method=auto&manifest_url=...&after_update=...` | Install the pending update. Without `method=auto`, returns either an OS install command or opens the download URL in the browser. With `method=auto`, runs the full Tauri-plugin-updater flow (download, signature verify, install, restart). `after_update` (optional) stashes a message to `~/.codeterm/post-update-message.txt`; the new daemon delivers it to the General tab after restart. Update application is single flight process-wide: a request arriving while another is applying is refused with `{"status":"already_in_flight"}`, which is distinct from `no_update`. |
| GET | `/api/update/channel` | — | Returns the active diagnostic channel, currently `{"channel":"stable"}` on normal installs. |
| POST | `/api/update/channel` | `{channel}` | Disabled while CodeTerm ships only the default update channel; returns `409 update channel switching is disabled`. |

**Channels.** Channel switching is intentionally disabled for now; use the default channel. Manifests are signed with ed25519 — the public key is embedded in `tauri.conf.json`; signature verification is handled by `tauri-plugin-updater` and a wrong-key signature aborts the install before any binary is touched.

**Auto install flow (`method=auto`).** The daemon spawns a background tokio runtime that calls `update::apply::check_and_apply`. This:
1. Discovers the stable release with a no-cache GitHub request, then pins the install to that tag's immutable manifest. A `manifest_url` query override still takes precedence; non-HTTPS overrides require `dev-updater-allowlocal`.
2. Downloads the platform asset, verifies the ed25519 signature, performs the platform-specific install (atomic binary swap on Windows, rsync into `.app` on macOS).
3. Performs the 9-step graceful shutdown: tabs are signaled to exit, the mem WAL is checkpointed, the supervisor is stopped, then the app restarts.
4. After restart, tabs with valid recorded provider sessions resume; stale or missing sessions launch fresh.

The HTTP response returns immediately (`{"status": "updating", "version": "..."}`); progress is reported via Tauri events to the frontend (`UpdateBadge` + `UpdateModal`).

**Manual install fallback.** Without `method=auto`, the response carries either:
- `{"status": "command", "command": "..."}` — for package-manager installs (Homebrew, Scoop) where running a shell command is the right action.
- `{"status": "browser_opened", "url": "..."}` — for raw archive/installer downloads.
- `{"status": "up_to_date"}` — when no update is available.

**Errors:** `502 Failed to check for updates` when GitHub is unreachable; `400 Run check first` when `install` is called before `check` populated the cache; `400 invalid channel` on a malformed `channel` body.

### Full-state snapshot overload

The 8-way bound on concurrent `GET /api/mux/state` snapshots, its 503 answer, and the client contract: see `codeterm docs API-EVENTS`.

## Related documents

Service registration, health, and activity: `codeterm docs API-SERVICES`. Bridge and cron integrations: `codeterm docs API-INTEGRATIONS`.

<!-- revision: 14 -->
