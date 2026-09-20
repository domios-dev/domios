# Tab and workspace API

REST fields and endpoints for tabs, workspaces, sessions, files, and related runtime state.

### Tab Fields

Full fields returned by `GET /api/tabs/inspect` for each selected tab:

| Field | Type | Description |
|-------|------|-------------|
| `id` | `string` | Unique tab ID (8-char hex, e.g. `a3f1b2c7`) |
| `pty_id` | `string` | Associated PTY process ID |
| `tab_type` | `string` | `"Terminal"` or `"Choice"` |
| `title` | `string` | The tab's single-word name. This is the sole name field in tab payloads. |
| `description` | `string?` | Short tab summary, max 200 chars. Put activity prose here; shown in the sidebar tooltip/subtitle and persisted in session snapshots. |
| `cwd` | `string?` | Current working directory (updated via OSC 7) |
| `agent_type` | `string?` | Agent running in tab, e.g. `"claude"` |
| `claude_session_id` | `string?` | Linked Claude Code session ID for resume |
| `role` | `string?` | `"Orchestrator"`, `"Worker"`, or `null` |
| `parent_tab_id` | `string?` | Parent tab ID in hierarchy, or `null` |
| `can_spawn` | `bool` | Whether this tab can spawn children. Orchestrator: true (default). Worker: always false. |
| `connected_services` | `string[]` | External services connected to this tab (e.g. `["telegram"]`). Shown as icons in sidebar. |
| `depth` | `number` | Hierarchy depth: 0 (no role), 1 (Orchestrator), 2 (Worker under Orchestrator) |

### Text input routes: `/send` vs `/chat-send`

Both text-input routes already exist, so no new agent send primitive is
required. `src-tauri/src/api/tab_agent_routes.rs:16` mounts
`/api/tabs/{id}/chat-send` next to `/api/tabs/{id}/send`, and the two are
distinct handlers. Choosing between them is route selection, not a missing
integration surface.

| Route | Handler | Returns |
|-------|---------|---------|
| `POST /api/tabs/{id}/send` | `tab_routes::send_text` (`src-tauri/src/api/tab_routes/send.rs:228`) | `{ok:true}` once delivery is accepted, or a structured `DeliveryError` |
| `POST /api/tabs/{id}/chat-send` | `tab_routes::chat_send_text` (`src-tauri/src/api/tab_routes/send.rs:14`) | One `ChatSendStatus` — `send_id`, `state`, `ledger_id`, `sequence`, optional `correlationId` |
| `GET /api/tabs/{id}/chat-send` | `tab_routes::chat_send_status` (`src-tauri/src/api/tab_routes/send.rs:50`) | `ChatSendStatus[]` for the tab, reconciled against session-turn evidence on read |

`POST .../send` is the ordinary write path: every CLI send, mesh relay, service
push-token write, and low-level `actions` / `raw` / `batch` terminal input. It
is **not** fire-and-forget — the tab is resolved before anything is written and
the response is returned only after delivery is accepted. A failure serializes
the delivery taxonomy (`tab_not_reachable_on_peer`, `tab_busy_refused`,
`pty_unavailable`, `submit_unconfirmed`) documented in
`.agents/skills/pty-delivery/SKILL.md`.

`POST .../chat-send` is the Chat UI user turn. It additionally opens a ledger
entry so the send can be reconciled later; `GET .../chat-send` reads that
ledger, and `state` is one of `accepted`, `unknown`, `confirmed`, `failed`
(`ChatSendState`, `src-tauri/src/service/chat_send_transaction.rs:11`).

The CLI posts only to `/send` — `cli/src/cmd/send_text.rs:412` on the mesh
relay path and `cli/src/cmd/send_text.rs:625` locally. A `codeterm send`
therefore opens no ledger entry, and `GET .../chat-send` will not list it.

Neither route promises that the recipient has *answered*. A reply comes back
through the return-address channel the sender was given (`codeterm send --tab
ID [--mesh PEER]`, or `codeterm agent report` for a Worker). Reading the
recipient's visible terminal text is not a delivery signal and must not be used
as one.

### Workspace routes

