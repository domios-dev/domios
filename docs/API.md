# CodeTerm API Reference

CodeTerm exposes three APIs for agent integration:
1. **REST API** — HTTP on port 7685 (configurable, binds to 0.0.0.0 by default)
2. **Socket API** — JSON-RPC v2 over named pipe (Windows) / Unix socket
3. **CLI** — `codeterm.exe` command-line tool

### Architecture: Unified Service Layer

All multiplexer operations (workspace/tab management, agent spawn, batch commands, etc.) are implemented in a single **ServiceContext** (`src-tauri/src/service.rs`). Both REST API routes and Tauri IPC commands delegate to this shared service layer, which ensures consistent behavior — PTY cleanup, event emission, hierarchy validation — regardless of the entry point. This means every operation available via REST is also available internally via Tauri commands, with identical semantics.

## REST API

Base URL: `http://<host>:7685/api`

**Public vs internal REST surfaces.** This reference documents agent-facing and
operator-facing REST routes. Browser mirror plumbing, debug-clock endpoints,
upload/PTY mirror routes, and other web-only internals are intentionally not part
of the public contract; see `src-tauri/src/api/web_routes.rs` and the route
registration in `src-tauri/src/api/routes.rs` when auditing those surfaces.

### Authentication
Optional Bearer token. Set in config:
```toml
[api]
host = "0.0.0.0"
port = 7685
token = "your-secret-token"
```
Then: `Authorization: Bearer your-secret-token`

### Endpoints

#### System
| Method | Path | Body | Description |
|--------|------|------|-------------|
| GET | `/api/health` | — | Health check. Returns `version`, `build` (git short hash), `name`, `platform`, `status`. |
| GET | `/api/identify` | — | Version, platform, features |
| GET | `/api/network` | — | Local IP, Tailscale IP, API URLs |
| GET | `/api/keepawake` | — | Get keep-awake status (see below) |
| POST | `/api/keepawake/start` | `{duration_secs?, display?}` | Start / restart keep-awake session |
| POST | `/api/keepawake/stop` | — | Stop keep-awake session |

The desktop REST listener has one monotonic runtime generation owner. A
generation is reported as `listening` only after it owns the real socket and a
real `GET /api/health` succeeds, its body is consumed, and the proof connection
closes. This completion boundary keeps same-port Windows restarts from racing a
lingering accepted health socket. Listener/server exit autonomously retires
that generation and creates exactly one replacement; bind ownership failure
waits for an explicit port-release or restart event instead of retrying on a timer.
The internal `get_rest_api_status` command returns `generation`, `phase`,
`diagnostic`, and `message`. Diagnostic values are `listener_exited`,
`bind_ownership_failed`, or `workers_blocked`; the last means the listener
remains bound but the four REST runtime workers cannot answer health.

Each successful outage transition also owns a monotonic recovery id. Recovery
events are queued, so two worker stalls recovered within one listener generation
each produce one mesh resync while observing an event twice produces none.

The representative worker-starvation cause is provider transcript parsing, not
the listener: session JSONL can be hundreds of MiB. REST chat/transcript routes
run the bounded 4 MiB tail parser on blocking workers, leaving all four REST
runtime workers available for health, tab, and relay requests.


