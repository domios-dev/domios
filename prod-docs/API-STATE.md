# State, observability, and update API

Secrets, sessions, web mirror, diagnostics, usage, logs, configuration, and update endpoints.

## Storage authority and recovery

Each instance has one typed Storage owner for migrated desk, cron, and Blueprint state in `storage/state.db` beneath that instance's data directory. Storage opens before restore and schedulers start. Its one-time importer seals and verifies a copy of recognized legacy inputs under `storage/legacy-originals/`; it retains the original files and switches authority only after archive and semantic checks pass. A failed open, import, or verification leaves migrated state unavailable. Storage-dependent API operations return 503 with the Storage-unavailable error; they do not return or create an empty substitute.

For a Storage backup, stop Domios cleanly and copy the complete `storage/` directory, including `state.db`, referenced assets, and the import archive. Restore the complete backup into an empty `storage/` root with a Storage-compatible build. Never downgrade by running an older binary against the retained legacy files: writes made after cutover are absent there. There is no supported in-place downgrade or JSON rollback; preserve both copies and use a reviewed compatible recovery path.

#### Secrets

Pluggable secret-store layer. Active backend is selected by `integrations.secrets.backend` in `config.toml`; built-in backends are `local` (plaintext local file, default), `bitwarden` (Bitwarden CLI, optional Vaultwarden server), and `mock` (in-memory, dev only). All endpoints require the standard Bearer token. **Local-only by design** — secret routes are never relayed across the mesh; the CLI rejects `--mesh` on `mem secret` subcommands with exit code 7.