| Method | Path | Body | Description |
|---|---|---|---|
| GET | `/api/workspaces` | — | List all workspaces |
| POST | `/api/workspaces` | `{name, cwd?, provision?}` | Create workspace; `name` required (absent body: `400`). A `cwd` matching a workspace returns it with `existing: true`. `provision: true` (`400` with `cwd`) creates a new empty data-dir directory; agents started exactly there get directory trust. |
| POST | `/api/workspaces/{id}/focus` | — | Switch to workspace |
| POST | `/api/workspaces/{id}/rename` | `{name}` | Rename workspace |
| POST | `/api/workspaces/{id}/close` | `{force?}` or no body | Delete workspace. No body is the ordinary non-forced close. Agent tabs require `force: true`; without it the `403` response identifies the affected tabs and leaves the workspace open. |
| GET | `/api/workspaces/{id}/recently-closed` | — | List recently-closed tab entries for this workspace. Returns `ClosedTab[]` (see [Closed History & Restore](#closed-history--restore)). |
| POST | `/api/workspaces/{id}/reopen-closed` | `{index?}` or no body | Reopen a recently-closed tab (browser Ctrl+Shift+T). `index` selects the ring entry (0 = most recent, default). Returns `TabInfo {id, title, tab_id}`. |
| POST | `/api/workspaces/{id}/tabs` | `{eager?}` or no body | Create tab; no body leaves eager PTY creation disabled. |
| POST | `/api/workspaces/{id}/tabs/{tab_id}/rename` | `{name}` | Rename tab |
| POST | `/api/workspaces/{id}/tabs/{tab_id}/move` | `{to_workspace_id}` | Move tab between workspaces |

#### Permission Hook (POST /api/tabs/{id}/hook/permission)

Called by Claude Code hooks (or any agent integration) to notify CodeTerm that the agent is showing a permission prompt. This is a faster path than PTY auto-detection and bypasses the cooldown period.

**Two modes:**

1. **Explicit prompt** -- provide `question` and `options` in the body. CodeTerm uses them directly.
2. **Auto-parse** -- send no body or `{}`. CodeTerm reads the terminal screen content and runs structural fingerprint detection to extract the prompt.
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

Response: `{"detected": true}` or `{"detected": false}`. Even when `detected: false`, the hook still fires a `agent.permission_request` webhook event (the hook firing itself signals the agent is waiting).

#### Stop Hook (POST /api/tabs/{id}/hook/stop)

Called by Claude Code hooks (Stop matcher) to notify CodeTerm that the agent has completed its task. No body is accepted. Triggers `agent.task_complete` webhook event and updates tab status. For protected tabs (e.g. General Agent), the stop hook does NOT clear `agent_type` — the tab retains its agent identity.

```bash
curl -X POST "http://host:7685/api/tabs/$TAB/hook/stop" \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $CODETERM_API_TOKEN" -d '{}'
```

#### Session State Hook (POST /api/tabs/{id}/hook/session_state)

Provider-reported live turn state: `{event, provider_id?, session_id?, binding_nonce?}`. The `event` is resolved against the reporting provider's declared `hookEventRoster`, so an event the provider never declared changes nothing, and a binding nonce that disagrees with the tab's launch nonce is rejected with 409. A binding on this route carries a `working` or `idle` verdict — OpenCode declares `session.status` as Working and `session.idle` as Idle. Core reads the declaration, never a provider id.

#### SessionStart Hook (POST /api/tabs/{id}/hook/session_start)

Called by Claude Code's `SessionStart` matcher every time Claude opens a session — fresh launch, `--resume`, `/new`, or `/clear` inside the TUI. No body is accepted for a status-only report. CodeTerm extracts `session_id` from the JSON Claude pipes to the hook on stdin and persists it on the tab, so the next restart resumes the right thread instead of the stale id from the previous session. When the id changes, the prior session becomes a persisted context epoch: Chat keeps its transcript visible behind a labelled divider while archived turns are excluded from the provider's new context.

```bash
# Claude pipes its hook payload on stdin; the installed hook forwards it verbatim.
curl -X POST "http://host:7685/api/tabs/$TAB/hook/session_start" \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $CODETERM_API_TOKEN" \
  --data-binary @-
```

Response: `{"ok": true, "changed": true}` when the persisted id changed, `{"ok": true, "changed": false}` when the same id was already on the tab, and `{"ok": true, "deferred": true, ...}` when a fileless session is held until its reported transcript path emits a filesystem materialization event. Each tab holds at most one deferral; a newer refusal replaces it, and any legitimate intervening binding cancels it. `{"ok": false, "reason": "no session_id in body"}` means the body lacks a recognizable `session_id` field.

#### Delayed Chat messages

| Method | Path | Body | Description |
|--------|------|------|-------------|
| GET | `/api/tabs/{id}/delayed-messages` | — | List queued and failed delayed Chat UI messages for this tab |
| POST | `/api/tabs/{id}/delayed-messages` | `{text, scheduledAtMs}` | Queue a message for future delivery to this tab. Normal send is unaffected. |
| PATCH | `/api/delayed-messages/{id}` | `{text?, scheduledAtMs?}` | Edit a queued or failed delayed message |
| DELETE | `/api/delayed-messages/{id}` | — | Delete a queued or failed delayed message |
| POST | `/api/delayed-messages/{id}/send-now` | — | Deliver a queued or failed delayed message immediately |

#### Session Rebind (POST /api/tabs/{id}/session/rebind)

User-facing recovery is `codeterm tab rebind SESSION_ID --tab ID`. It binds the named existing provider session through the same validation as `SessionStart`, without restarting or interrupting the agent. The response names `found_session_id`, `requested_session_id`, and whether the binding changed. A missing session cannot evict a materialized session and returns a conflict naming both ids.

`GET /api/tabs/{id}/session/binding-state` reads the tab-scoped fact the daemon already holds rather than inferring one: `healthy`, `deferred_pending`, `refused`, `unbound`, or `stale_pending`, with the current and requested session identities when known. `healthy` is the ordinary answer for almost every tab and carries no registry entry, so an absent entry alone never means trouble. Repair: `codeterm docs API-REPAIR`. Adopt a live foreign PTY as an agent send target: `codeterm docs API-ADOPT`.

#### Hook auth

`/api/tabs/*/hook/*` and `/api/mem/flush` require the standard Bearer token — there is no auth-exempt loopback path. CodeTerm injects `CODETERM_API_TOKEN` into every PTY (alongside `CODETERM_API_PORT` and `CODETERM_TAB_ID`), and the installed Claude hook commands reference `${CODETERM_API_TOKEN}` directly. External callers wiring custom hooks must set the same header.

#### File serving
| Method | Path | Body | Description |
|--------|------|------|-------------|
| GET | `/api/fs/list-dir` | `?path=...` | List a local directory |
| POST | `/api/fs/mkdir` | `{path}` | Create a local directory |
| GET | `/api/serve-file` | `?path=...` | Serve a local file (assets, screenshots, etc.) |

#### Notifications
| Method | Path | Body | Description |
|--------|------|------|-------------|
| GET | `/api/notifications` | `?unread=true` | List notifications |
| POST | `/api/notifications` | `{title, body?, severity?, actions?, sticky?, tab_id?, level?, event_key?, target_tab_id?, chat_message_id?}` | Create notification (+ desktop toast when the level gate admits it). Navigation provenance is daemon-resolved: `X-CodeTerm-Tab-Id` outranks a body `tab_id`, an id naming no live tab is dropped (the notification still lands, without a Show Tab target), and `tab_title`/`workspace_id` come from the multiplexer. Mesh-relayed calls lose every tab claim and carry the signed peer name instead. `target_tab_id` and `chat_message_id` name the exact chat message a Needs you row jumps to; they are navigation hints stored as sent and emitted back on the record, never provenance. Blank values are dropped, a message id without a target tab is dropped, and a mesh-relayed call keeps neither. A gone tab or message renders the row unavailable. `level` is the closed set `verbose` / `action_needed` / `failure`; anything else is `400`. It gates the interrupting sinks: a notification naming no level is stored and reaches the panel but raises no desktop toast and no push. Desktop and every push channel are opt-in. The panel is not gated at all — it is the complete log, and every record reaches it live. Each interrupting destination follows `notifications.agent_levels` (desktop, then one key per push-channel service id; missing means closed); desktop also follows `notifications.desktop` as master. The row's `bell` field is inert, named only so flattened channel keys never read it as a channel. The row's `sound` field is named for the same reason but is NOT inert: it is whether that level chimes in the app, `true`/`false` is the user's answer and an omitted key falls back to what shipped (`action_needed` and `failure` chime, `verbose` does not), with `notifications.sound` as the master mute over all of them. The desktop toast additionally requires the OS permission CodeTerm asks for when the surface is switched on: without a grant, and whenever the native send itself fails, the toast is not claimed delivered and the in-app fallback window carries the notification instead. A level with every column closed is still recorded. `notifications.show_bell` is presentation only: it hides the indicator without changing what is recorded — but when it is off AND no toast is admitted AND no push channel carries the record, the response adds `unseen: {closed:[{surface,reason}]}` naming why each surface is shut. The response is still `200` with its `id`; the key is absent whenever any surface is open. `severity` (`info`/`warning`/`error`) is toast styling only. Push outcome is never returned; an opted-in channel that cannot carry the message logs and marks the record failed. `actions` is the CALLER-facing closed set — `{type:"url",label,value}`, `{type:"dismiss",label}`, `{type:"answer",label}`; an unknown kind or an unknown field is refused at parse. There is deliberately no caller `invoke` kind: `invoke` actions are constructed by the daemon alone, name a command from a fixed daemon allowlist (`{type:"invoke",label,command,args?}`, where `command` is currently only `respond_provider_update_prompt`), and are emitted on read but never accepted on write — an `invoke` in a request body is `400` on every transport. A stored row carrying an `invoke` outside that allowlist loads with the action stripped and the message kept. `answer` is the only kind an agent may offer: it carries a label and nothing else, and a notification carrying one records the resolved asking tab at creation. Blank or duplicate choice labels are `400`. |
| POST | `/api/notifications/{id}/answer` | `{label}` | Record the user's choice. The label must be one the notification offered; the answer goes to the tab captured when the question was sent, never to a tab resolved now. Exactly one answer per notification: a later call returns the standing `{answered}` unchanged. The recorded answer is then carried to that tab as an ordinary host message through the existing envelope, exactly once. The response is `{answered, delivered}` (plus `deliveryError` when the send was refused): recording an answer and carrying it back are separate facts, so a refused send reads as `delivered:false` on the record rather than as answered-and-delivered. `409` when the label was never offered or the asking tab is gone. |
| POST | `/api/notifications/clear` | — | Clear all |
| GET | `/api/notifications/channels` | — | `{channels:[{id,label,control_available}]}` — the registered push channels, in registry order. `id` is the bridge service id, `label` is the settings column caption, and `control_available` says whether that channel's own controls may be offered (for Telegram: a live bridge supervisor AND a non-empty `telegram_allowed_users`). The settings grid draws its channel columns from this list, so a channel the registry gains appears without a client change. |

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

Custom themes are stored as `.toml` files in `%APPDATA%/codeterm/themes/` (Windows) or `~/.codeterm/themes/` (macOS/Linux).

#### Mesh Networking
| Method | Path | Body / Query | Description |
|--------|------|------|-------------|
| GET | `/api/mesh/ping` | — | Lightweight mesh liveness probe |
| GET | `/api/mesh/debug/sign` | — | Mint a signed mesh auth token for diagnostics |
| GET | `/api/mesh/status` | — | Mesh identity, peers, discovery status |
| GET | `/api/mesh/peers` | — | List all peers (paired + discovered) |
| POST | `/api/mesh/discover` | — | Trigger discovery, return discovered peers |
| POST | `/api/mesh/connect` | `{instance_id}` | Initiate pairing with a discovered peer (crypto handled server-side) |
| POST | `/api/mesh/approve` | `{request_id}` | Accept an incoming pairing request |
| POST | `/api/mesh/reject` | `{request_id}` | Reject and block an incoming pairing request |
| POST | `/api/mesh/disconnect` | `{instance_id}` | Unpair from a paired peer (notifies remote side) |
| POST | `/api/mesh/pair/request` | `PairingRequest` | Inbound pairing request; returns `{ok, instance_id}` so a manual target is promoted before acceptance |
| POST | `/api/mesh/pair/accept` | `PairingAccept` | Inbound acceptance for a pairing request |
| POST | `/api/mesh/pair/confirm` | `PairingConfirm` | Inbound pairing confirmation |
| POST | `/api/mesh/pair/reject` | `{request_id}` | Inbound pairing rejection |
| POST | `/api/mesh/introduce` | `{...}` | Introduce peers to each other |
| POST | `/api/mesh/sync` | `MeshSyncPayload` | Exchange mesh gossip/discovery state |
| POST | `/api/mesh/broadcast/start` | — | Start local discovery broadcast |
| POST | `/api/mesh/broadcast/stop` | — | Stop local discovery broadcast |
| POST | `/api/mesh/relay` | `{method, path, body}` | Relay an API call to a remote peer |
| POST | `/api/mesh/relay-to` | `{target_peer_id, method, path, body}` | Local browser gateway call. Sends a signed relay request without exposing the target token. Requires relay protocol version `1`. |
| POST | `/api/mesh/rpc` | `{method, params, request_id}` | Execute allowlisted plan/spec/task/team JSON-RPC through the peer's in-process dispatcher. Requires full peer access and deduplicates by request ID. |
| GET | `/api/mesh/relay-stream` | `?peer=<id>&path=<streamable>&since=<cursor>` | Browser→local byte-pipe of a peer's own `/api/events/stream` or `/api/output/stream`, cursor-resumed and un-timed. Peer unreachable → `504` `peer-unreachable`. |
| GET | `/api/mesh/relay-asset/{peer}/{*path}` | `?token=<LOCAL>` | Read-only, `plugins/{id}/ui/*`-scoped byte proxy for a peer's plugin-UI iframe. |
| DELETE | `/api/mesh/peers/{id}` | — | Remove a peer (used internally by disconnect broadcast) |
| POST | `/api/mesh/refresh` | — | Refresh local addresses and announce them to peers |
| POST | `/api/mesh/peers/add-manual` | `{address, port}` | Inject a `Manual` discovered peer by Tailscale IP / port (skips mDNS / UDP scan). Returns `{ok, instance_id}` and emits `mesh-state-changed`. |
| POST | `/api/mesh/peers/approve/{id}` | — | Path-style alias for `POST /api/mesh/approve` (`request_id` in URL). |
| POST | `/api/mesh/peers/reject/{id}` | — | Path-style alias for `POST /api/mesh/reject`. |
| POST | `/api/mesh/peers/disconnect/{id}` | — | Path-style alias for `POST /api/mesh/disconnect` (`instance_id` in URL). |
| POST | `/api/mesh/announce` | `PeerAnnounce` | Inbound peer announcement with addresses, `relay_protocol_version`, and optional typed `web_ui` endpoints |
| POST | `/api/mesh/announce/all` | `?to=peer_id?` | Announce local addresses to one peer or all peers |
| POST | `/api/mesh/peers/{id}/addr` | `{ip, port?}` | Override a paired peer's cached address |

**Mesh-disabled gate (503).** When `config.mesh.enabled = false`, the auth middleware short-circuits every `/api/mesh/*` path other than `/api/mesh/ping` with `503 Mesh networking is disabled`. The same 503 also fires for any caller authenticated via the mesh signed-token path (i.e. another peer relaying to us) — the local Bearer token still works. Toggle the config back on to restore mesh routing without restarting the daemon.

**Peer-ping scheduler.** A background scheduler (`mesh::ping_scheduler`) pings each paired peer's `/api/mesh/ping` every 20 s with a 4 s timeout. On any online/offline transition, the peer record's `online` flag flips, `last_seen` updates on coming back, and `mesh-state-changed` fires so the UI re-renders without polling.

**Relay streaming (`relay-stream` / `relay-asset`).** These let a browser reach a paired peer without knowing its address. `relay-stream` opens a signed mesh request to the peer's own `/api/events/stream` or `/api/output/stream` (the only two whitelisted paths) and copies response bytes straight through — never buffered, filtered, or reordered — so the cursor/overflow contract of the underlying stream carries over unmodified. `relay-asset` is scoped to read-only `plugins/{id}/ui/*` GETs (path traversal rejected) and exists because an `<iframe src>` can't send a `Bearer` header: the `.html` entry self-verifies the caller's LOCAL api token via the `?token=` query — exempted from the normal bearer middleware by an exact prefix allowlist in `auth.rs`'s `is_custom_auth` — and the daemon then reaches the peer with a server-minted signed mesh token that is never serialized back to the browser.

**Relay security.** Direct signed peer requests and `/api/mesh/relay` share one access-policy function. `/api/secrets/*` and `/api/mesh/debug/sign` are local-only for every peer access level. REST `GET /api/config` always returns `api.token: null`; the browser retains only its gateway token locally. Relay, relay-stream, and relay-asset reject peers whose announced `relay_protocol_version` is not `1` with `426 Upgrade Required`.

**Tauri IPC (Mesh):**
| Command | Params | Description |
|---------|--------|-------------|
| `mesh_set_access` | `instance_id, access_level` | Set access level for a paired peer. `access_level`: `full`, `read_only`, `agent_isolated`. |

The `connect`, `approve`, `reject`, and `disconnect` endpoints are high-level — they handle crypto, key exchange, and peer notification automatically. These mirror the Tauri IPC commands used by the UI. Management commands are local-only and cannot be relayed via `--mesh`.

**Flat peer model:** Paired = trusted. No crown, no king, no trust declarations. Each machine sets an **AccessLevel** (Full/ReadOnly/AgentIsolated) per peer locally via the `mesh_set_access` Tauri command. Default is Full. UDP discovery broadcasts every 30 seconds.

## Related documents

Memory item, graph, calibration, and worker endpoints: `codeterm docs API-MEMORY`.

<!-- revision: 30 -->
