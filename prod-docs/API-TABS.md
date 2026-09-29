# Tab and workspace API

REST fields and endpoints for tabs, workspaces, sessions, files, and related runtime state.

### Tab Fields

Full fields returned by `GET /api/tabs/inspect` for each selected tab:

| Field | Type | Description |
|-------|------|-------------|
| `id` | `string` | Unique tab ID (8-char hex, e.g. `a3f1b2c7`) |
| `pty_id` | `string` | Associated PTY process ID |
| `tab_type` | `string` | `"Terminal"` or `"Choice"` |
| `title` | `string` | The tab's single-word name; the sole name field in tab payloads. |
| `description` | `string?` | Short activity summary, max 200 chars; shown in the sidebar tooltip/subtitle and persisted in session snapshots. |
| `cwd` | `string?` | Current working directory (updated via OSC 7) |
| `agent_type` | `string?` | Agent running in tab, e.g. `"claude"` |
| `claude_session_id` | `string?` | Linked Claude Code session ID for resume |
| `role` | `string?` | `"Orchestrator"`, `"Worker"`, or `null` |
| `parent_tab_id` | `string?` | Parent tab ID in hierarchy, or `null` |
| `can_spawn` | `bool` | Whether this tab can spawn children. Orchestrator: true (default). Worker: always false. |
| `connected_services` | `string[]` | External services connected to this tab (e.g. `["telegram"]`), shown as sidebar icons. |
| `depth` | `number` | Hierarchy depth: 0 (no role), 1 (Orchestrator), 2 (Worker under Orchestrator) |

### Text input routes: `/send` vs `/chat-send`

Both routes exist; no new agent send primitive is needed. `src-tauri/src/api/tab_agent_routes.rs:16` mounts `/api/tabs/{id}/chat-send` next to `/api/tabs/{id}/send` as distinct handlers, so choosing is route selection.

| Route | Handler | Returns |
|-------|---------|---------|
| `POST /api/tabs/{id}/send` | `tab_routes::send_text` (`src-tauri/src/api/tab_routes/send.rs:228`) | `{ok:true, delivery:"sent"}` after confirmed delivery, `{ok:true, delivery:"pending"}` when an enveloped send is accepted in the background, or a structured delivery error with its `bytes_written` status. Long limited-provider sends add `pad_file`; batches add `pad_files`. |
| `POST /api/tabs/{id}/chat-send` | `tab_routes::chat_send_text` (`src-tauri/src/api/tab_routes/send.rs:14`) | One `ChatSendStatus` — `send_id`, `state`, `ledger_id`, `sequence`, optional `correlationId`, and optional `pad_file`; accepts a tab with agent identity at any launch stage or a chat-backend tab, and refuses shell-only tabs with `tab_busy_refused` and `bytes_written:false` |
| `GET /api/tabs/{id}/chat-send` | `tab_routes::chat_send_status` (`src-tauri/src/api/tab_routes/send.rs:50`) | `ChatSendStatus[]` for the tab, reconciled against session-turn evidence on read |

`POST .../send` is the synchronous REST write path for mesh relays, service push-token writes, and low-level `actions` / `raw` / `batch` terminal input; its `delivery` values are in the table above. A failure serializes the delivery taxonomy (`tab_not_reachable_on_peer`, `tab_busy_refused`, `pty_unavailable`, `submit_unconfirmed`) documented in `.agents/skills/pty-delivery/SKILL.md`.

Local `codeterm send` uses socket `tab.send_text`: `OK (sent)` after a confirmed write, `OK (pending)` when the route accepted background work or wrote bytes without submit confirmation, a refusal when no bytes were written. A substituted send names its recipient inbox file as `pad_file`; human output prints the path and multi-target JSON reports `pad_files`. With `--json`, `delivery` and any `bytes_written` field carry the same distinction for one target; multi-target JSON includes `pending_written` for accepted writes that still lack submit confirmation. The resolved payload echo is emitted for confirmed delivery or a pending write.

`POST .../chat-send` is the Chat UI user turn. A PTY tab without agent identity is refused before the send transaction so prose never reaches a bare shell. Accepted sends open a ledger entry for later reconciliation; `GET .../chat-send` reads it, `state` one of `accepted`, `unknown`, `confirmed`, `failed` (`ChatSendState`, `src-tauri/src/service/chat_send_transaction.rs:11`).