#### Workspaces
| Method | Path | Body | Description |
|--------|------|------|-------------|
| GET | `/api/workspaces` | — | List all workspaces |
| POST | `/api/workspaces` | `{name, cwd?}` | Create workspace. When `cwd` matches an existing workspace, the existing workspace is returned with `existing: true`; no new workspace is created and the requested name is ignored. |
| POST | `/api/workspaces/{id}/focus` | — | Switch to workspace |
| POST | `/api/workspaces/{id}/rename` | `{name}` | Rename workspace |
| POST | `/api/workspaces/{id}/close` | — | Delete workspace |
| GET | `/api/workspaces/{id}/recently-closed` | — | List recently-closed tab entries for this workspace. Returns `ClosedTab[]` (see [Closed History & Restore](#closed-history--restore)). |
| POST | `/api/workspaces/{id}/reopen-closed` | `{index?}` | Reopen a recently-closed tab (browser Ctrl+Shift+T). `index` selects the ring entry (0 = most recent, default). Returns `TabInfo {id, title, tab_id}`. |
| POST | `/api/workspaces/{id}/tabs` | — | Create tab |
| POST | `/api/workspaces/{id}/tabs/{tab_id}/rename` | `{name}` | Rename tab |
| POST | `/api/workspaces/{id}/tabs/{tab_id}/move` | `{to_workspace_id}` | Move tab between workspaces |

#### Closed History & Restore

Closed tabs and workspaces are recorded in a persistent ring. Every close action is bucketed into a *group* — a `single` tab close, a `cascade` (orchestrator + its spawned subtree), or a `workspace` close. Groups are identified by an opaque `group_id` (ULID). The ring is bounded (`RECENTLY_CLOSED_CAP` entries); oldest entries are evicted first.

| Method | Path | Body / Query | Description |
|--------|------|------|-------------|
| POST | `/api/closed-groups/{group_id}/reopen` | — | Reopen an entire closed group by its `group_id` (from the `recently_closed` snapshot). Recreates the workspace if it no longer exists; if a live workspace already exists at the same `cwd` it is reused — no duplicate is created. Returns `{workspace_id, tab_id, tab_id}` for the activated tab. |

**Path-match restore behavior.** When reopening any group or orphaned tab that records a `cwd`, CodeTerm calls `find_workspace_by_cwd` (normalised via `paths::normalize_cwd` — trailing slashes and case-folded) before creating a new workspace. If a live workspace already points at the same folder the tab(s) are inserted there instead of spawning a duplicate. This applies to `reopen_closed_group`, `reopen_closed_tab`, and `mux_restore_orphaned_tab` (commit 6115030e).

**`ClosedTab` fields** (returned by `GET /api/workspaces/{id}/recently-closed`):

| Field | Type | Description |
|-------|------|-------------|
| `tab_id` | `string` | Original tab id |
| `title` | `string` | Tab title at close time |
| `description` | `string?` | Tab description at close time |
| `cwd` | `string?` | Working directory at close time |
| `backing` | `string` | `"pty"` or `"chat_backend"` |
| `agent_type` | `string?` | Agent provider at close time (e.g. `"claude"`) |
| `agent_session_id` | `string?` | Provider session id for resume |
| `model` | `string?` | Model in use at close time |
| `closed_at` | `u64` | Unix milliseconds |
| `workspace_id` | `string` | Workspace the tab belonged to when closed |
| `workspace_name` | `string` | Human-readable workspace name |
| `parent_tab_id` | `string?` | Parent tab in hierarchy (preserved for cascade restore) |

**`ClosedTabView` fields** — `ClosedTab` enriched for the sidebar. Returned in `mux_get_state` → `recently_closed[]` and in the `GET /api/mux/state` snapshot:

| Field | Type | Description |
|-------|------|-------------|
| *(all ClosedTab fields above)* | | Flattened via `#[serde(flatten)]` |
| `index` | `number` | Position in the per-workspace reopen ring (`0` = most recent) |
| `orphaned` | `bool` | `true` when the original workspace no longer exists |
| `group_id` | `string` | Opaque group identifier (ULID) — pass to reopen-group endpoints |
| `group_kind` | `string` | `"single"` \| `"cascade"` \| `"workspace"` |

**`mux_get_state` / `GET /api/mux/state` snapshot** now includes a top-level `recently_closed: ClosedTabView[]` field. The list is the union of all groups across all workspaces, sorted newest-first, with `orphaned` computed live against the current workspace set. Sidebar and web-mirror clients consume this field to render the closed-history tabl.

#### Tabs
| Method | Path | Body / Query | Description |
|--------|------|------|-------------|
| GET | `/api/tabs` | `?workspace_id=<id>&agents=<bool>` or `?all_workspaces=true` | List compact tab summaries. Defaults to the focused workspace; use the explicit inspect endpoint for full payloads. |
| GET | `/api/tabs/inspect` | `?tab_ids=<id>,<id>` | Return the full tab payload for the selected tab IDs only. |
| POST | `/api/tabs` | `{title?, description?, cwd?, workspace_id?, cmd?, eager?, chat_backend?, system_prompt?, preset?, model?}` | Create a tab. `description` is trimmed, empty clears it, and values over 200 chars are rejected. When `cmd` is provided, CodeTerm sends it to the fresh tab through the same verified send path used by `/api/tabs/{id}/send`, with Enter appended. When `chat_backend` names an installed `chatBackend` plugin id, an **open agent shell** tab is created instead of a PTY: `system_prompt` overrides the resolved preset's prompt, `preset` selects one (omit → the plugin's `defaultPreset`), `model` picks a model. These three are only meaningful with `chat_backend`. |
| POST | `/api/tabs/{id}/send` | `{text?, enter?, raw?, actions?: [{type:"key", key} \| {type:"delay", ms}], batch?: [{text, enter?, delay_ms?}], source_tab?}` | Send text or terminal actions to PTY. Fire-and-forget — returns `{ok:true}` as soon as bytes are written or queued. `actions` supports keys (`enter`, `esc`, arrows, `tab`, `backspace`, `delete`, `home/end`, `pageup/pagedown`, `space`, `ctrl-c`) and millisecond delays. Verification fields were removed in v1.2.3 (#101): TUI agents (Claude/Codex) intercept input and don't echo per-character, so the old `verify` flag couldn't deliver on its name. For agent request/response, a future `codeterm agent send` will hook into provider semantics. |
| POST | `/api/tabs/{id}/stop` | — | Stop the tab's PTY process |
| POST | `/api/tabs/{id}/exec` | `{command, timeout_ms?, strip_ansi?}` | Send command + wait for output |
| GET | `/api/tabs/{id}/output` | `?offset=N&strip_ansi=true` | Get output buffer (cursor-based) |
| GET | `/api/tabs/{id}/text` | `?formatted=false` | Visible terminal text (VT100-parsed, alternate buffer included). Served from the backend — no frontend roundtrip, works for hidden tabs. Response: `{"text": "...", "formatted": false, "tab_id": "..."}`. With `?formatted=true` the `text` contains raw ANSI escape codes preserving colors, cursor position, and cursor visibility (used on tab remount to restore TUI apps like Claude that hide the cursor via DECTCEM). |
| GET | `/api/tabs/{id}/usage` | — | Latest usage/cost counters recorded for this tab |
| GET | `/api/tabs/{id}/output/stream` | — | SSE real-time output stream |
| GET | `/api/agent-runs/{tab_id}/log` | — | Agent run log for a tab |
| POST | `/api/tabs/{id}/focus` | — | Focus tab |
| POST | `/api/tabs/{id}/close` | — | Close tab + kill PTY |
| POST | `/api/tabs/{id}/rename` | `{title}` | Rename tab |
| POST | `/api/tabs/{id}/describe` | `{description}` | Set or clear the tab description. Server trims whitespace, treats empty as clear, and rejects values over 200 chars. |
| POST | `/api/tabs/{id}/resize` | `{cols, rows}` | Resize terminal |
| GET | `/api/tabs/{id}/chat` | — | Chat snapshot for the tab |
| POST | `/api/tabs/{id}/chat/start` | — | Start chat JSONL tailing |
| POST | `/api/tabs/{id}/chat/stop` | — | Stop chat JSONL tailing |
| POST | `/api/tabs/{id}/chat/backfill` | `{before_offset, max_bytes?}` | Backfill older chat JSONL content |
| GET | `/api/tabs/{id}/delayed-messages` | — | List queued/failed delayed Chat UI messages for this tab |
| POST | `/api/tabs/{id}/delayed-messages` | `{text, scheduledAtMs}` | Queue a message for future delivery to this tab. Normal send is unaffected. |
| PATCH | `/api/delayed-messages/{id}` | `{text?, scheduledAtMs?}` | Edit a queued or failed delayed message |
| DELETE | `/api/delayed-messages/{id}` | — | Delete a queued or failed delayed message |
| POST | `/api/delayed-messages/{id}/send-now` | — | Deliver a queued or failed delayed message immediately |
| POST | `/api/tabs/{id}/hook/permission` | `{question?, options?: [{label, value}]}` | Notify that tab is showing a permission prompt (see below) |
| POST | `/api/tabs/{id}/hook/stop` | `{}` | Notify that the agent finished its turn (see below) |
| POST | `/api/tabs/{id}/hook/session_start` | *(Claude SessionStart JSON, forwarded from stdin)* | Persist the agent-reported `session_id` on the tab; covers `/new` and `/clear` inside the TUI (see below) |
| POST | `/api/tabs/{id}/agent/restart` | `{force?: bool, grace_secs?: u64}` | Restart the agent in this tab; resumes only when the recorded session is valid. See [Agent Restart](#agent-restart-post-apitabsidagentrestart). |

#### Chat attachment authority

The desktop `chat_start` command and web-mirror
`POST /api/tabs/{id}/chat-raw/start` return the same bounded snapshot shape.
`source_session_id` and `session_binding_nonce` are derived from the Tab and
form one authority generation. A client adopts snapshots and
`tab-chat-msg-{id}` deltas only when both values match the Tab's current
generation.

`pending=true` with null session authority and no subscription means the Tab
is still awaiting its own session; it is not attached and must not fall back to
another provider session. `pending=true` with matching session authority and a
subscription means the Tab is awaiting its own store. `pending=false` is a
complete bounded snapshot, including a complete empty snapshot. Visible Chat
owns one subscription, releases it when hidden, and obtains a fresh bounded
snapshot on reactivation or event-stream resync.

#### Agent Hierarchy
| Method | Path | Body / Query | Description |
|--------|------|------|-------------|
| POST | `/api/tabs/{id}/role` | `{role, parent_tab_id?, can_spawn?}` | Set tab role (`Orchestrator` or `Worker`; `Conductor` accepted as an alias for `Orchestrator`). |
| DELETE | `/api/tabs/{id}/role` | — | Clear role: detach a Worker from its Orchestrator, or orphan an Orchestrator's Workers |
| GET | `/api/tabs/{id}/status` | — | Tab status + role + children info |
| POST | `/api/tabs/{id}/report` | `{message}` | Send report to direct parent (cascade, not to Orchestrator). Auto-injected into parent's PTY. |
| GET | `/api/tabs/{id}/reports` | `?clear=true` | Read reports from children (optionally clearing them) |
| POST | `/api/tabs/{id}/can-spawn` | `{can_spawn: bool}` | Toggle whether tab can spawn children |
| POST | `/api/tabs/{id}/agent-type` | `{agent_type}` | Mark a tab as running a specific agent provider |
| POST | `/api/tabs/{id}/prompt/respond` | `{response}` | Respond to a pending permission prompt |
| POST | `/api/tabs/{id}/prompt/clear` | — | Clear a pending permission prompt |
| GET | `/api/agents/providers` | — | List registered agent providers **and open agent shells**. Each entry carries `id`, `display_name`, `available_models`, `default_model`, `reasoning_efforts` (declared launch-time effort levels — empty ⇒ no effort selector), `default_reasoning_effort`, `settings_schema`, etc. plus a **`kind`** field: `"agent"` (PTY providers — claude/codex/opencode) or `"shell"` (installed `chatBackend` plugins, e.g. lmstudio). Shell entries also carry `presets` and `supports_chat_ui: true`. Single source of truth for "what agents and shells does this CodeTerm instance know about" — frontend dropdowns, CLI `agent providers/models`, and cron/mem job dialogs all read from here. |
| GET | `/api/providers` | — | Public provider list alias for agent/provider selectors |
| POST | `/api/agents/spawn` | `{agent?, agent_name?, title?, description?, cwd?, role?, role_profile?, parent_tab_id?, can_spawn?, task?, args?, workspace_id?, model?, reasoning_effort? (alias `reasoning-effort`), system_prompt? (alias `system`), preset?, charter?, tick_interval_secs?, nudge_cooldown_secs?, max_actions_per_tick?, report_to?, origin_peer?}` | One-call agent spawn (see below). `description` follows the same trim/empty-clear/200-char validation as tab description. `reasoning_effort` is capability-validated against the provider's `reasoning_efforts`: an unsupported value is rejected with a structured error and **no agent launch** (never silently downgraded); the resolved effort and its `{planned,requested,resolved,actual}` provenance ride back on the spawn result. Routing is **by capability, not identity**: if `agent` matches a `chatBackend` shell id, the request opens an open agent shell (threading `system_prompt`/`preset`/`model` and watcher `mode`/`engine` fields into the shell's `openSession` ctx); otherwise the PTY agent path. `role` accepts `Orchestrator`, `Manager`, `Worker`, plus `Planner`/`Watcher` sugar that maps to `Manager` with `role_profile`. Watcher spawns require a watcher-capable `chatBackend` shell and `charter`; policy fields are clamped by the host. An unknown id errors with the available agent **and** shell ids. `report_to` sets where the worker's `agent report` lands (Tab id or `main`/`general` alias). `origin_peer` is **never client-supplied** — the mesh relay stamps it from the authenticated sender so a worker spawned via `agent spawn --mesh PEER` reports back across the mesh; see *Remote mesh workers*. |
| GET | `/api/remote-workers` | `?tab=<id>` | Orchestrator-side view of remote (cross-mesh) workers, populated from inbound mesh reports. With `?tab=` returns that orchestrator's workers; otherwise the full `{ tab_id: RemoteWorker[] }` map. `RemoteWorker = {peer, remote_tab, title, model, status, last_message, last_seen_ms}`. Live updates via the `remote-workers-changed` event. |

#### Interactive chat prompts (#162)

Chat messages may carry an `interactive` array of answerable prompt widgets parsed from the provider's session stream — populated via the `AgentProvider::parse_interactive_events` capability (native or JS `parseInteractiveEvents` + manifest `interactiveEvents` flag), never by branching on a provider id. It rides on both chat wire shapes: the `GET /chat` and `chat/start` snapshot (`RichMessage[]`) and the live `tab-chat-msg-{tabId}` events (`ChatMsg[]`, attached to the tool-call message). First element type:

```json
{ "type": "question", "id": "toolu_42-0", "toolUseId": "toolu_42",
  "prompt": "Which database?", "header": "Database",
  "options": [{ "label": "Postgres", "description": "Relational" }], "multiSelect": false }
```

The enum is open for future element types. The frontend renders each as a clickable widget and routes the answer back through the existing tab send path (`/api/tabs/{id}/send`).

#### Batch
| Method | Path | Body | Description |
|--------|------|------|-------------|
| POST | `/api/batch` | `{commands: [{method, params, id?}]}` | Execute multiple commands sequentially (see below) |

#### Pasteboard (cross-mesh ephemeral payloads, #98)
| Method | Path | Body | Description |
|--------|------|------|-------------|
| POST | `/api/pasteboard` | `{body, content_type?, ttl_ms?, pop_on_read?}` | Stash a payload. Returns `{id: "paste:NNNNNN", expires_at}`. TTL default 1h, max 24h. 20-entry cap with oldest-first eviction. |
| GET | `/api/pasteboard/{id}` | — | Fetch body. `?pop=true` deletes after read. Returns 404 if expired/unknown. |
| DELETE | `/api/pasteboard/{id}` | — | Explicit delete. |
| GET | `/api/pasteboard` | — | List local pastes (id, size, ttl_remaining_ms, content_type). |

No disk persistence — restart wipes every paste. Correct semantics for "drop a payload, get a slug, let any peer fetch it, then forget."

### Tab Fields

Full fields returned by `GET /api/tabs/inspect` for each selected tab:

| Field | Type | Description |
|-------|------|-------------|
| `id` | `string` | Unique tab ID (8-char hex, e.g. `a3f1b2c7`) |
| `pty_id` | `string` | Associated PTY process ID |
| `tab_type` | `string` | `"Terminal"` or `"Browser"` (canvas for visualizations) |
| `title` | `string` | Display title (user-pinned or auto-detected) |
| `description` | `string?` | Short tab summary, max 200 chars. Shown in the sidebar tooltip/subtitle and persisted in session snapshots. |
| `cwd` | `string?` | Current working directory (updated via OSC 7) |
| `url` | `string?` | URL for canvas tabs (localhost, data URLs) |
| `agent_type` | `string?` | Agent running in tab, e.g. `"claude"` |
| `claude_session_id` | `string?` | Linked Claude Code session ID for resume |
| `role` | `string?` | `"Orchestrator"`, `"Worker"`, or `null` |
| `parent_tab_id` | `string?` | Parent tab ID in hierarchy, or `null` |
| `can_spawn` | `bool` | Whether this tab can spawn children. Orchestrator: true (default). Worker: always false. |
| `connected_services` | `string[]` | External services connected to this tab (e.g. `["telegram"]`). Shown as icons in sidebar. |
| `depth` | `number` | Hierarchy depth: 0 (no role), 1 (Orchestrator), 2 (Worker under Orchestrator) |

#### Permission Hook (POST /api/tabs/{id}/hook/permission)

Called by Claude Code hooks (or any agent integration) to notify CodeTerm that the agent is showing a permission prompt. This is a faster path than PTY auto-detection and bypasses the cooldown period.

**Two modes:**

1. **Explicit prompt** -- provide `question` and `options` in the body. CodeTerm uses them directly.
2. **Auto-parse** -- send an empty body `{}`. CodeTerm reads the terminal screen content and runs structural fingerprint detection to extract the prompt.

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

Called by Claude Code hooks (Stop matcher) to notify CodeTerm that the agent has completed its task. Triggers `agent.task_complete` webhook event and updates tab status. For protected tabs (e.g. General Agent), the stop hook does NOT clear `agent_type` — the tab retains its agent identity.

```bash
curl -X POST "http://host:7685/api/tabs/$TAB/hook/stop" \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $CODETERM_API_TOKEN" -d '{}'
```

#### SessionStart Hook (POST /api/tabs/{id}/hook/session_start)

Called by Claude Code's `SessionStart` matcher every time Claude opens a session — fresh launch, `--resume`, `/new`, or `/clear` inside the TUI. CodeTerm extracts `session_id` from the JSON Claude pipes to the hook on stdin and persists it on the tab, so the next restart resumes the right thread instead of the stale id from the previous session.

```bash
# Claude pipes its hook payload on stdin; the installed hook forwards it verbatim.
curl -X POST "http://host:7685/api/tabs/$TAB/hook/session_start" \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $CODETERM_API_TOKEN" \
  --data-binary @-
```

Response: `{"ok": true, "changed": true}` when the persisted id changed, `{"ok": true, "changed": false}` when the same id was already on the tab (no-op so per-turn pings don't churn the snapshot), `{"ok": false, "reason": "no session_id in body"}` when the body lacks a recognizable `session_id` field.

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
| POST | `/api/notifications` | `{title, body?, level?}` | Create notification (+ OS toast if unfocused) |
| POST | `/api/notifications/clear` | — | Clear all |

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
| GET | `/api/mesh/web-token` | — | Mint a short-lived browser token for this daemon's web mirror |
| GET | `/api/mesh/peers/{id}/web-token` | — | Ask a paired peer for a short-lived web token and browser host |
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
| POST | `/api/mesh/rpc` | `{method, params, request_id}` | Execute allowlisted plan/spec/task/team JSON-RPC through the peer's in-process dispatcher. Requires full peer access and deduplicates by request ID. |
| GET | `/api/mesh/relay-stream` | `?peer=<id>&path=<streamable>&since=<cursor>` | Browser→local byte-pipe of a peer's own `/api/events/stream` or `/api/output/stream`, cursor-resumed and un-timed. Peer unreachable → `504` `peer-unreachable`. |
| GET | `/api/mesh/relay-asset/{peer}/{*path}` | `?token=<LOCAL>` | Read-only, `plugins/{id}/ui/*`-scoped byte proxy for a peer's plugin-UI iframe. |
| DELETE | `/api/mesh/peers/{id}` | — | Remove a peer (used internally by disconnect broadcast) |
| POST | `/api/mesh/refresh` | — | Refresh local addresses and announce them to peers |
| POST | `/api/mesh/peers/add-manual` | `{address, port}` | Inject a `Manual` discovered peer by Tailscale IP / port (skips mDNS / UDP scan). Returns `{ok, instance_id}` and emits `mesh-state-changed`. |
| POST | `/api/mesh/peers/approve/{id}` | — | Path-style alias for `POST /api/mesh/approve` (`request_id` in URL). |
| POST | `/api/mesh/peers/reject/{id}` | — | Path-style alias for `POST /api/mesh/reject`. |
| POST | `/api/mesh/peers/disconnect/{id}` | — | Path-style alias for `POST /api/mesh/disconnect` (`instance_id` in URL). |
| POST | `/api/mesh/announce` | `PeerAnnounce` | Inbound peer address announcement |
| POST | `/api/mesh/announce/all` | `?to=peer_id?` | Announce local addresses to one peer or all peers |
| POST | `/api/mesh/peers/{id}/addr` | `{ip, port?}` | Override a paired peer's cached address |

**Mesh-disabled gate (503).** When `config.mesh.enabled = false`, the auth middleware short-circuits every `/api/mesh/*` path other than `/api/mesh/ping` with `503 Mesh networking is disabled`. The same 503 also fires for any caller authenticated via the mesh signed-token path (i.e. another peer relaying to us) — the local Bearer token still works. Toggle the config back on to restore mesh routing without restarting the daemon.

**Peer-ping scheduler.** A background scheduler (`mesh::ping_scheduler`) pings each paired peer's `/api/mesh/ping` every 20 s with a 4 s timeout. On any online/offline transition, the peer record's `online` flag flips, `last_seen` updates on coming back, and `mesh-state-changed` fires so the UI re-renders without polling.

**Relay streaming (`relay-stream` / `relay-asset`).** These let a browser reach a paired peer without knowing its address. `relay-stream` opens a signed mesh request to the peer's own `/api/events/stream` or `/api/output/stream` (the only two whitelisted paths) and copies response bytes straight through — never buffered, filtered, or reordered — so the cursor/overflow contract of the underlying stream carries over unmodified. `relay-asset` is scoped to read-only `plugins/{id}/ui/*` GETs (path traversal rejected) and exists because an `<iframe src>` can't send a `Bearer` header: the `.html` entry self-verifies the caller's LOCAL api token via the `?token=` query — exempted from the normal bearer middleware by an exact prefix allowlist in `auth.rs`'s `is_custom_auth` — and the daemon then reaches the peer with a server-minted signed mesh token that is never serialized back to the browser.

**Tauri IPC (Mesh):**
| Command | Params | Description |
|---------|--------|-------------|
| `mesh_set_access` | `instance_id, access_level` | Set access level for a paired peer. `access_level`: `full`, `read_only`, `agent_isolated`. |

The `connect`, `approve`, `reject`, and `disconnect` endpoints are high-level — they handle crypto, key exchange, and peer notification automatically. These mirror the Tauri IPC commands used by the UI. Management commands are local-only and cannot be relayed via `--mesh`.

**Flat peer model:** Paired = trusted. No crown, no king, no trust declarations. Each machine sets an **AccessLevel** (Full/ReadOnly/AgentIsolated) per peer locally via the `mesh_set_access` Tauri command. Default is Full. UDP discovery broadcasts every 30 seconds.

#### Memory
| Method | Path | Body / Query | Description |
|--------|------|------|-------------|
| GET | `/api/mem/search` | `?q=...` | Search memory items |
| POST | `/api/mem/items` | `{...}` | Save a memory item |
| GET | `/api/mem/items/{id}` | — | Get one memory item |
| PATCH | `/api/mem/items/{id}` | `{...}` | Update one memory item |
| DELETE | `/api/mem/items/{id}` | — | Delete one memory item |
| GET | `/api/mem/items/{id}/history` | — | Read item history |
| GET | `/api/mem/pinned` | — | List pinned memory items |
| GET | `/api/mem/taxonomy` | — | Read memory taxonomy |
| GET | `/api/mem/tail` | — | Tail recent memory events |
| DELETE | `/api/mem/forget/{area}` | — | Forget all memory in an area |
| GET | `/api/mem/wake-up` | — | Agent wake-up memory context |
| GET | `/api/mem/stats` | — | Memory store statistics |
| POST | `/api/mem/reembed` | — | Rebuild embeddings |
| POST | `/api/mem/learn` | `{...}` | Learn from agent/user context |
| POST | `/api/mem/promote/{id}` | — | Promote a memory item |
| POST | `/api/mem/flush` | — | Flush pending memory writes |
| POST | `/api/mem/check-duplicate` | `{...}` | Check for duplicate memory |
| POST | `/api/mem/dream` | `{...}` | Run memory consolidation |
| POST | `/api/mem/extract-pending` | `{...}` | Extract pending memories |
| GET | `/api/mem/dream/report` | — | Latest memory consolidation report |
| POST | `/api/mem/sync` | `{...}` | Sync memory state |
| GET | `/api/mem/sync/status` | — | Memory sync status |
| POST | `/api/mem/graph/entities` | `{...}` | Create graph entities |
| POST | `/api/mem/graph/relate` | `{...}` | Create graph relations |
| GET | `/api/mem/graph/related` | `?id=...` | Related graph entities |
| POST | `/api/mem/graph/invalidate` | `{...}` | Invalidate graph cache |
| GET | `/api/mem/graph/timeline` | — | Graph timeline |
| GET | `/api/mem/graph/traverse` | `?id=...` | Traverse graph neighborhood |
| GET | `/api/mem/graph/path` | `?from=...&to=...` | Find graph path |
| GET | `/api/mem/graph/tunnels` | — | Graph tunnel candidates |
| POST | `/api/mem/graph/alias` | `{...}` | Add graph alias |
| GET | `/api/mem/graph/full` | — | Full graph view |
| GET | `/api/mem/graph/labels` | — | Graph labels |
| GET | `/api/mem/graph/edges` | — | Graph edges |
| GET | `/api/mem/graph/snapshot` | — | Compressed graph snapshot |
| POST | `/api/mem/graph/layout/rebuild` | — | Rebuild graph layout |
| POST | `/api/mem/graph/repair/find-relations` | `{...}` | Find relation repair candidates |
| POST | `/api/mem/graph/repair/delete-relations` | `{...}` | Delete relation repair candidates |
| POST | `/api/mem/graph/repair/merge-relations` | `{...}` | Merge relation repair candidates |
| POST | `/api/mem/graph/repair/find-entities` | `{...}` | Find entity repair candidates |
| POST | `/api/mem/graph/repair/augment-entity` | `{...}` | Augment graph entity |
| POST | `/api/mem/graph/repair/delete-entity` | `{...}` | Delete graph entity |
| POST | `/api/mem/graph/repair/orphan-vectors` | `{...}` | Repair orphan vectors |
| POST | `/api/mem/graph/repair/orphan-blobs` | `{...}` | Repair orphan blobs |
| GET | `/api/mem/pinned-positions` | — | List pinned graph positions |
| PATCH | `/api/mem/pinned-positions/{mem_id}` | `{...}` | Set pinned graph position |
| DELETE | `/api/mem/pinned-positions/{mem_id}` | — | Delete pinned graph position |
| GET | `/api/mem/activity/recent` | — | Recent memory activity |
| POST | `/api/mem/calibration/sweep` | `{...}` | Run memory calibration sweep |
| GET | `/api/mem/calibration/log` | — | Read calibration log |
| GET | `/api/mem/workers` | — | List memory workers |
| POST | `/api/mem/workers` | `{...}` | Spawn memory worker |
| GET | `/api/mem/workers/{id}` | — | Get memory worker status |
| DELETE | `/api/mem/workers/{id}` | — | Kill memory worker |
| POST | `/api/mem/workers/{id}/progress` | `{...}` | Submit worker progress |
| POST | `/api/mem/workers/{id}/report` | `{...}` | Submit worker report |
| GET | `/api/mem/workers/{id}/log` | — | Read worker log |

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

#### Usage Tracker

Per-provider usage snapshots scraped from the running agent (Claude `/usage`, Codex `/status`). The poller runs a hidden background tab, observes the provider's own usage screen, and persists `{used_pct, weekly_pct, weekly_opus_pct, ts_ms, resets_at, weekly_resets_at, account_id}` rows. Drives the in-app Usage bubble + history modal; everything is local — nothing is reported upstream.

| Method | Path | Body / Query | Description |
|--------|------|------|-------------|
| GET | `/api/usage/latest` | `?provider=claude&account_id=…` | Latest snapshot for one provider/account. `null` when no data yet. |
| GET | `/api/usage/history` | `?provider=…&since=<ms>&limit=200&account_id=…` | Raw snapshots since `since` (epoch ms). `limit` clamped to `[1, 5000]`. |
| GET | `/api/usage/rollup/daily` | `?provider=…&since=<ms>&account_id=…` | Per-day max-used aggregates for the history chart. |
| GET | `/api/usage/projection` | `?provider=…&account_id=…` | Median-baseline projection: per-DOW shape + scale + overspend ETA. |
| POST | `/api/usage/refresh` | `{provider, account_id?}` | Trigger an out-of-band probe right now. Returns `{queued, probe_id}`; `503` if the poller isn't running. |

**Adaptive polling.** A single background scheduler ticks every 30 s and decides which (provider, account) is due to probe. The base cadence steps through `[120 s, 300 s, 600 s, 1200 s, 2400 s]` (`BACKOFF_STEPS_S` in `src-tauri/src/usage/poller.rs`) with ±10% jitter at the top — dense while a tab for that provider is active, then stretches out the longer the user is idle. A tab is "active" when its activity timestamp is fresh (`TabActivity::mark` on keypress / send / focus). After `PAUSE_AFTER_FAILURES = 3` consecutive probe failures the account is paused until manual refresh.

**Projection algorithm.** `src-tauri/src/usage/projection.rs` builds per-day-of-week medians from the last 4 weeks of snapshots, projects today's curve from the elapsed shape, and rejects "pair-noise" jumps wider than `SHAPE_PAIR_GAP_MS = 4h` apart with `SHAPE_PAIR_DELTA_MAX = 25 %` jump — typical of off-device usage gaps where two distant samples bracket someone else's activity on the same account. The frontend uses `activeWeeklyCycle()` in `src/components/Usage/cycle.ts` to discard stale `weekly_resets_at` values that lag the latest `resets_at`.

#### Logs

CodeTerm pipes the daemon's `tracing` output into a structured ring + SQLite store (`src-tauri/src/logs/`). The UI's Settings → Logs tab is the canonical surface; the REST shape is identical.

| Method | Path | Body / Query | Description |
|--------|------|------|-------------|
| GET | `/api/logs` | `?level=info\|warn\|error\|debug&source=<substr>&since=<ms>&limit=500` | Recent app-log rows. `limit` clamped to `[1, 5000]`. |
| DELETE | `/api/logs` | `?before=<ms>` | Purge rows older than `before` (epoch ms). Returns `{deleted: N}`. |

Each row is `{id, ts, level, source, message, context_json}`. `source` is the `tracing` target (e.g. `codeterm::mem::dreamer`, `codeterm::usage::poller`). Storage is bounded by a 1000-entry in-memory ring; once the SQL backend opens (`logs/app_logs.db` under the data dir, WAL mode) the ring drains into it. The `log-emitted` Tauri event fires per-record so the Logs tab can auto-tail without polling.

#### Setup (Rules, Hooks)
| Method | Path | Body | Description |
|--------|------|------|-------------|
| GET | `/api/setup/rules` | — | Check if project rules are installed |
| POST | `/api/setup/rules/install` | — | Install project rules |
| GET | `/api/setup/hooks` | — | Check Claude hooks status |
| POST | `/api/setup/hooks/install` | — | Install Claude hooks |
| POST | `/api/setup/hooks/uninstall` | — | Uninstall Claude hooks |

#### Services

Daemon-kind services are registered here; cron jobs are projected as virtual `cron`-kind services but managed via `/api/crons`. Only `daemon` can be created through this API.

`POST /api/tabs/{id}/send` accepts `Authorization: Bearer <service_token>` (the per-service push token returned at create or rotation time) in addition to the normal access token. This lets a registered daemon write to a tab without holding the global access token.

| Method | Path | Body / Query | Description |
|--------|------|------|-------------|
| GET | `/api/services` | `?kind=cron\|daemon&enabled=true\|false` | List all services (cron projection + registered daemons), optionally filtered |
| POST | `/api/services` | `{name, kind:"daemon", health_mode, health_interval?, push_enabled?, url?, host?, port?, process_label?, script_path?, health_path?, health_json_field?, health_ok_values?, health_degraded_values?, description?, field_mapping?}` | Register a new daemon-kind service. `kind` other than `daemon` returns 400 (cron jobs go through `/api/crons`). Returns `{service, token}` — save the token, it is shown only once. `health_interval` defaults to 30 s. `push_enabled` defaults to false; set true to allow Bearer-token writes to tabs. |
| GET | `/api/services/{id}` | — | Get service by ID. Body includes the latest health snapshot (joined from `service_health` via `ROW_NUMBER` window — most recent row only). |
| PUT | `/api/services/{id}` | `{name?, description?, url?, host?, port?, process_label?, script_path?, health_mode?, health_path?, health_json_field?, health_ok_values?, health_degraded_values?, health_interval?, field_mapping?, push_enabled?, enabled?}` | Update service fields. Each field is a sparse patch (only provided keys are written). |
| DELETE | `/api/services/{id}` | — | Delete service + its push token + activity |
| POST | `/api/services/{id}/regenerate-token` | — | Rotate push token. Returns `{token, push_enabled: true}` — old token is immediately invalidated, and `push_enabled` is auto-set to true (rotating implies you intend to push). |

**Health modes:** `http` (HTTP GET to `url` + optional `health_path`, default `/healthz`), `tcp` (TCP connect to `host:port`), `process` (launchctl/systemd unit via `process_label`), `script` (exit 0 = healthy, `script_path`). The probe scheduler ticks every 30 s and requires **3 consecutive identical observations** before firing a transition event — single-tick blips never page the UI.

**HTTP health response mapper** (per-service, all optional):
- `health_json_field` — which JSON key to inspect (default: `"status"`). Ignored when body is empty or non-JSON (2xx alone → Healthy).
- `health_ok_values` — string array of values that mean Healthy, case-insensitive (default: `["ok","healthy","up"]`). Example: `["UP"]` for SonarQube, `["running"]` for custom services.
- `health_degraded_values` — values that mean Degraded (default: `["degraded","starting"]`). Anything else → Down.
- If the configured field is absent from the JSON body, the probe falls back to "2xx = Healthy".
- CLI: `--health-json-field status --health-ok-values UP,ok --health-degraded-values STARTING`

**Push-token middleware.** `POST /api/tabs/{id}/send` accepts `Authorization: Bearer <service_token>` (the per-service push token returned at create or rotation time) in addition to the normal API token. Token files live at `<data_dir>/services/<service_id>/token` (mode `0600` on Unix), are 64-hex-char random, and are matched constant-time. Every accepted push is mirrored into the activity log as a `push` event.

**Tauri events:**
- `service-health-changed` — `{service_id, status}` — fired by the health probe scheduler on confirmed status transitions only (3-tick debounced).
- `service-pushed` — `{service_id, tab_id, body_preview}` — fired when a daemon writes to a tab via its push token. Also fired by the mesh-relay path when a paired peer relays a `/send` to us, so the activity log reflects cross-peer push traffic the same way as local pushes.
- `remote-workers-changed` — `{orchestrator_tab_id, workers}` — fired when an inbound mesh report updates the remote-worker registry; `workers` is the full `{ tab_id: RemoteWorker[] }` map. Drives the sidebar's remote tab ghost nodes. See `GET /api/remote-workers`.

#### Activity Log

Every service push, health transition, peer event, and cron lifecycle event is recorded in the bounded activity log (sqlite, capped at 10 000 rows / 30 days, evicted every 100 inserts).

| Method | Path | Body / Query | Description |
|--------|------|------|-------------|
| GET | `/api/activity` | `?source=push\|health\|peer\|cron&since=<ms>&limit=<N>` | List recent activity events newest-first. `source` filters by event source; `since` filters to `occurred_at > since` (unix ms); `limit` defaults to 500. |

Each event: `{id, occurred_at, source, source_id?, target?, source_peer?, level: info\|warn\|error, message, meta}`.

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

**Workspace resolution:** The CLI `cron create` command uses `$CODETERM_WORKSPACE_ID` by default (the agent's own workspace), falling back to the first workspace if the env var is not set. Override with `--workspace ID`.

**Updating cron jobs:** Use `PUT /api/crons/:id` (CLI: `codeterm cron edit ID`) to modify a cron job in-place. Do NOT delete and recreate -- that loses run history and changes the ID.

**Deleting a cron job** cascades: kills all running tabs/PTYs for that job, closes their tabs, and removes the entire log directory (`~/.codeterm/crons/<job_id>/`).

**Tauri events:**
- `cron-state-changed` — emitted on any cron CRUD operation
- `cron-fired` — emitted when a cron fires, payload: `{cron_id, run_id, tab_id, tab_id}`

#### Memory
| Method | Path | Body / Query | Description |
|--------|------|------|-------------|
| GET | `/api/mem/search` | `?q=...&kind=...&area=...&scope=...&limit=N&mode=hybrid_rrf\|vector_first_rerank&weight=0.30&rerank=true` | Hybrid retrieval. Default mode is `hybrid_rrf` (FTS5+vector RRF fusion). `mode=vector_first_rerank` switches to the a reference memory system-style pipeline (vector recall + keyword/temporal rerank); `weight` tunes its keyword-overlap strength (0.0..=1.0, default 0.30). `rerank=true` layers an LLM second-pass on top. |
| GET | `/api/mem/items/{id}` | — | Get one item (full body). On real (non-worker) reads, `evidence_count` is bumped for kinds `skill | lesson | rule | fact | preference`. Bump is suppressed when the request carries `X-CodeTerm-Worker-Id` so internal sweeps (dreamer, extraction) don't poison the score. |
| GET | `/api/mem/pinned` | `?limit=N` | List items with `is_pinned_wakeup = 1`, ordered by `importance DESC, evidence_count DESC, updated_at DESC`. `limit` defaults to 100, hard-capped at 500. |
| POST | `/api/mem/items` | `{kind, title, body, scope?, summary?, area?, tags?, source?, importance?, replace?, skip_extract?, source_path?, pinned?}` | Save a mem item. `pinned: true` atomically sets `is_pinned_wakeup = 1` in the same INSERT (no follow-up PATCH). `scope` accepts `"global"` (default) or `"workspace:<id>"`. `skip_extract: true` suppresses the auto-extraction cascade (seed loaders use this to avoid re-extracting bundled items on every startup). |
| PATCH | `/api/mem/items/{id}` | `{title?, summary?, area?, tags?, body?, importance?, pinned?, changed_by?}` | Partial update. `pinned: true|false` toggles `is_pinned_wakeup`; the change is recorded in `mem_item_history` so the audit trail captures explicit pin intent. `changed_by` is an optional audit actor; the server trims and sanitizes it, and falls back to the actor headers when omitted or blank. |
| DELETE | `/api/mem/items/{id}` | — | Delete an item |
| GET | `/api/mem/wake-up` | `?budget=N&l0_only=true&no_l0=true&workspace=<ws>&explain=true` | L0 + L1 critical-context digest. `workspace` boosts items scoped to that workspace (`scope = "workspace:<ws>"`) by `+0.5` in the L1 score and admits them alongside global rows. `explain=true` appends a per-item score breakdown comment block. L1 budget defaults to `[memory] l1_budget_tokens` (default 300) when `budget` is omitted. |
| GET | `/api/mem/stats` | — | Counts per kind, totals, index health |
| POST | `/api/mem/calibration/sweep` | `?force=true` | Run the dreamer's importance / archive sweep against the local store. Server-side gate: `strftime('%w','now') == '0'` (Sunday) unless `force=true` is passed. Hard caps: 30 mutations per session, abort if candidate set > 100, refuse with `skipped_reason: "recent_run"` if any `changed_by LIKE 'dreamer-calibration%'` row landed in the last 6h. Returns `{day_of_week, examined, applied, archived, skipped_reason?}`. Also auto-invoked as a fourth phase of `POST /api/mem/dream?phase=all` so the LLM dreamer can read the result rather than running the sweep itself (which it was prone to skipping under session resumption). |
| GET | `/api/mem/calibration/log` | `?since=ISO8601&limit=N&count=true` | Audit trail of calibration mutations. Filters `mem_item_history WHERE changed_by LIKE 'dreamer-calibration%' AND changed_at >= since`. `count=true` returns `{count: N}`; otherwise `{entries: [...]}` with `item_id`, `changed_at`, `changed_by` (`dreamer-calibration:promote\|demote\|archive`), `change_type`, `old_importance`, `new_importance`, `old_kind`, `new_kind`. `since` is validated as RFC3339 and rejected with 400 on parse failure. `limit` defaults to 50, hard-capped at 500. |

**Actor / worker headers.** Two request headers shape mem-write semantics:

- `X-CodeTerm-Worker-Id` — set automatically by the CLI when `CODETERM_WORKER_ID` is in the environment (extraction workers, dreamer, REM phase). Any `GET /api/mem/items/{id}` with this header skips the `evidence_count` bump so internal sweeps don't feedback-loop the L1 score.
- `X-CodeTerm-Actor` — stored in `mem_item_history.changed_by`. Defaults to `"user"`; set explicitly via `codeterm mem update --changed-by <actor>` (e.g. `dreamer-calibration:promote`). The calibration sweep populates this with `dreamer-calibration:{promote|demote|archive}` so the calibration log can filter cleanly.

**Extraction concurrency:** `ServiceContext::extract_semaphore` caps auto-extraction at **4 concurrent workers** across the whole process. Every `mem_save` (except items tagged `auto-extracted`, sourced from `extracted_from:*`, or sent with `skip_extract: true`) schedules an extraction task that acquires a permit from this semaphore before spawning a hidden extraction tab. Additional graph routes (`/api/mem/graph/*`), learnings, dreaming, and worker endpoints exist but are not documented here — read `src-tauri/src/api/mem_routes.rs` for the full surface.

#### Memory — Graph self-repair (Dreamer primitives)

Whitelist-bounded graph cleanup primitives. All destructive ops audit-log to `graph_purge_log`. See `docs/MEMORY.md` for the full Dreamer flow.

| Method | Path | Body | Description |
|--------|------|------|-------------|
| POST | `/api/mem/graph/repair/find-relations` | `{group_by:[…], limit:N}` | List duplicate relation groups by a whitelisted subset of columns (`from_id`, `relation_type`, `to_id`, `scope`, `valid_from`). Returns `{groups:[{key, count, rowids, sample}]}`. Read-only. |
| POST | `/api/mem/graph/repair/delete-relations` | `{rowids:[…], actor}` | Delete explicit rowids; full row payload preserved in audit. |
| POST | `/api/mem/graph/repair/merge-relations` | `{keep:N, merge:[…], actor}` | Fold merge-rowids into keep — max(confidence), properties merged (keep wins on collision), audit row written. |
| POST | `/api/mem/graph/repair/find-entities` | `{strategy:{kind:"exact_name"\|"alias_chain"\|"vector", threshold?, entity_type?}, limit:N}` | Surface candidate duplicate entities. Vector strategy uses cosine-sim threshold (0.0–1.0). Read-only. |
| POST | `/api/mem/graph/repair/augment-entity` | `{id, patch:{…}, actor}` | PATCH `properties_json` then re-embed. Audit row with `count=0`. |
| POST | `/api/mem/graph/repair/delete-entity` | `{id, force:bool, purge_history:bool, actor}` | Delete entity (refuses if active relations and `force=false`). `force` cascades active rels, `purge_history` also drops invalidated rels. Also cleans `entity_aliases` referencing the id and the entity's `mem_vectors`. |
| POST | `/api/mem/graph/repair/orphan-vectors` | `{dry_run:bool, actor}` | Find/delete `mem_vectors` rows whose mem_id has no matching `entities`/`mem_items` row. |
| POST | `/api/mem/graph/repair/orphan-blobs` | `{dry_run:bool, actor}` | CAS-aware sweep of `sha256_*.txt` blob files no `mem_item.body_blob_path` references. |

#### Secrets

Pluggable secret-store layer. Active backend is selected by `integrations.secrets.backend` in `config.toml`; built-in backends are `local` (OS keychain, default), `bitwarden` (Bitwarden CLI, optional Vaultwarden server), and `mock` (in-memory, dev only). All endpoints require the standard Bearer token. **Local-only by design** — secret routes are never relayed across the mesh; the CLI rejects `--mesh` on `mem secret` subcommands with exit code 7.

| Method | Path | Body / Query | Description |
|--------|------|------|-------------|
| GET  | `/api/secrets/status` | — | `{backend, status:{status: "logged_out"\|"locked"\|"unlocked"\|"unavailable", user?, transient?, endpoint?, reason?}}` |
| POST | `/api/secrets/unlock` | `{master_password, email?, two_factor_token?, api_key_client_id?, api_key_client_secret?}` | Unlock the active backend. Master password is consumed and dropped immediately. |
| POST | `/api/secrets/lock` | — | Drop in-process session; persisted creds (OS keychain) are kept. |
| POST | `/api/secrets/logout` | — | Drop session AND every persisted credential. |
| POST | `/api/secrets/auto-unlock` | — | Re-run the active backend's `init` — for Bitwarden, attempt unlock from the master password persisted on the previous successful login (only when `bitwarden.persist_master_password = true`). Returns `{ok: bool, backend, status}`; `ok=false` means nothing was stored to restore from. No-op for backends without persisted creds. |
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
| POST | `/api/session/save` | — | Snapshot the multiplexer (workspaces/tabs) into the session store immediately. Durability is otherwise the 30s auto-save timer + close-to-tray hook; e2e harnesses call this before force-killing the daemon so restart-persistence is deterministic. |

#### Web Mirror
| Method | Path | Body / Query | Description |
|--------|------|------|-------------|
| GET | `/` | — | The CodeTerm web mirror: the same React bundle as the desktop app, served static at the daemon root (public route; every API call it makes stays token-gated). `?a11y=1` enables the screen-reader terminal DOM. |
| GET | `/api/mux/state` | — | Full multiplexer snapshot (workspaces/tabs + focus) — the browser bundle's `mux_get_state`. |
| POST | `/api/workspaces/{id}/tabs/{tab_id}/focus` | — | Switch active tab. |
| POST | `/api/workspaces/{id}/tabs/{tab_id}/close` | — | Close a tab. |
| POST | `/api/tabs/{id}/chat-view` | `{enabled}` | Toggle a tab's chat view flag. |
| GET | `/api/mesh/peers/{id}/services` | — | Peer services view (same cache+failover as the Mesh tab). |
| GET | `/api/events/stream` | — | SSE of system events (`data: {event_type, data}`) — mux/mesh/tab-status changes; the web bundle's `listen()`. |
| GET | `/api/output/stream` | `?since=<b64url cursor>` | Multiplexed SSE: ONE connection carries every Tab's PTY output as tab-tagged frames (`{tab_id, output, offset}`, plus per-tab `{truncated, from}` on ring eviction, `{ended}` on genuine PTY death, and a one-shot `{boot_changed, boot}` on daemon restart), replacing the per-tab long-poll below. `since` is a b64url `{boot, tabs:{id:offset}}` cursor. Killswitch: browser localStorage `codeterm-web-output-stream=off` falls back to `/api/pty/{id}/read`. With a mesh relay target set, the browser instead streams via `/api/mesh/relay-stream?path=/api/output/stream` — same frames, same cursor. |
| GET | `/api/pty/{id}/read` | — | Long-poll one chunk of PTY output (JSON byte array) — the browser terminal's read loop; fallback when the output-stream killswitch is off. |
| POST | `/api/pty/{id}/write` | `{data}` | Write input to a PTY. |
| POST | `/api/pty/{id}/resize` | `{cols, rows}` | Resize a PTY. |

#### Debug (test-only capabilities)
| Method | Path | Body / Query | Description |
|--------|------|------|-------------|
| GET | `/api/debug/clock` | — | `{mocked, now_ms}` — whether cron runs on a mock clock, and its current time. |
| POST | `/api/debug/clock/advance` | `{ms}` | Advance the mock cron clock and wake the scheduler so overdue jobs fire immediately. **409** unless the daemon is a debug build started with `CODETERM_TEST_CLOCK=1` (release builds never activate the mock). Powers the 07-cron e2e suite's time travel. |

#### Past Agent Sessions
| Method | Path | Body / Query | Description |
|--------|------|------|-------------|
| GET | `/api/sessions/search` | `?q=...&provider=claude\|codex\|all&project=...&limit=N&max_snippets=N` | Cross-provider session content search |
| GET | `/api/sessions/{id}` | — | Resolve a session id to its provider metadata (cwd, file path, timestamps) |
| POST | `/api/sessions/{id}/resume` | `{title?, workspace_id?, workspace_name?}` | Atomic "open the right workspace + tab + resume" action |

Provider-agnostic: every registered `AgentProvider` contributes its own session enumerator + parser. Today Claude reads `~/.claude/projects/*.jsonl` (cwd extracted from JSONL content, not the lossy directory name); Codex walks `~/.codex/sessions/YYYY/MM/DD/*.jsonl` and parses `response_item` messages. Providers without on-disk session storage silently contribute nothing.

**GET /api/sessions/search** — whitespace-splits `q` into terms; every term must appear (case-insensitively) in at least one non-meta message of a session for it to match. Hits are sorted most-recently-modified first. `provider=all` (default) walks every registered provider; pass an id (`claude`, `codex`) to scope. `limit` defaults to 10, `max_snippets` to 3. Snippets carry ~80 chars of context before and ~120 after the first matching position, sliced on UTF-8 char boundaries so non-ASCII queries don't panic.

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
3. Spawn an agent tab in that workspace with `--resume <id>` baked into `args` (Claude). Other providers are routed via their `build_resume_command` shape.
4. The tab's `agent_session_id` is NOT pre-set — the SessionStart hook records it as soon as the agent reports it, avoiding double `--resume` injection from the launch/resume code path in `commands.rs::spawn_pty`.

Response:
```json
{
  "tab_id": "a3f1b2c7",
  "tab_id": "d5e6f7a8",
  "workspace_id": "b4e2c8d1",
  "provider": "claude",
  "session_id": "0193e1a4-7b2c-7d4e-9f80-1234567890ab",
  "cwd": "/Users/me/code/foo",
  "resume_command": "claude --resume 0193e1a4-... --dangerously-skip-permissions"
}
```

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

#### Updates
| Method | Path | Body / Query | Description |
|--------|------|------|-------------|
| GET | `/api/update/check` | `?force=true` | Check for available updates on the active channel. Cached for 1 hour unless `force=true`. |
| POST | `/api/update/install` | `?method=auto&manifest_url=...&after_update=...` | Install the pending update. Without `method=auto`, returns either an OS install command or opens the download URL in the browser. With `method=auto`, runs the full Tauri-plugin-updater flow (download, signature verify, install, restart). `after_update` (optional) stashes a message to `~/.codeterm/post-update-message.txt`; the new daemon delivers it to the General tab after restart. |
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

#### Agent Restart (POST /api/tabs/{id}/agent/restart)

Restart the agent in a tab. Default behaviour is graceful: send the provider-specific exit command and wait up to `grace_secs` for the shell-ready event. CodeTerm then tears down the old PTY, creates a replacement PTY, and relaunches. If the recorded provider session still exists on disk, the provider resume command is used; otherwise CodeTerm launches a fresh session.

```json
{"force": false, "grace_secs": 10}
```

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `force` | `bool` | `false` | Skip the graceful exit event wait and tear down the old PTY immediately. Use only when the agent is hung or unresponsive. |
| `grace_secs` | `u64` | `10` | Failure deadline around the shell-ready event wait before the old PTY is torn down. Ignored when `force=true`. |

**Ready response** (`202 Accepted`):
```json
{
  "restart_accepted": true,
  "old_pty_torn_down": true,
  "agent_ready": true,
  "old_pty_id": "f0c92c76-1cce-4a65-b5e4-a68c69ae1e2e",
  "new_pty_id": "b264031d-5a5a-461b-bba6-189405223f80",
  "tab_id": "a3f1b2c7",
  "provider": "claude",
  "session_id": "0193e1a4-7b2c-7d4e-9f80-1234567890ab",
  "session_was_stale": false,
  "session_action": "resume",
  "auto_approve": true,
  "permission_profile": "yolo",
  "force": false
}
```

The three transition fields are independent observations. `restart_accepted` means validation and rate limiting accepted the request. `old_pty_torn_down` means CodeTerm observed the prior PTY leave the tab and its process tree complete. `agent_ready` means an `AgentReady` transition was observed for `new_pty_id`, which must differ from `old_pty_id`. A pre-restart `AgentReady` value attached to the old PTY never satisfies this contract.

`session_was_stale` is `true` when the recorded session id pointed at a JSONL that no longer exists on disk; the restart proceeds without `--resume` and a fresh session is created.

**Errors:**
- `404` — tab id not found
- `400` — tab has no agent to restart, or unsupported provider
- `429` — rate-limited (returns `Retry-After` header + `{"retry_after_secs": N}` body); restart is throttled per-tab to avoid restart loops
- `500` — teardown or relaunch failed
- `504` — the replacement PTY did not reach `AgentReady` inside the bounded readiness deadline

Failures after acceptance include `error` plus the same transition fields. Every boolean remains false unless that leg was observed, and `new_pty_id` is included only if a replacement PTY was linked. Request rejections (`400`, `404`, and `429`) occur before acceptance and retain their compact error bodies. A failed restart is never returned in the ready `202` shape.

This is the same flow `codeterm agent restart --tab TAB_ID [--force] [--grace-secs N]` invokes from the CLI.

### Plan Task Attempts

Task execution attempts are append-only history on `Task.execution_attempts`. The CLI/API uses the canonical stable `TaskExecutionAttempt.id` as `attempt_id`; `executor_tab_id` remains the executor identity and is not the primary key. Respawning creates a new Worker tab and a new live attempt; it never reactivates the selected historical attempt.

### Team-bound plan creation

An orchestrator's first plan creation is atomic: its response includes `plan.team_id`, top-level `team_id`, and `team_minted`. When `team_minted` is true, `_hints` includes `codeterm team rename <team_id> <name>` and `codeterm docs ORCHESTRATION`. The `mux-state-changed` snapshot includes the Team registry in `teams` and each tab's `team_id`; committed plan mutations also emit `plan-changed`. A Team with an active plan refuses a second plan with the unchanged `team_already_has_active_plan` error.

### Team-bound plan activation and transfer

Every active plan must reference exactly one live Team, and a live Team may have only one active plan. `POST /api/plans/{id}/unarchive` and `POST /api/plans/{id}/resume` reject a missing or stale Team binding; unarchive also rejects a Team that already owns another active plan. `team.adopt_plan` activates an archived/teamless plan atomically, replaces its retired owner with the target Team's sole live Orchestrator, records owner history, clears the stale planner binding, and relinks live task hierarchy to that Orchestrator. A Team with zero or multiple live Orchestrators cannot adopt implicitly; use an explicit transfer destination when applicable.

`POST /api/plans/{id}/transfer` accepts `{to: OrchestratorRef, reason?}` and requires `X-CodeTerm-Tab-Id` to identify the source Team's Orchestrator or planner-profile Manager. Missing callers, Workers, and foreign-Team tabs receive `403` without mutation. The destination must be a live Orchestrator belonging to a live Team. Owner, owner history, Team binding, and stale planner retirement are one storage mutation, followed by hierarchy relinking under the Team lock.

### Close Team (POST /api/teams/{team_id}/close)

Explicitly closes every Team member through the Team cascade path, removes the Team registry entry, archives and detaches every bound plan, retires each plan owner into owner history, clears its owner/planner routing authority, and emits one committed `mux-state-changed`. This is the only user operation that removes a Team; ordinary tab closes always retain the Team record. The `X-CodeTerm-Tab-Id` header is required and must identify the Team's Orchestrator or planner-profile Manager. Missing header returns `400`, unauthorized or foreign callers return `403`, and an unknown or already closed Team returns structured `404`.

| Method | Path | Body / Query | Description |
|--------|------|--------------|-------------|
| GET | `/api/plans/{id}/tasks/{tid}/attempts` | — | List execution attempts recorded for one task |
| GET | `/api/plans/{id}/tasks/{tid}/attempts/{attempt_id}` | — | Show one attempt by stable attempt id, including derived report command, derived prompt, and last report summary when present |
| POST | `/api/plans/{id}/tasks/{tid}/respawn` | `{attempt_id, provider?, model?, parent_tab_id?, workspace_id?, title?}` | Build a Worker prompt from the task body, selected prior attempt, last report/status, lane branch/worktree hints, and exact `codeterm agent report --plan ... --task ...` command; spawn a new Worker and append a new `execution_attempts` entry |

### Keep-Awake (GET/POST /api/keepawake*)

Prevents the machine from sleeping (and optionally from dimming the display) while long-running agents are working. Global machine state, not tab- or workspace-scoped. All three endpoints require the standard Bearer token.

**Status shape (`KeepAwakeStatus`):**
```json
{
  "active": true,
  "display": false,
  "expires_at": 1744387200000
}
```
- `active` — whether a keep-awake session is currently running.
- `display` — whether the display is also being kept on (not just the system).
- `expires_at` — unix milliseconds when the session auto-expires, or `null` for an indefinite session.

**GET /api/keepawake** — returns the current `KeepAwakeStatus`. When no session is active, `active=false`, `display=false`, `expires_at=null`.

**POST /api/keepawake/start** — request body (all fields optional):
```json
{"duration_secs": 3600, "display": false}
```
- `duration_secs` — session length in seconds. Omit or `null` for an indefinite session.
- `display` — `true` to also prevent display sleep/dimming. Default `false`.

Any existing session is stopped first, then the new one is started. Returns the fresh `KeepAwakeStatus`.

**POST /api/keepawake/stop** — no body. Stops any active session. Returns `{"ok": true}`.

**Tauri event `keepawake-changed`** — emitted on every state transition: explicit start, explicit stop, natural expiry (when `duration_secs` elapses), and `caffeinate` spawn failure on macOS. Payload matches `KeepAwakeStatus`. Frontends subscribe with `listen("keepawake-changed", (ev) => ...)`.

**Persistence:** state is written to `~/.codeterm/keepawake.json` (or `~/.codeterm-dev/keepawake.json` on dev) and restored on startup — indefinite sessions resume as-is, timed sessions resume with the remaining duration, already-expired sessions are discarded.

**Platform support:**
- **macOS** — spawns `caffeinate -i -s` (plus `-d` when `display=true`).
- **Windows** — calls `SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED)`, OR-ing in `ES_DISPLAY_REQUIRED` when `display=true`.
- **Linux** — currently a no-op; logs a warning and still returns a status object so clients work unchanged.

**Example:**
```bash
# Start an indefinite session that also keeps the display on
curl -X POST http://host:7685/api/keepawake/start \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"display": true}'

# Start a 1-hour session (system only)
curl -X POST http://host:7685/api/keepawake/start \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"duration_secs": 3600}'

# Check status
curl http://host:7685/api/keepawake -H "Authorization: Bearer $TOKEN"
# {"active":true,"display":false,"expires_at":1744387200000}

# Stop
curl -X POST http://host:7685/api/keepawake/stop -H "Authorization: Bearer $TOKEN"
# {"ok":true}
```

### Agent Spawn (POST /api/agents/spawn)

Creates a **new tab** in the parent's workspace, assigns a role, launches an agent (claude/aider/any), and optionally sends a task. All in one request. The response returns immediately (non-blocking) -- PTY readiness detection and agent launch happen in a background thread.

The hierarchy (Orchestrator -> Worker) is a logical chain, not nested layouts. Each agent gets its own tab. Agent spawns do not auto-switch the active tab -- the user stays on their current tab.

**Request body:**
```json
{
  "agent": "claude",           // agent command (default: "claude")
  "agent_name": "architect",   // maps to --agent flag (.claude/agents/ file)
  "title": "Worker-1",         // tab title (default: agent_name or agent command)
  "cwd": "C:/project",         // working directory
  "role": "Worker",            // Orchestrator or Worker (Conductor accepted as alias for Orchestrator)
  "parent_tab_id": "a3f1b2c7", // parent Orchestrator for a Worker (8-char hex ID)
  "can_spawn": true,           // allow spawning children
  "task": "analyze the code",  // baked into launch via -p flag (instant start)
  "model": "sonnet",           // optional; must match the provider's advertised model list when one exists
  "args": ["--verbose"],       // extra agent args
  "workspace_id": "b4e2c8d1"  // optional, defaults to active workspace
}
```

All fields are optional. `agent` defaults to `"claude"`. For Claude, `--dangerously-skip-permissions` is added automatically. When `model` is provided and the provider advertises models, unknown ids are rejected before tab creation; run `codeterm agent models <provider>` for valid ids. When `task` is provided, it is passed via the `-p` flag so the agent starts working immediately (no delay). When `agent_name` is provided, it maps to `--agent <name>` which loads the agent definition from `.claude/agents/<name>.md`.

**Response** (returned immediately, before agent is fully started):
```json
{"tab_id": "a3f1b2c7", "tab_id": "d5e6f7a8", "pty_id": "c9d0e1f2", "agent": "claude", "title": "Worker-1", "role": "Worker"}
```

### Remote mesh workers

A worker spawned on another peer with `codeterm agent spawn <agent> --mesh PEER [--report-to <tab|main|general>]` gets a **durable cross-mesh report target**: the CLI relays the spawn to `PEER`, whose relay ingress stamps an authenticated `origin_peer` onto the body, and the spawned Tab persists `report_target_mesh_peer = <caller's peer>` + `report_target_tab_id = <report_to>` (defaults to the caller's Tab).

The worker then just runs `codeterm agent report "..."` — no `--mesh`, no `--tab`. Its daemon resolves the remote target and relays the report over the same signed `/send` path (with `kind: "report"`), so it surfaces on the origin peer as `[Report from <worker>@<peer>]`, identical to a local report. If no target resolves, `agent report` fails with an actionable hint instead of silently dropping.

Report routing precedence: **remote mesh target → `parent_tab_id` → `report_target_tab_id` → `spawned_by_tab_id`**; never self.

Each inbound mesh report registers the worker in the orchestrator-side registry exposed by `GET /api/remote-workers` and the `remote-workers-changed` event, which the sidebar renders as a read-only "remote tab" ghost node under its orchestrator (peer chip + status, no local PTY).

### Batch Commands (POST /api/batch)

Execute multiple operations in a single HTTP request. Commands run sequentially. Each command can have an `id` that other commands reference via `$id` (resolves to `tab_id` from that result).

**Request body:**
```json
{
  "commands": [
    {"method": "spawn", "id": "boss", "params": {"title": "Boss", "role": "Orchestrator", "agent": "claude", "cwd": "C:/project", "can_spawn": true}},
    {"method": "spawn", "params": {"title": "Worker-1", "role": "Worker", "parent_tab_id": "$boss", "agent": "claude", "task": "analyze code"}},
    {"method": "spawn", "params": {"title": "Worker-2", "role": "Worker", "parent_tab_id": "$boss", "agent": "claude"}},
    {"method": "notify", "params": {"title": "All spawned", "body": "4 agents ready"}}
  ]
}
```

**Available methods:** `spawn`, `send`, `rename`, `close`, `role`, `notify`, `can-spawn`, `report`

**$ref syntax:** Any string value starting with `$` is replaced with the `tab_id` from the result of the command with that `id`. E.g. `"parent_tab_id": "$boss"` resolves to the tab_id returned by the "boss" spawn command.

**Response:**
```json
{"results": [{"tab_id": "...", "tab_id": "...", ...}, {"ok": true}, ...]}
```

On error, individual commands return `{"error": "message", "method": "..."}` but subsequent commands still execute.

**Batch method parameters:**

| Method | Required params | Optional params |
|--------|----------------|-----------------|
| `spawn` | — | `agent`, `agent_name`, `title`, `cwd`, `role`, `parent_tab_id`, `can_spawn`, `task`, `args` |
| `send` | `tab_id`, | `text`, `enter` (default: true) |
| `rename` | `tab_id`, `title` | — |
| `close` | `tab_id` | — |
| `role` | `tab_id`, `role` | `parent_tab_id`, `can_spawn` |
| `notify` | — | `title`, `body`, `level` |
| `can-spawn` | `tab_id` | `can_spawn` (default: true) |
| `report` | `tab_id`, `message` | — |

### Output Cursor Pattern

Use `offset` parameter for efficient polling without fetching the full buffer:
```bash
# First call — get all output
curl "http://host:7685/api/tabs/$TAB/output"
# Response: {"output": "...", "offset": 1234}

# Subsequent calls — only new data
curl "http://host:7685/api/tabs/$TAB/output?offset=1234"
# Response: {"output": "new stuff", "offset": 1500}
```

### ANSI Stripping

Add `strip_ansi=true` to get clean text without escape codes:
- `GET /api/tabs/:id/output?strip_ansi=true`
- `POST /api/tabs/:id/exec {"command": "...", "strip_ansi": true}`

### Exec Pattern

Send a command and wait for its output in one call:
```bash
curl -X POST "http://host:7685/api/tabs/$TAB/exec" \
  -H "Content-Type: application/json" \
  -d '{"command": "echo hello", "timeout_ms": 5000}'
# Response: {"output": "hello\r\n...", "offset": 1600}
# If timeout: {"output": "...", "offset": 1600, "timeout": true}
```

### SSE Output Stream

Subscribe to real-time output via Server-Sent Events:
```bash
curl -N "http://host:7685/api/tabs/$TAB/output/stream"
# data: {"output": "new text", "offset": 1234}
# data: {"output": "more text", "offset": 1300}
```

### Examples

```bash
# Health check
curl http://host:7685/api/health

# Create workspace + tab
WS=$(curl -s -X POST http://host:7685/api/workspaces \
  -H "Content-Type: application/json" \
  -d '{"name":"my-project"}' | python3 -c "import sys,json;print(json.load(sys.stdin)['id'])")

# Get tab and run command
TAB=$(curl -s http://host:7685/api/tabs | python3 -c "import sys,json;print(json.load(sys.stdin)[-1]['id'])")
curl -X POST "http://host:7685/api/tabs/$TAB/exec" \
  -H "Content-Type: application/json" \
  -d '{"command": "npm test", "timeout_ms": 30000}'

# Send with batch (e.g. launch Claude Code)
curl -X POST "http://host:7685/api/tabs/$TAB/send" \
  -H "Content-Type: application/json" \
  -d '{"batch": [{"text": "claude", "enter": true}, {"text": "", "enter": true, "delay_ms": 8000}]}'

# Register webhook for agent events
curl -X POST http://host:7685/api/webhooks \
  -H "Content-Type: application/json" \
  -d '{"url": "https://my-orchestrator/hook", "events": ["agent.permission_request"]}'

# One-shot webhook (fires once, then auto-deletes — like a Promise)
curl -X POST http://host:7685/api/webhooks \
  -H "Content-Type: application/json" \
  -d '{"url": "https://my-orchestrator/done", "events": ["agent.task_complete"], "once": true, "tab_id": "'"$TAB"'"}'

# Network info
curl http://host:7685/api/network
# {"local_ip":"192.168.1.50","tailscale_ip":"100.64.10.1","api_url":"http://192.168.1.50:7685"}
```

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

## Socket API (JSON-RPC v2)

### Connection
- **Windows**: Named pipe `\\.\pipe\codeterm-{PID}`
- **macOS/Linux**: Unix socket `/tmp/codeterm-{PID}.sock`
- Auto-discovery: `CODETERM_SOCKET_PATH` env var

### Protocol
Newline-delimited JSON:
```json
{"jsonrpc":"2.0","id":1,"method":"system.identify","params":null}
```

### Methods

| Method | Params | Description |
|--------|--------|-------------|
| `system.identify` | — | Version, platform, features |
| `system.capabilities` | — | List all methods. Socket-only; unsupported over mesh relay. |
| `workspace.list` | — | List workspaces |
| `workspace.create` | `{name}` | Create workspace |
| `workspace.focus` | `{workspace_id}` | Focus workspace |
| `workspace.rename` | `{workspace_id, name}` | Rename workspace |
| `workspace.close` | `{workspace_id}` | Close workspace |
| `workspace.recently_closed` | `{workspace_id}` | List recently-closed tab entries for a workspace. Returns `ClosedTab[]`. |
| `workspace.reopen_closed` | `{workspace_id, index?}` | Reopen the nth recently-closed tab in a workspace (`index` 0 = most recent). Returns `TabInfo`. |
| `workspace.reopen_closed_group` | `{group_id}` | Reopen a closed group by `group_id`. Returns `{workspace_id, tab_id, tab_id}`. |
| `workspace.move_tab` | `{tab_id, from_workspace_id, to_workspace_id}` | Move tab |
| `team.list` | — | Return `{teams: Team[]}` from the live Team registry. |
| `team.show` | `{team_id}` | Return `{team: Team}` or reject an unknown Team. |
| `team.rename` | `{team_id, name, caller_tab_id?}` | Rename a Team and emit `mux-state-changed`. |
| `team.close` | `{team_id, caller_tab_id?}` | Explicitly cascade-close Team members, remove the registry entry, archive/detach bound plans, retire owner/planner routing, and emit `mux-state-changed`. |
| `team.adopt_plan` | `{team_id, plan_id, caller_tab_id}` | Atomically activate an existing plan 1:1 under the Team's sole live Orchestrator, replace owner/history, clear stale planner routing, relink hierarchy, and emit both `plan-changed` and `mux-state-changed`. The caller is required and must manage the destination Team. |
| `plan.transfer` | `{plan_id, to: OrchestratorRef, reason?, caller_tab_id}` | Transfer atomically to the destination Orchestrator's live Team. The caller is required and must manage the source Team. |
| `tab.list` | — | List tabs |
| `tab.focus` | `{tab_id}` | Focus tab |
| `tab.close` | `{tab_id}` | Close tab |
| `tab.send_text` | `{tab_id, text, enter?}` | Send text to PTY (`enter` defaults to `true`, appends `\r`) |
| `tab.get_output` | `{tab_id, max_bytes?}` | Read terminal output |
| `tab.rename` | `{tab_id, title}` | Rename tab (pinned) |
| `mem.stats` | — | Return memory item, vector, entity, and relation counts over the daemon socket. |
| `mem.search` | `{q, kind?, area?, scope?, project?, limit?, rerank?}` | Search memory and the optional project plan index over the daemon socket. |
| `mem.get` | `{id}` | Fetch one mem item by id. |
| `mem.save` | `{kind, title, body, scope?, summary?, area?, tags?, importance?, source_path?, replace?, skip_extract?, pinned?}` | Create a mem item; may spawn an auto-extraction worker. |
| `mem.delete` | `{id}` | Delete a mem item by id. |
| `mem.forget` | `{area}` | Batch-delete every item in an area. |
| `mem.update` | `{id, title?, summary?, area?, tags?, body?, importance?, pinned?, changed_by?}` | Partial update; also used for pin/unpin (`pinned: true/false`). |
| `mem.pinned` | `{limit?}` | List pinned items. |
| `mem.append` | `{id, text, changed_by?}` | Append text to an item's body (non-destructive). |
| `mem.restore` | `{id, version?, changed_by?}` | Restore a prior body from the audit history. |
| `mem.wake_up` | `{budget?, l0_only?, no_l0?, explain?, workspace?}` | L0+L1 wake-up context text. |
| `mem.learn` | `{text, kind?, tags?}` | Capture a quick learning. |
| `mem.learn_promote` | `{id}` | Promote a learning to a fact. |
| `mem.flush` | `{tab, title?}` | Flush a tab's PTY scrollback as a `conversation` mem item. |
| `mem.sync` | `{path?}` | Trigger file sync for source-tracked items (Librarian). |
| `mem.reembed` | `{scope?}` | Re-embed mem items and/or graph entities. |
| `mem.extract_pending` | `{limit?}` | Re-queue extraction workers for un-extracted items. |
| `mem.history` | `{id}` | Full audit trail for one item. |
| `mem.taxonomy` | `{workspace?}` | `{area -> {kind -> count}}` breakdown. |
| `mem.tail` | `{since?, limit?}` | Recent mem writes, newest first. |
| `mem.calibration.log` | `{since?, limit?, count?}` | Calibration audit log (importance/archive changes). |
| `mem.calibration.sweep` | `{force?}` | Trigger the dreamer's importance/archive sweep. |
| `mem.worker.spawn` | `{intent, task, agent?, parent?}` | Spawn a hidden graph worker. |
| `mem.worker.list` | — | List all graph workers. |
| `mem.worker.get` | `{id}` | Fetch one graph worker by id. |
| `mem.worker.progress` | `{id, text, status?}` | Report worker progress (called by the worker itself). |
| `mem.worker.report` | `{id, summary}` | Submit a worker's final report and mark it done. |
| `mem.worker.kill` | `{id}` | Kill a graph worker. |
| `mem.graph.create` | `{id, type, scope?, properties?}` | Create a graph entity. |
| `mem.graph.relate` | `{from, rel, to, valid_from?, valid_to?, confidence?, scope?, reason?}` | Create a relation. |
| `mem.graph.related` | `{id, rel?, as_of?, include_invalidated?}` | List outgoing relations from an id. |
| `mem.graph.invalidate` | `{from, rel, to, ended?}` | Invalidate a relation (preserves history). |
| `mem.graph.timeline` | `{entity, include_invalidated?}` | Full temporal timeline for an entity. |
| `mem.graph.traverse` | `{from, hops?, max_nodes?, as_of?, include_invalidated?}` | BFS traversal from an entity. |
| `mem.graph.path` | `{from, to, max_hops?, include_invalidated?}` | Shortest path between two entities. |
| `mem.graph.tunnels` | `{include_invalidated?}` | List entities bridging 2+ areas. |
| `mem.graph.alias` | `{alias, canonical}` | Merge one entity into another. |
| `mem.graph.repair.find_relations` | `{group_by, limit?}` | Find duplicate relations (read-only). |
| `mem.graph.repair.delete_relations` | `{rowids, actor?}` | Delete relations by rowid (audited). |
| `mem.graph.repair.merge_relations` | `{keep, merge, actor?}` | Merge relations into `keep`. |
| `mem.graph.repair.find_entities` | `{strategy, limit?}` | Find duplicate entities. |
| `mem.graph.repair.augment_entity` | `{id, patch, actor?}` | Patch properties on an entity (audited). |
| `mem.graph.repair.delete_entity` | `{id, force?, purge_history?, actor?}` | Delete an entity (audited). |
| `mem.graph.repair.orphan_vectors` | `{dry_run?, actor?}` | Find/delete orphan `mem_vectors` rows. |
| `mem.graph.repair.prune_thin_vectors` | `{dry_run?, actor?}` | Find/delete bodyless-embedding vectors. |
| `mem.graph.repair.collapse_synonym_edges` | `{dry_run?, actor?}` | Collapse parallel membership-synonym edges. |
| `mem.graph.repair.prune_dream_hub_edges` | `{dry_run?, actor?}` | Invalidate dream-diary backlinks into hubs. |
| `mem.graph.repair.orphan_blobs` | `{dry_run?, actor?}` | Find/delete unreferenced blob files. |

`codeterm mem` routes its full item/history/taxonomy/tail/learn/wake-up/sync/skill/worker/calibration/graph surface over these socket methods — HTTP is no longer attempted for any of it, so a live socket with a dead HTTP API no longer breaks `mem`. A handful of `mem graph` subcommands (route, purge-invalidated, revive-broken, introduce-topic, hubs, islands, orphans, get) are local-only by design — the CLI opens `knowledge.db` directly and never touches either transport. `mem secret` is the one deliberate holdout: `mem.secret.*` gets an explicit `-32004` rejection over the socket (see `secret_socket_rejection` in `socket/server/mem/mod.rs`) because it's the only mem surface that can return plaintext credentials and the socket has no auth gate, so it stays on the bearer-token-gated `/api/secrets/*` HTTP API by design, not because it's unmigrated.
| `notification.create` | `{title, body?, level?, tab_id?}` | Create notification |
| `notification.list` | `{unread_only?}` | List notifications |
| `notification.clear` | `{id?}` | Clear notifications |
| `agent.list` | — | List all tabs with roles |
| `agent.hierarchy` | — | Full hierarchy tree |
| `agent.spawn` | `{agent?, title?, cwd?, role?, parent_tab_id?, can_spawn?, task?, args?, workspace_id?}` | Spawn agent (same as REST) |
| `agent.set_role` | `{tab_id, role, parent_tab_id?, can_spawn?}` | Set tab role |
| `agent.clear_role` | `{tab_id}` | Clear tab role |
| `agent.status` | `{tab_id}` | Get tab status + role |
| `agent.children` | `{tab_id}` | List child tabs |
| `agent.link` | `{tab_id, parent_tab_id}` | Link tab under parent tab |
| `agent.report` | `{tab_id, message}` | Send report to direct parent |
| `agent.restart` | `{tab_id, force?, grace_secs?}` | Restart the agent in a tab; resumes only when the recorded session is valid. |
| `plan.task.attempts` | `{plan_id, task_id}` | List execution attempts for one plan task |
| `plan.task.attempt.show` | `{plan_id, task_id, attempt_id}` | Show one historical task attempt |
| `plan.task.respawn` | `{plan_id, task_id, attempt_id, provider?, model?, parent_tab_id?, workspace_id?, title?}` | Spawn a new Worker from one prior task attempt and record a new live attempt |
| `cron.list` | `{workspace_id?}` | List cron jobs |
| `cron.get` | `{id}` | Get cron job by ID |
| `cron.create` | `{name, schedule, task, workspace_id, agent?, model?, cwd?, description?, oneshot?, jitter_max_ms?}` | Create cron job |
| `cron.update` | `{id, name?, schedule?, task?, cwd?, description?, enabled?, agent?, model?, jitter_max_ms?}` | Update cron job |
| `cron.delete` | `{id}` | Delete cron job |
| `cron.enable` | `{id}` | Enable cron job |
| `cron.disable` | `{id}` | Disable cron job |
| `cron.trigger` | `{id}` | Manually trigger cron job |
| `cron.cancel` | `{id}` | Stop currently-running executions (`{cancelled: N}`) without deleting the job |
| `cron.history` | `{id}` | Get run history |
| `cron.complete` | `{run_id, summary?}` | Mark a cron run as complete |
| `services.list` | `{kind?, enabled?}` | List registered services (cron + daemon). |
| `services.create` | `{name, kind:"daemon", health_mode, ...}` | Register a daemon-kind service. Same body shape as `POST /api/services`. Returns `{service, token}`; rejects non-daemon kinds with `-32602` (use `cron.create` for cron jobs). |
| `services.get` | `{id}` | Get one service (with latest health snapshot). |
| `services.update` | `{id, ...patch}` | Sparse update; same fields as `PUT /api/services/{id}`. |
| `services.delete` | `{id}` | Delete service + its push token + activity. |
| `services.regenerate_token` | `{id}` | Rotate push token. Returns `{token}`. |
| `activity.list` | `{source?, since?, limit?}` | Read recent activity events (push / health / peer / cron). Defaults: `limit=500`. |
| `bridge.send` | `{text, chat_id?, service?, append?, type?, file?, caption?, filename?}` | Send message or media to external service (Telegram). `type`: `text`/`photo`/`document`/`voice`/`animation`/`video`/`sticker`. `file`: absolute path for media. `append`: concatenate to previous message. |

For `team.rename` and `team.close`, omitted `caller_tab_id` is the trusted direct-user path on the local socket. `team.adopt_plan` requires a non-empty caller belonging to the destination Team as its Orchestrator or planner-profile Manager; missing callers, Workers, and foreign tabs are rejected before mutation. Adoption preserves the structured `team_already_has_active_plan` conflict when the Team already owns another active plan. Events are emitted only after persistence succeeds.

## CLI

The complete CLI reference is auto-generated from the clap command tree:
see **`docs/CLI.md`** (regenerated by `codeterm cli-manifest --write`). The
machine-readable tree lives in `docs/cli-manifest.json`.

### CLI JSON ID Contract

Planner mutation commands emit one JSON envelope:
`{ok, kind, id, data, hints?, error?}`. `id` is the primary created or mutated
entity id, and `data` contains the raw daemon payload with typed fields such as
`plan_id`, `task_id`, `spec_id`, and `section_id`. For example,
`spec create --json` returns `kind == "spec"` and `id == spec_id`,
`plan create --json` returns `kind == "plan"` and `id == plan_id`, and
`plan task add --json` returns `kind == "task"` and `id == task_id`.

### Internal Tauri Commands

These are Tauri IPC commands (invoked from frontend via `invoke()`), not REST endpoints.

| Command | Params | Description |
|---------|--------|-------------|
| `mux_reopen_closed_tab` | `workspace_id: String, index: Option<usize>` | Reopen the nth recently-closed tab in a workspace (`index` 0 = most recent, default). Returns `TabInfo {id, title, tab_id}`. |
| `mux_reopen_closed_group` | `group_id: String` | Reopen a closed group (single/cascade/workspace) by `group_id` from the snapshot. Returns `{workspace_id, tab_id, tab_id}`. |
| `mux_restore_orphaned_tab` | `tab_id: String` | Reopen a tab from the orphaned ring (its original workspace was deleted). Reuses a live workspace at the same `cwd` if one exists, else recreates from the stored name + cwd. Returns `{workspace_id, tab_id, tab_id}`. Desktop (Tauri) only — no REST route; in web mode bridged via `mux_restore_orphaned_tab` invoke alias. |
| `mux_update_tab_cwd` | `tab_id, cwd` | Updates tab's current working directory (called by OSC 7 handler in frontend) |
| `mux_set_tab_agent_type` | `tab_id, agent_type` | Marks tab as running an agent (e.g. `"claude"`) |
| `detect_claude_session_id` | `cwd, exclude?, max_age_secs?` | Finds Claude Code session ID by scanning `~/.claude/projects/` directory structure |
| `mux_set_claude_session_id` | `tab_id, session_id` | Links tab to a Claude Code session for `--resume` support |
| `get_default_cwd` | — | Returns the application's current working directory |
| `mux_batch` | `commands: [{method, params, id?}]` | Execute batch commands with $ref resolution (same semantics as POST /api/batch). Delegates to the unified ServiceContext. |
| `get_autostart_enabled` | — | Returns whether OS autostart (launch at login) is enabled |
| `set_autostart_enabled` | `enabled: bool` | Enable/disable OS autostart. Windows: HKCU registry. macOS: LaunchAgent plist. Linux: .desktop file. |
| `save_clipboard_file` | `bytes: Vec<u8>, filename: String` | Saves arbitrary clipboard file bytes to a managed temp bucket (`files`) under the OS temp dir and returns the absolute path. Used by clipboard paste / drag-drop to materialize attachments before sending to an agent. Eviction is handled by the shared `attachments` module. |
| `read_clipboard_files` | — | Reads native OS clipboard file references and returns absolute paths. **Windows:** `CF_HDROP` via `clipboard-win`. **macOS:** `NSPasteboardTypeFileURL`. **Linux:** stub (returns empty). |
| `get_tab_text` | `tab_id: String, formatted: Option<bool>` | Returns the tab's visible VT100-parsed text. With `formatted=true` the string contains raw ANSI escape codes preserving colors, cursor position, and cursor visibility — used on tab remount to avoid the ConPTY double-cursor issue when TUI apps hide the cursor via DECTCEM. Default `formatted=false` returns plain text. |

## Environment Variables

Set automatically in every PTY spawned by CodeTerm:

| Variable | Example | Description |
|----------|---------|-------------|
| `CODETERM` | `1` | Running inside CodeTerm |
| `CODETERM_VERSION` | `0.4.0` | App version |
| `CODETERM_SOCKET_PATH` | `\\.\pipe\codeterm-1234` | Socket path |
| `CODETERM_TAB_ID` | `a3f1b2c7` | Current tab ID (8-char hex) |
| `CODETERM_WORKSPACE_ID` | `b4e2c8d1` | Current workspace ID (8-char hex) |
| `CODETERM_ROLE` | `Orchestrator` | Agent role: `Orchestrator` or `Worker` (empty if no role). Set at PTY spawn. |
| `CODETERM_PARENT_TAB_ID` | `b4e2c8d1` | Parent Tab ID in hierarchy (empty if Orchestrator or no role). Set at PTY spawn. |
| `CODETERM_API_PORT` | `7685` | API port for the running instance. Set by CodeTerm so CLI and hooks use the correct port (dev: 7686, prod: 7685). |
| `CODETERM_API_TOKEN` | `your-secret-token` | Bearer token for the API. Injected into every PTY when `[api].token` is set, so CLI subcommands and the auth-enforced hook callbacks (`/api/tabs/*/hook/*`, `/api/mem/flush`) can authenticate without per-call wiring. |
| `TERM` | `xterm-256color` | Terminal type |
| `COLORTERM` | `truecolor` | Color support |
| `TERM_PROGRAM` | `CodeTerm` | Terminal name |

## Agent Integration

CodeTerm automatically detects Claude Code and other agents:
- **Permission requests** → in-app toast + Windows notification + webhook
- **Task completion** → in-app toast + Windows notification + webhook
- **Agent icon** on tab (orange ✻ for Claude Code)
- **Pinned tab names** — manual rename blocks OSC auto-rename

Skills for Claude Code are in `skills/` directory. Install via Settings → Agents & Skills.

### Open agent shells (`chatBackend`)

Beyond PTY agents, a tab can host an **open agent shell** — a non-PTY chat tab
driven by a `chatBackend` plugin that talks to a local LLM, runs its own ReAct
tool loop inside the plugin sandbox, and renders its active system prompt as an
observable first-message card. Shells are discovered and spawned through the same
surfaces as agents:

- **Discover:** `GET /api/agents/providers` (CLI `codeterm agent providers`) lists
  installed shells alongside PTY agents, tagged `kind: "shell"`. There is no
  hardcoded shell list — core enumerates the `chatBackend` registry.
- **Spawn:** `codeterm agent spawn <shell-id> [--system TEXT] [--preset ID] [--model M]`
  (or `POST /api/agents/spawn`) routes by capability to the shell. `--system`
  overrides the preset's system prompt for that session, `--preset` picks a preset,
  no flag → the plugin's `defaultPreset`.
- **Watcher spawn:** `codeterm agent spawn <shell-id> --role Watcher --parent <orchestrator-tab> --charter <TEXT|charter:id> [--interval 30s] [--nudge-cooldown 10m] [--max-actions 3]`.
  `--charter-file PATH` may be used instead of `--charter`. The REST body fields
  are `charter`, `tick_interval_secs`, `nudge_cooldown_secs`, and
  `max_actions_per_tick`. The host opens the shell with `mode: "watcher"` and
  `engine: { kind: "machine", charter, tickIntervalSecs }`, rejects direct sends
  into the watcher tab, schedules `watcherTick`, parses `watcher_verdict`
  messages, and executes only host-allowlisted verdict actions.
- **Open directly:** `POST /api/tabs {chat_backend, system_prompt?, preset?, model?}`.
- **Observe:** the shell emits a `system_prompt` marker message rendered as a
  collapsible "System prompt" card; tool-call / tool-result messages render via the
  existing chat-UI message rendering.

Authoring a `chatBackend` plugin, the open-shell loop, presets, the curated text
tool protocol, and the worked lmstudio example: see **`docs/PLUGINS.md`**.

## Command Palette

`Cmd+K` (macOS) / `Ctrl+K` (Windows/Linux) opens the command palette for quick access to tabs, workspaces, settings, and theme switching.

## System Tray

CodeTerm hides to the system tray on window close instead of quitting. To fully quit, use the tray menu. This ensures cron jobs and background agents continue running when the window is closed.

**macOS:** The tray icon uses a template image (white on transparent) that auto-adapts to light and dark menu bar modes.

## Dev/Prod Isolation

Dev builds (`tauri.dev.conf.json`) use identifier `com.codeterm.dev` and are fully isolated from production:
- **API port:** 7686 (prod: 7685)
- **Data directory:** `~/.codeterm-dev/` (prod: `~/.codeterm/`)
- **Socket prefix:** `codeterm-dev` (prod: `codeterm`)
- **Single-instance:** Disabled in dev builds, so dev and prod can run side by side.

## Agent Prompts

Agent prompt templates are stored as Markdown files in `src-tauri/prompts/` and loaded at compile time via `include_str!()`. Role-specific suffixes are appended to tasks at spawn time:
- **Worker** (`worker_suffix.md`): Instructions to report back via `codeterm agent report`.
- **Orchestrator** (`orchestrator_suffix.md`): "Do NOT sleep or poll for reports" rule.
- **Cron tasks** (`cron_complete_suffix.md`): Completion instruction with `cron complete --run-id` command.
- **General Agent** (`general_agent.md`): System prompt for the always-on General Agent.
- **CodeTerm Rule** (`codeterm_rule.md`): Base rules injected into all agent prompts.
- **Extra Instructions** (`extra_instructions.md`): Additional context for agent tasks.
- **Cron Report Hint** (`cron_report_hint.md`): Hint for cron agents to produce structured reports.

<!-- revision: 95 -->