| Method | Path | Body / Query | Description |
|--------|------|------|-------------|
| GET  | `/api/secrets/status` | — | `{backend, status:{status: "logged_out"\|"locked"\|"unlocked"\|"unavailable", user?, transient?, endpoint?, reason?}, auto_unlock_capable, master_password_saved}`. `auto_unlock_capable` is the active backend's manifest declaration (`secretBackend.autoUnlock`); `master_password_saved` is true only when the capability is declared **and** the backend's own bucket holds its credential. See [SECRETS.md](SECRETS.md#the-auto-unlock-contract). |
| POST | `/api/secrets/unlock` | `{master_password, email?, two_factor_token?, api_key_client_id?, api_key_client_secret?, persist_for_auto_unlock?}` | Unlock the active backend. Master password is consumed and dropped immediately. `persist_for_auto_unlock` (default `false`) is the explicit consent to remember the credential for headless unlock; it is refused with `400 bad_request` when the active backend declares no auto-unlock capability. See [SECRETS.md](SECRETS.md#the-auto-unlock-contract). |
| POST | `/api/secrets/lock` | — | Drop in-process session; persisted creds are kept. |
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
| POST | `/api/session/save` | optional `{continue_tab, continue_prompt}` | Flush accepted desk writes through Storage. With `continue_tab`, the next start delivers one system continue to that tab (the supplied prompt or default) after its own resumed session is agent-ready; the response reports `registered` or a refusal reason (`no_agent_session`, `no_pty_agent_session`). A snapshot that would drop workspaces or tabs from the committed desk is refused unless it carries authorized-reduction intent; the committed desk remains authoritative. Storage errors are reported, never replaced by empty state. |
| GET | `/api/session/backups` | — | List rotated full-desk backup projections from Storage, newest first: `{backups: [{name, saved_at_secs, workspaces, tabs}]}`. CLI: `codeterm sessions backups list`. |
| POST | `/api/session/backups/{name}/restore` | — | Additive desk recovery from one backup: recreates workspaces/tabs missing from the live desk under their original ids; existing ones are never modified or closed. Returns `{restored, workspaces_added, tabs_added, tabs_already_present}`; 404 for an unknown/invalid name. CLI: `codeterm sessions backups restore <name>`. |

Authenticated snapshot exchange: `GET /api/session/backups/{name}/export`
accepts `latest` or a backup timestamp and returns a sanitized desk, Plan store,
filler histories, size evidence and hidden-tab metadata. CLI:
`codeterm sessions backups export latest --sanitize --out /tmp/desk.json`.
`POST /api/session/backups/import` replaces a disposable named `upgrade-restore-*`
DEV desk without PTYs; PROD refuses it. CLI: `codeterm sessions backups import --file /tmp/desk.json`.
Import returns `{imported_tabs, restart_required, boot_id}`; `/api/health` boot_id proves a subsequent restart.

#### Web Mirror

| Method | Path | Body / Query | Description |
|--------|------|------|-------------|
| GET | `/` | — | The Domios web mirror: the same React bundle as the desktop app, served static at the daemon root (public route; every API call it makes stays token-gated). `?a11y=1` enables the screen-reader terminal DOM. |
| GET | `/api/mux/state` | — | Published multiplexer snapshot (workspaces/tabs + focus). Writers and rebuilds do not delay reads or cause 503. Genuine storage unavailability still returns 503. Publication sends `mux-state-changed`; callers must not poll. |
| POST | `/api/workspaces/{id}/tabs/{tab_id}/focus` | — | Switch active tab. |
| POST | `/api/workspaces/{id}/tabs/{tab_id}/close` | — | Close a tab. |
| POST | `/api/tabs/{id}/chat-view` | `{enabled}` | Toggle a tab's chat view flag. |
| GET | `/api/mesh/peers/{id}/services` | — | Peer services view (same cache+failover as the Mesh tab). |
| GET | `/api/events/stream` | `?since=<boot>:<seq>` | SSE of system events (`data: {seq, boot, event_type, data}`) - mux/mesh/tab-status changes; the web bundle's `listen()`. `since` replays the retained ring then goes live; absent, it starts live. Loss is signalled by one `{event_type:"overflow"}` frame carrying `{reason: boot-changed|ring-evicted|lagged, boot, next_seq, oldest?}`, never by a silent gap. `?tab_id=` filters to one tab (the web mirror does not use it). |
| GET | `/api/output/stream` | `?since=<b64url cursor>` | Multiplexed SSE: ONE connection carries every Tab's PTY output as tab-tagged frames (`{tab_id, output, offset}`, plus per-tab `{truncated, from}` on ring eviction, `{ended}` on genuine PTY death, and a one-shot `{boot_changed, boot}` on daemon restart), replacing the per-tab long-poll below. `since` is a b64url `{boot, tabs:{id:offset}}` cursor. Killswitch: browser localStorage `codeterm-web-output-stream=off` falls back to `/api/pty/{id}/read`. With a mesh relay target set, the browser instead streams via `/api/mesh/relay-stream?path=/api/output/stream` — same frames, same cursor. |
| GET | `/api/pty/{id}/read` | — | Long-poll one chunk of PTY output (JSON byte array) — the browser terminal's read loop; fallback when the output-stream killswitch is off. |
| POST | `/api/pty/{id}/write` | `{data}` | Write input to a PTY. |
| POST | `/api/pty/{id}/resize` | `{cols, rows}` | Resize a PTY. |

Published reads, coalesced refresh and stream event order: `codeterm docs API-EVENTS`.

#### Debug (test-only capabilities)
| Method | Path | Body / Query | Description |
|--------|------|------|-------------|
| GET | `/api/debug/clock` | — | `{mocked, now_ms}` — whether cron runs on a mock clock, and its current time. |
| POST | `/api/debug/clock/advance` | `{ms}` | Advance the mock cron clock and wake the scheduler so overdue jobs fire immediately. **409** unless the daemon is a debug build started with `CODETERM_TEST_CLOCK=1` (release builds never activate the mock). |

#### Usage Tracker

Per-provider usage snapshots come from each provider plugin's quota-free `fetchUsage` capability. Claude reads its OAuth usage endpoint, Codex reads its account rate-limit endpoint, and Cursor reads its dashboard endpoint. The poller never starts an agent or sends a metered prompt to observe usage. Plugins reduce account responses to meter percentages and reset timestamps before the core persists them; credentials and unrelated account fields are never stored. The snapshots drive the in-app Usage bubble and history modal.

| Method | Path | Body / Query | Description |
|--------|------|------|-------------|
| GET | `/api/usage/latest` | `?provider=claude&account_id=…` | Latest snapshot for one provider/account. `null` when no data yet. |
| GET | `/api/usage/history` | `?provider=…&since=<ms>&limit=200&account_id=…` | Raw snapshots since `since` (epoch ms). `limit` clamped to `[1, 5000]`. |
| GET | `/api/usage/rollup/daily` | `?provider=…&since=<ms>&account_id=…` | Per-day max-used aggregates for the history chart. |
| GET | `/api/usage/projection` | `?provider=…&account_id=…` | Median-baseline projection: per-DOW shape + scale + overspend ETA. |
| POST | `/api/usage/refresh` | `{provider, account_id?}` | Trigger an out-of-band probe immediately. Returns `{queued, probe_id}`; `503` if the poller is not running. |
| POST | `/api/usage/ledger/rebuild` | — | Re-derive KPI `token_usage` rows from the transcript behind every recorded usage cursor: one row per provider response, charged to the tab and plan of the rows it replaces. Idempotent. Returns `{sources, missing_sources, responses, legacy_rows_removed, unattributed_responses}`; `503` when the KPI ledger is not recording. CLI: `codeterm usage rebuild-ledger`. |

Usage refresh is quota-free, rebuild is idempotent, projections are estimates, and unsupported providers have no automatic usage probe.

Domios keeps structured daemon logs. Settings → Logs is the canonical UI; the REST shape is identical.

| Method | Path | Body / Query | Description |
|--------|------|------|-------------|
| GET | `/api/logs` | `?level=info\|warn\|error\|debug&source=<substr>&since=<ms>&limit=500` | Read recent app-log rows. `limit` is clamped to `[1, 5000]`. |
| DELETE | `/api/logs` | `?before=<ms>` | Purge rows older than `before` in epoch milliseconds. Returns `{deleted: N}`. |

Each row is `{id, ts, level, source, message, context_json}`. `source` is the `tracing` target. Logs are stored in `logs/app_logs.db` in the data directory. A `log-emitted` event fires per record.

#### Past Agent Sessions
| Method | Path | Body / Query | Description |
|--------|------|------|-------------|
| GET | `/api/sessions/search` | `?q=...&provider=<id>\|all&project=...&limit=N&max_snippets=N` | Cross-provider session content search; returns `{hits, warnings}` |
| GET | `/api/sessions/{id}` | — | Resolve a session id to its provider metadata (cwd, file path, timestamps) |
| POST | `/api/sessions/{id}/resume` | `{title?, workspace_id?, workspace_name?}` or no body | Atomic resume with persisted orchestration identity or explicit roleless recovery actions |

Provider-agnostic: every provider with stored sessions contributes them (Claude, Codex, OpenCode); providers without stored sessions contribute nothing.

**GET /api/sessions/search** — whitespace-splits `q` into terms; every term must appear (case-insensitively) in at least one non-meta message of a session for it to match. Hits are sorted most-recently-modified first. `provider=all` (default) walks every registered provider; pass an id (`claude`, `codex`, `opencode`) to scope, and an id no provider owns is refused with 400. `limit` defaults to 10, `max_snippets` to 3. Snippets carry ~80 chars of context before and ~120 after the first matching position, sliced on UTF-8 char boundaries so non-ASCII queries don't panic.

A session's searchable text is bounded to its newest 8 MiB.

The scan is bounded, not exhaustive. Each provider orders its sessions by recency and stops at `limit` hits, and the merged result keeps the newest `limit`. Search may skip or truncate unreadable or large sessions (over 8 MiB) with explicit warnings. `sessions get`/`resume` still resolve a skipped session, since those read metadata only. Each provider logs `[sessions-search] provider=… sessions=… scanned=… parsed=… elapsed_ms=…`.

Nothing is skipped silently. `warnings` lists, per provider, the sessions it could not fully search: `{provider, reason, sessions, message}`, where `reason` is `no_content_source`, `over_byte_cap`, `unreadable`, `content_truncated` (a store session searched only in its newest 8 MiB), or `scan_failed`. The CLI prints each `message` after the hits, also when nothing matched.

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

**GET /api/sessions/{id}** — returns the `SessionMetadata` for whichever provider claims this id. Each provider answers from its own per-session record; session history is never enumerated, so the cost does not grow with the number of stored sessions. Returns `404` if no provider knows it (gone from disk, never existed, wrong machine).

**POST /api/sessions/{id}/resume** — single call replacing the error-prone "create workspace at guessed cwd → spawn agent with `--resume <id>`" dance. Steps:
1. Locate the session across providers and read its cwd and provider id from the session's own metadata.
2. Reuse the workspace at `workspace_id` if given; otherwise create one at the recorded cwd, named `workspace_name` or the cwd basename.
3. Restore authoritative role, profile, parent, and spawn authority from the exact provider session binding. A Worker or Manager is restored only while its Orchestrator parent is live.
4. Spawn an identity-only agent tab with the provider resume command and no starter. Historical tasks, starter prompts, launch assignments, fallback input, and wake signals are never replayed.
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
| POST | `/api/update/channel` | `{channel}` | Disabled while Domios ships only the default update channel; returns `409 update channel switching is disabled`. |
| GET | `/api/update/status` | — | Current updater state and last update result |

**Channels.** Channel switching is intentionally disabled for now; use the default channel. Manifests are signed with ed25519 — a wrong-key signature aborts the install before any binary is touched.

**Auto install flow (`method=auto`).** Auto-install returns `updating` immediately, then:
1. Discovers the stable release with a no-cache GitHub request, then pins the install to that tag's immutable manifest. A `manifest_url` query override still takes precedence; non-HTTPS overrides require `dev-updater-allowlocal`.
2. Downloads the selected platform artifact and verifies its signature before installing it.
3. Shuts down gracefully and restarts the app.
4. After restart, tabs with valid recorded provider sessions resume; stale or missing sessions launch fresh.

The HTTP response returns immediately (`{"status": "updating", "version": "..."}`); the app reports progress.

**Manual install fallback.** Without `method=auto`, the response carries either:
- `{"status": "command", "command": "..."}` — for package-manager installs (Homebrew, Scoop) where running a shell command is the right action.
- `{"status": "browser_opened", "url": "..."}` — for raw archive/installer downloads.
- `{"status": "up_to_date"}` — when no update is available.

**Errors:** `502 Failed to check for updates` when GitHub is unreachable; `400 Run check first` when `install` is called before `check` populated the cache; `400 invalid channel` on a malformed `channel` body.

### Tab-scoped local previews

The browser mirror uses `POST /api/tabs/{id}/preview/read` with `{ "path": "/absolute/file.md", "kind": "document" }` and `POST /api/tabs/{id}/preview/check` with `{ "paths": ["/absolute/file.md"] }`. Both require the normal Bearer authentication and an exact live tab ID. Peer access follows the tab's read permissions and ownership policy.

A file must be referenced by that tab's own transcript: a full-line `@ds-image`, `@ds-document`, `@ds-page` (or `@ct-*`/`@file`) directive, an attachment, a recorded edit/diff path, or an image result path. Client-supplied paths do not grant access. Prose mentions, fenced examples and another tab's references grant nothing. Unavailable and refused reads return the same `404` JSON error; batch checks omit refused paths without exposing their existence.

Read returns `{ "data": "...", "media_type": "..." }`; images use a base64 data URL. Batch returns `{ "paths": [...], "home": "..." }`. Allowed extensions and byte limits are shared with desktop: HTML pages 200,000 bytes, UTF-8 documents 1,000,000 bytes, images 10 MiB. Symlinks/junctions, hardlinks, traversal, UNC/device paths and alternate streams are refused. CodeTerm data directories, SSH/cloud credential directories and credential/token filenames are denied. Additional absolute file or directory roots can be denied with `api.preview_secret_paths = ["/absolute/private"]`; invalid roots fail closed.

Responses carry `Cache-Control: no-store`, `X-Content-Type-Options: nosniff`, and a restrictive sandbox CSP. HTML is returned inside JSON and rendered in the existing sandboxed preview. Requests are capped at 120 per minute across these routes, four concurrent operations, 64 KiB request bodies and 128 paths per batch. Excess traffic returns `429`; oversized batches return `413`. Reveal in the OS and Open in the OS remain desktop-only.

Related documents — service registration, health, and activity: `codeterm docs API-SERVICES`. Bridge and cron integrations: `codeterm docs API-INTEGRATIONS`.

<!-- revision: 29 -->