Local and mesh `codeterm send` open no Chat send ledger entry, so `GET .../chat-send` never lists them.

Neither route promises the recipient *answered*. Replies come through the sender's return-address channel (`codeterm send --tab ID [--mesh PEER]`, or `codeterm agent report` for a Worker). The recipient's visible terminal text is never a delivery signal.

### Workspace routes

| Method | Path | Body | Description |
|---|---|---|---|
| GET | `/api/workspaces` | — | List all workspaces |
| POST | `/api/workspaces` | `{name, cwd?, provision?}` | Create workspace; `name` required (absent body: `400`). A `cwd` matching a workspace returns it with `existing: true`. `provision: true` (`400` with `cwd`) creates a new empty data-dir directory; agents started exactly there get directory trust. |
| POST | `/api/workspaces/{id}/focus` | — | Switch to workspace |
| POST | `/api/workspaces/{id}/rename` | `{name}` | Rename workspace |
| POST | `/api/workspaces/{id}/close` | `{force?, confirm?}` or no body | Close the workspace. Agent tabs require `force: true`. A request without `X-CodeTerm-Tab-Id` is Human only when it sends `{force: true, confirm: "<workspace_id>"}` after the user confirms (logged `human-attributed (headerless, confirmed)`); otherwise it is refused as Unattributed and logged. A coordinator agent cannot close its own workspace even with `force: true`. Every Team with members here is closed through Team close before remaining tabs are removed. |
| GET | `/api/workspaces/{id}/recently-closed` | — | This workspace's recently-closed tabs as `ClosedTab[]` (see [Closed History & Restore](#closed-history--restore)). |
| POST | `/api/workspaces/{id}/reopen-closed` | `{index?}` or no body | Reopen a recently-closed tab (browser Ctrl+Shift+T). `index` picks the ring entry (default 0, most recent). Returns `TabInfo {id, title, tab_id}`. |
| POST | `/api/workspaces/{id}/tabs` | `{eager?}` or no body | Create tab; no body leaves eager PTY creation disabled. |
| POST | `/api/workspaces/{id}/tabs/{tab_id}/rename` | `{name}` | Rename tab |
| POST | `/api/workspaces/{id}/tabs/{tab_id}/move` | `{to_workspace_id}` | Move tab between workspaces |

#### Permission Hook (POST /api/tabs/{id}/hook/permission)

Called by Claude Code hooks (or any agent integration) when the agent shows a permission prompt; faster than PTY auto-detection and bypasses the cooldown.

**Three modes:**

1. **Explicit prompt** -- provide `question` and `options` in the body. CodeTerm uses them directly.
2. **Auto-parse** -- send no body or `{}`. CodeTerm runs structural fingerprint detection on the terminal screen to extract the prompt.
3. **Per-tool payload** -- send the provider's own `PermissionRequest`: `tool_name`, `tool_input` and `permission_suggestions[]` are kept verbatim and the vendor's suggestions become the surfaced options. No screen scraping and no late `Notification` wait. A body without `tool_name` keeps modes 1 and 2. A report whose binding nonce disagrees with the tab's launch nonce is rejected with 409.

```bash
# Auto-parse (most common — Claude hooks just ping the endpoint)
curl -X POST "http://host:7685/api/tabs/$TAB/hook/permission" \
  -H "Content-Type: application/json" -d '{}'

# Explicit prompt
curl -X POST "http://host:7685/api/tabs/$TAB/hook/permission" \
  -H "Content-Type: application/json" \
  -d '{"question":"Allow file write?","options":[{"label":"Yes","value":"1"},{"label":"No","value":"3"}]}'
```

Response: `{"detected": true|false}`. Even with `detected: false` the hook fires an `agent.permission_request` webhook event, since the hook itself signals the agent is waiting.

#### Stop Hook (POST /api/tabs/{id}/hook/stop)

Called by Claude Code hooks (Stop matcher) when the agent completes its task. No body is accepted. Triggers the `agent.task_complete` webhook event and updates tab status. Protected tabs (e.g. General Agent) keep `agent_type`; the stop hook does NOT clear it.

```bash
curl -X POST "http://host:7685/api/tabs/$TAB/hook/stop" \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $CODETERM_API_TOKEN" -d '{}'
```

#### Session State Hook (POST /api/tabs/{id}/hook/session_state)

Provider-reported live turn state: `{event, provider_id?, session_id?, binding_nonce?}`. `event` resolves against the reporting provider's declared `hookEventRoster`, so an undeclared event changes nothing; a binding nonce disagreeing with the tab's launch nonce is rejected with 409. A binding here carries a `working` or `idle` verdict (OpenCode declares `session.status` Working, `session.idle` Idle); core reads the declaration, never a provider id.

#### SessionStart Hook (POST /api/tabs/{id}/hook/session_start)

Called by Claude Code's `SessionStart` matcher whenever Claude opens a session: fresh launch, `--resume`, `/new`, or `/clear`. No body is accepted for a status-only report. CodeTerm persists the `session_id` from the JSON Claude pipes to the hook on stdin, so the next restart resumes the right thread, not the previous stale id. A changed id makes the prior session a persisted context epoch: Chat keeps its transcript behind a labelled divider, and archived turns are excluded from the provider's new context.

```bash
# Claude pipes its hook payload on stdin; the installed hook forwards it verbatim.
curl -X POST "http://host:7685/api/tabs/$TAB/hook/session_start" \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $CODETERM_API_TOKEN" \
  --data-binary @-
```

Response: `{"ok": true, "changed": true|false}` (whether the persisted id changed), or `{"ok": true, "deferred": true, ...}` when a fileless session is held until its reported transcript path emits a filesystem materialization event. One deferral per tab; a newer refusal replaces it and any legitimate intervening binding cancels it. `{"ok": false, "reason": "no session_id in body"}` means the body lacks a recognizable `session_id` field.

#### Delayed Chat messages

| Method | Path | Body | Description |
|--------|------|------|-------------|
| GET | `/api/tabs/{id}/delayed-messages` | — | List queued and failed delayed Chat UI messages for this tab |
| POST | `/api/tabs/{id}/delayed-messages` | `{text, scheduledAtMs, createdByTab?, recurrence?: {everyMs, untilMs?, count?}}` | Queue a one-shot or recurring message for this tab. Recurrence starts at `scheduledAtMs`, repeats at an anchored interval of at least one minute, with at most one stop condition. Normal send is unaffected. |
| PATCH | `/api/delayed-messages/{id}` | `{text?, scheduledAtMs?, recurrence?: {everyMs, untilMs?, count?}}` | Edit a queued or failed delayed message, including its recurrence rule |
| DELETE | `/api/delayed-messages/{id}` | — | Delete a queued or failed delayed message |
| POST | `/api/delayed-messages/{id}/send-now` | — | Deliver a queued or failed delayed message immediately |

#### Session Rebind (POST /api/tabs/{id}/session/rebind)

User-facing recovery is `codeterm tab rebind SESSION_ID --tab ID`: it binds the named existing provider session with `SessionStart` validation, without restarting or interrupting the agent. The response names `found_session_id`, `requested_session_id`, and whether the binding changed. A missing session cannot evict a materialized session and returns a conflict naming both ids.

`GET /api/tabs/{id}/session/binding-state` reads the daemon's tab-scoped fact, never an inference: `healthy`, `deferred_pending`, `refused`, `unbound`, or `stale_pending`, with the current and requested session identities when known. `healthy` is the ordinary answer and carries no registry entry, so an absent entry alone never means trouble. Repair: `codeterm docs API-REPAIR`. Adopt a live foreign PTY as an agent send target: `codeterm docs API-ADOPT`.

#### Hook auth

`/api/tabs/*/hook/*` and `/api/mem/flush` require the Bearer token; no loopback path is auth-exempt. CodeTerm injects `CODETERM_API_TOKEN` (with `CODETERM_API_PORT` and `CODETERM_TAB_ID`) into every PTY, and installed Claude hook commands reference `${CODETERM_API_TOKEN}` directly. Custom hooks must set the same header.

#### File serving
| Method | Path | Body | Description |
|--------|------|------|-------------|
| GET | `/api/fs/list-dir` | `?path=...` | List a local directory |
| POST | `/api/fs/mkdir` | `{path}` | Create a local directory |
| GET | `/api/serve-file` | `?path=...` | Serve a local file (assets, screenshots) |

#### Notifications
| Method | Path | Body | Description |
|--------|------|------|-------------|
| GET | `/api/notifications` | `?unread=true` | List notifications |
| POST | `/api/notifications` | `{title, body?, severity?, actions?, sticky?, tab_id?, level?, event_key?, target_tab_id?, chat_message_id?}` | Create notification (+ desktop toast when the level gate admits it). Navigation provenance is daemon-resolved: `X-CodeTerm-Tab-Id` outranks a body `tab_id`, an id naming no live tab is dropped (the notification lands without a Show Tab target), and `tab_title`/`workspace_id` come from the multiplexer. Mesh-relayed calls lose every tab claim and carry the signed peer name. `target_tab_id` and `chat_message_id` name the chat message a Needs you row jumps to: navigation hints stored and emitted as sent, never provenance. Blank values are dropped, a message id without a target tab is dropped, and a mesh-relayed call keeps neither. A gone tab or message renders the row unavailable. `level` is the closed set `verbose` / `action_needed` / `failure`; anything else is `400`. It gates the interrupting sinks: a notification without a level is stored and reaches the panel but raises no toast or push. The panel is ungated: the complete log, every record live. Each interrupting destination follows `notifications.agent_levels` (desktop, then one key per push-channel service id; missing means closed); desktop also follows `notifications.desktop` as master. The row's `bell` field is inert, named only so flattened channel keys never read it as a channel. The row's `sound` field is named for the same reason but is NOT inert: whether that level chimes in the app; `true`/`false` is the user's answer and an omitted key falls back to what shipped (`action_needed` and `failure` chime, `verbose` does not), with `notifications.sound` as the master mute over all of them. The desktop toast also requires the OS permission CodeTerm requests when the surface is enabled: without a grant, or when the native send fails, the toast is not claimed delivered and the in-app fallback window carries it. A level with every column closed is still recorded. `notifications.show_bell` only hides the indicator, never changing what is recorded; but when it is off AND no toast is admitted AND no push channel carries the record, the response (still `200` with its `id`) adds `unseen: {closed:[{surface,reason}]}` naming why each surface is shut. The key is absent whenever any surface is open. `severity` (`info`/`warning`/`error`) is toast styling only. Push outcome is never returned; an opted-in channel that cannot carry the message logs and marks the record failed. `actions` is the CALLER-facing closed set: `{type:"url",label,value}`, `{type:"dismiss",label}`, `{type:"answer",label}`; an unknown kind or field is refused at parse. `invoke` is reserved for daemon-owned actions and never accepted from callers; a stored row with an unsupported `invoke` loads with the action stripped and the message kept. `answer` is the only kind an agent may offer: a label only, and a notification carrying one records the resolved asking tab at creation. Blank or duplicate choice labels are `400`. |
| POST | `/api/notifications/{id}/answer` | `{label}` | Record the user's choice. The label must be one the notification offered; the answer goes to the tab captured when the question was sent, never one resolved now. One answer per notification: a later call returns the standing `{answered}` unchanged. The answer is carried to that tab exactly once as an ordinary host message through the existing envelope. Response `{answered, delivered}` (plus `deliveryError` when the send was refused): recording and delivery are separate facts, so a refused send reads `delivered:false`. `409` when the label was never offered or the asking tab is gone. |
| POST | `/api/notifications/clear` | — | Clear all |
| GET | `/api/notifications/channels` | — | `{channels:[{id,label,control_available}]}`: registered push channels in registry order. `id` is the bridge service id, `label` the settings column caption, `control_available` whether that channel's controls may be offered (Telegram: a live bridge supervisor AND non-empty `telegram_allowed_users`). The settings grid draws its columns from this list, so new channels appear without a client change. |

#### Webhooks
| Method | Path | Body | Description |
|--------|------|------|-------------|
| GET | `/api/webhooks` | — | List registered webhooks |
| POST | `/api/webhooks` | `{url, events?, once?, tab_id?, headers?, payload_extra?}` | Register webhook |
| DELETE | `/api/webhooks/{id}` | — | Unregister webhook |

Webhook events: `agent.permission_request`, `agent.task_complete`, `agent.report`

#### Themes
| Method | Path | Body | Description |
|--------|------|------|-------------|
| GET | `/api/themes` | — | List all themes (builtin + custom) |
| POST | `/api/themes` | `{id, toml}` | Save a custom theme |
| DELETE | `/api/themes/{id}` | — | Delete a custom theme |
| POST | `/api/themes/apply` | `{name}` | Apply a theme by name |

Custom themes are `.toml` files in `%APPDATA%/codeterm/themes/` (Windows) or `~/.codeterm/themes/` (macOS/Linux).

#### Mesh Networking
| Method | Path | Body / Query | Description |
|--------|------|------|-------------|
| GET | `/api/mesh/ping` | — | Lightweight mesh liveness probe |
| GET | `/api/mesh/debug/sign` | — | Mint a signed mesh auth token for diagnostics |
| GET | `/api/mesh/status` | — | Mesh identity, peers, discovery status |
| GET | `/api/mesh/peers` | — | List peers (paired + discovered) |
| POST | `/api/mesh/discover` | — | Run discovery, return discovered peers |
| POST | `/api/mesh/connect` | `{instance_id}` | Pair with a discovered peer (crypto server-side) |
| POST | `/api/mesh/approve` | `{request_id}` | Accept an incoming pairing request |
| POST | `/api/mesh/reject` | `{request_id}` | Reject and block an incoming pairing request |
| POST | `/api/mesh/disconnect` | `{instance_id}` | Unpair from a paired peer (notifies remote side) |
| POST | `/api/mesh/pair/request` | `PairingRequest` | Inbound pairing request; returns `{ok, instance_id}` so a manual target is promoted before acceptance |
| POST | `/api/mesh/pair/accept` | `PairingAccept` | Inbound acceptance for a pairing request |
| POST | `/api/mesh/pair/confirm` | `PairingConfirm` | Inbound pairing confirmation |
| POST | `/api/mesh/pair/reject` | `{request_id}` | Inbound pairing rejection |
| POST | `/api/mesh/introduce` | `{...}` | Introduce a discovered peer; creates a pending owner-approved pairing request |
| POST | `/api/mesh/sync` | `MeshSyncPayload` | Exchange mesh gossip/discovery state |
| POST | `/api/mesh/broadcast/start` | — | Start local discovery broadcast |
| POST | `/api/mesh/broadcast/stop` | — | Stop local discovery broadcast |
| POST | `/api/mesh/relay` | `{method, path, body}` | Relay an API call to a remote peer |
| POST | `/api/mesh/relay-to` | `{target_peer_id, method, path, body}` | Local browser gateway call: a signed relay request that never exposes the target token. Requires relay protocol version `1`. |
| POST | `/api/mesh/rpc` | `{method, params, request_id}` | Execute allowlisted plan/spec/task/team JSON-RPC through the peer's in-process dispatcher. Requires full peer access and deduplicates by request ID. |
| GET | `/api/mesh/relay-stream` | `?peer=<id>&path=<streamable>&since=<cursor>` | Browser→local byte-pipe of a peer's own `/api/events/stream` or `/api/output/stream`, cursor-resumed and un-timed. Peer unreachable → `504` `peer-unreachable`. |
| GET | `/api/mesh/relay-asset/{peer}/{*path}` | `?token=<LOCAL>` | Read-only, `plugins/{id}/ui/*`-scoped byte proxy for a peer's plugin-UI iframe. |
| DELETE | `/api/mesh/peers/{id}` | — | Remove a peer (internal, disconnect broadcast) |
| POST | `/api/mesh/refresh` | — | Refresh local addresses and announce to peers |
| POST | `/api/mesh/peers/add-manual` | `{address, port}` | Inject a `Manual` discovered peer by Tailscale IP/port (no mDNS/UDP scan). Returns `{ok, instance_id}` and emits `mesh-state-changed`. |
| POST | `/api/mesh/peers/approve/{id}` | — | Path-style alias for `POST /api/mesh/approve` (`request_id` in URL). |
| POST | `/api/mesh/peers/reject/{id}` | — | Path-style alias for `POST /api/mesh/reject`. |
| POST | `/api/mesh/peers/disconnect/{id}` | — | Path-style alias for `POST /api/mesh/disconnect` (`instance_id` in URL). |
| POST | `/api/mesh/announce` | `PeerAnnounce` | Inbound peer announcement with addresses, `relay_protocol_version`, and optional typed `web_ui` endpoints |
| POST | `/api/mesh/announce/all` | `?to=peer_id?` | Announce local addresses to one or all peers |
| POST | `/api/mesh/peers/{id}/addr` | `{ip, port?}` | Override a paired peer's cached address |

**Mesh-disabled gate (503).** With `config.mesh.enabled = false`, auth middleware answers every `/api/mesh/*` path except `/api/mesh/ping` with `503 Mesh networking is disabled`, as it does any caller authenticated by mesh signed token (a relaying peer); the local Bearer token still works. Re-enabling restores mesh routing without a daemon restart.

**Mesh peer access levels.** Direct signed requests and relayed requests use the same policy. Local-only secret routes and mesh token minting are refused for every peer.

| Level | Permitted operations |
|---|---|
| Full | All peer API routes, including announce, sync, and peer introductions, except local-only routes. |
| ReadOnly | Permitted `GET` routes only, except local-only routes. `POST /api/mesh/sync` is scoped to the sender's liveness and identity; announce and third-party introductions are refused. |
| AgentIsolated | `POST /api/agents/spawn`; `GET /api/tabs` returns only this peer's spawned tabs; `GET /api/tabs/{id}/text` and `POST` send, attachments, exec, respond, report, and clear are allowed only when `{id}` belongs to this peer. No workspace inventory, other tabs, mesh inventory, memory, config, secrets, or notifications. |
| Custom | Only configured `Permission` grants; `Admin` grants ordinary peer API access and sender-only sync except local-only routes and Full-only announce/introduction controls. |

The daemon records the authenticated spawning peer on each spawned tab and checks it on every tab-scoped route. Body-supplied ownership does not grant access.

**Peer-ping scheduler.** A background scheduler (`mesh::ping_scheduler`) pings each paired peer's `/api/mesh/ping` every 20 s with a 4 s timeout. On an online/offline transition the peer's `online` flag flips, `last_seen` updates on return, and `mesh-state-changed` fires so the UI re-renders without polling.

**Relay streaming (`relay-stream` / `relay-asset`).** These let a browser reach a paired peer without knowing its address. `relay-stream` opens a signed mesh request to the peer's own `/api/events/stream` or `/api/output/stream` (the only whitelisted paths) and copies bytes straight through, never buffered, filtered, or reordered, so the stream's cursor/overflow contract carries over. `relay-asset` is scoped to read-only `plugins/{id}/ui/*` GETs (path traversal rejected) because an `<iframe src>` can't send a `Bearer` header: the `.html` entry self-verifies the caller's LOCAL api token via `?token=` (exempted from bearer middleware by an exact prefix allowlist in `auth.rs`'s `is_custom_auth`), and the daemon reaches the peer with a server-minted signed mesh token never serialized to the browser.

**Relay security.** Direct signed peer requests and `/api/mesh/relay` share one access policy. `/api/secrets/*` and `/api/mesh/debug/sign` are local-only for every peer access level. REST `GET /api/config` always returns `api.token: null`; the browser retains only its gateway token locally. Relay, relay-stream, and relay-asset reject peers whose announced `relay_protocol_version` is not `1` with `426 Upgrade Required`.

**Tauri IPC (Mesh):**
| Command | Params | Description |
|---------|--------|-------------|
| `mesh_set_access` | `instance_id, access_level` | Set access level for a paired peer. `access_level`: `full`, `read_only`, `agent_isolated`. |

`connect`, `approve`, `reject`, and `disconnect` are high-level: they handle crypto, key exchange, and peer notification, mirroring the UI's Tauri IPC commands. Management commands are local-only and cannot be relayed via `--mesh`.

**Flat peer model:** Paired = trusted; no trust hierarchy or declarations. Each machine sets a per-peer **AccessLevel** (Full/ReadOnly/AgentIsolated, default Full) locally via `mesh_set_access`. UDP discovery broadcasts every 30 seconds.

## Related documents

Memory item, graph, calibration, and worker endpoints: `codeterm docs API-MEMORY`.

<!-- revision: 45 -->
