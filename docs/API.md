# CodeTerm API Reference

CodeTerm exposes authenticated REST, local socket JSON-RPC, and the `codeterm` CLI.

### Architecture: Unified Service Layer

All multiplexer operations are implemented in one **ServiceContext**
(`src-tauri/src/service/mod.rs`). REST routes and Tauri IPC delegate to it,
preserving PTY cleanup, events, hierarchy validation, and semantics.

The CLI's local socket dispatches internal `api.request` JSON-RPC through Axum
without TCP. Unix peer credentials or the Windows process-token SID restrict it
to the daemon's OS user; the daemon owns authentication, signing, and transport.

Human `codeterm workspace list` shows authoritative tab counts, cwd, state, Teams,
active Plans, and tabs in stable creation order. Inspect one tab with `codeterm tab
inspect <id>`; whole-list JSON remains automation-only.
`workspace current --json` returns `{ "id", "source" }`, where `source` is
`explicit`, `caller-tab`, `workspace-env`, or `focus-fallback`. Resolution is never
positional: `--workspace`, `$CODETERM_TAB_ID` joined through workspace membership,
a validated `$CODETERM_WORKSPACE_ID`, then the active workspace. When no rung
resolves, stdout stays empty and the command exits 1 (`--json` puts the error
envelope on stdout).

## REST API

Base URL: `http://<host>:7685/api`
**Public vs internal REST surfaces.** This reference documents agent-facing and operator-facing
REST routes. Browser mirror plumbing, debug-clock endpoints, upload/PTY mirror routes, and other
web-only internals live in `src-tauri/src/api/web_routes.rs` and `src-tauri/src/api/routes.rs`.

### Authentication
CodeTerm generates and persists a Bearer token on first load. Control endpoints
require it even on loopback. The small public health/pairing allowlist is
documented by the router and does not grant local control.

Default configuration:
```toml
[api]
host = "127.0.0.1"
port = 7685
cors = false
```
Then: `Authorization: Bearer your-secret-token`

Non-loopback binds require an explicit host and non-empty token; remote requests work only while
Mesh is enabled. Turning Mesh off blocks remote HTTP while preserving localhost. CORS is allowlist-only:

```toml
[api]
host = "192.0.2.10"
token = "replace-with-a-long-local-secret"
cors = true
cors_allow_origins = ["https://console.example"]
```

Authenticated config readback includes `api_listener.active`, `api_listener.configured`, and
`api_listener.restart_required`, covering enabled state, address, CORS/origins, and authentication.
Exact queries include listener truth, so writes cannot masquerade as runtime rebinds. Loopback
hosts canonicalize to valid socket/callback URL forms; rebind/recovery retains generation below.

An empty CORS allowlist permits no cross-origin requests and never disables the listener; `*` and malformed origins are rejected.

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

Closed tabs and workspaces are recorded in a bounded persistent ring. Each close
belongs to a `single`, `cascade`, or `workspace` group identified by an opaque
`group_id`. Oldest entries are evicted first.

| Method | Path | Body / Query | Description |
|--------|------|------|-------------|
| POST | `/api/closed-groups/{group_id}/reopen` | — | Reopen an entire closed group by its `group_id` (from the `recently_closed` snapshot). Recreates the workspace if it no longer exists; if a live workspace already exists at the same `cwd` it is reused — no duplicate is created. Returns `{workspace_id, tab_id}` for the activated tab. |

Restore normalizes the recorded `cwd` and reuses a matching live workspace
instead of creating a duplicate. `GET /api/mux/state` exposes the newest-first
union as `recently_closed: ClosedTabView[]`; each view adds `index`, `orphaned`,
`group_id`, and `group_kind` to its `ClosedTab` fields. The socket equivalents
and response shapes are documented in the API-TRANSPORT reference.

#### Tabs
| Method | Path | Body / Query | Description |
|--------|------|------|-------------|
| GET | `/api/tabs` | `?workspace_id=<id>&agents=<bool>` or `?all_workspaces=true` | List compact tab summaries. Defaults to the focused workspace; use the explicit inspect endpoint for full payloads. |
| GET | `/api/tabs/inspect` | `?tab_ids=<id>,<id>` | Return the full tab payload for the selected tab IDs only. |
| POST | `/api/tabs` | `{title?, description?, cwd?, workspace_id?, cmd?, eager?, chat_backend?, system_prompt?, preset?, model?}` | Create a tab. `description` is trimmed, empty clears it, and values over 200 chars are rejected. When `cmd` is provided, CodeTerm sends it to the fresh tab through the same verified send path used by `/api/tabs/{id}/send`, with Enter appended. When `chat_backend` names an installed `chatBackend` plugin id, an **open agent shell** tab is created instead of a PTY: `system_prompt` overrides the resolved preset's prompt, `preset` selects one (omit → the plugin's `defaultPreset`), `model` picks a model. These three are only meaningful with `chat_backend`. |
| POST | `/api/tabs/{id}/send` | `{text?, raw?, actions?: [{type:"key", key} \| {type:"delay", ms}], batch?: [{text, delay_ms?}], source_tab?}` | The ordinary write path — CLI, mesh relay, and service push tokens all use it. Plain text and every batch item always submit. Use `actions` or `raw` only for explicit low-level terminal input. Not fire-and-forget: returns `{ok:true}` only after delivery is accepted, otherwise a structured delivery error. |
| POST | `/api/tabs/{id}/chat-send` | `{text?, client_token?}` (shares the `/send` body type; only these two are read) | Chat UI user turn. Adds a ledger entry on top of delivery and returns one `ChatSendStatus` (`send_id`, `state`, `ledger_id`, `sequence`, `correlationId?`). `client_token` is echoed verbatim, never interpreted. The CLI does not use this route. |
| GET | `/api/tabs/{id}/chat-send` | — | Read the chat-send ledger for this tab: `ChatSendStatus[]`, reconciled against session-turn evidence on read. `state` ∈ `accepted` \| `unknown` \| `confirmed` \| `failed`. Only lists sends started through `POST /api/tabs/{id}/chat-send`. |
| POST | `/api/tabs/{id}/stop` | — | Stop the tab's PTY process |
| POST | `/api/tabs/{id}/exec` | `{command, timeout_ms?, strip_ansi?}` | Send command + wait for output |
| GET | `/api/tabs/{id}/output` | `?offset=N&strip_ansi=true` | Get output buffer (cursor-based) |
| GET | `/api/tabs/{id}/text` | `?formatted=false` | Visible terminal text (VT100-parsed, alternate buffer included). Served from the backend — no frontend roundtrip, works for hidden tabs. Response: `{"text": "...", "formatted": false, "tab_id": "..."}`. With `?formatted=true` the `text` contains raw ANSI escape codes preserving colors, cursor position, and cursor visibility (used on tab remount to restore TUI apps like Claude that hide the cursor via DECTCEM). |
| GET | `/api/tabs/{id}/usage` | — | Latest usage/cost counters recorded for this tab |
| GET | `/api/tabs/{id}/output/stream` | — | SSE real-time output stream |
| GET | `/api/agent-runs/{tab_id}/log` | — | Agent run log for a tab |
| POST | `/api/tabs/{id}/focus` | — | Focus tab |
| POST | `/api/tabs/{id}/close` | — | Close tab + kill PTY |
| POST | `/api/tabs/{id}/restart` | `{cwd?}` | Replace a plain shell PTY in the same tab while preserving tab and Team metadata. Agent and chat-backend tabs are rejected. |
| POST | `/api/tabs/{id}/rename` | `{title}` | Rename tab |
| POST | `/api/tabs/{id}/describe` | `{description}` | Set or clear the tab description. Server trims whitespace, treats empty as clear, and rejects values over 200 chars. |
| POST | `/api/tabs/{id}/resize` | `{cols, rows}` | Resize terminal |
| GET | `/api/tabs/{id}/chat` | `?since=N` | Stateless transcript read, resolved from the tab's own session record. `{messages, count, pending}`. A tab with no bound session answers `pending: true` with an empty list — never another tab's transcript. Unknown tab → 404 |
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
| GET | `/api/providers` | — | Authenticated provider list alias for agent/provider selectors |
| POST | `/api/agents/spawn` | `{agent?, agent_name?, title?, description?, cwd?, role?, role_profile?, parent_tab_id?, can_spawn?, task?, args?, workspace_id?, model?, reasoning_effort? (alias `reasoning-effort`), system_prompt? (alias `system`), preset?, charter?, tick_interval_secs?, nudge_cooldown_secs?, max_actions_per_tick?, audience?, report_to?, origin_peer?}` | One-call agent spawn (see below). `audience` accepts `user` (default) or `automation`; automation-owned prompts remain available to the harness but never raise user-visible Input-needed notifications. `description` follows the same trim/empty-clear/200-char validation as tab description. `reasoning_effort` is capability-validated against the provider's `reasoning_efforts`: an unsupported value is rejected with a structured error and **no agent launch** (never silently downgraded); the resolved effort and its `{planned,requested,resolved,actual}` provenance ride back on the spawn result. Routing is **by capability, not identity**: if `agent` matches a `chatBackend` shell id, the request opens an open agent shell (threading `system_prompt`/`preset`/`model` and watcher `mode`/`engine` fields into the shell's `openSession` ctx); otherwise the PTY agent path. `role` accepts `Orchestrator`, `Manager`, `Worker`, plus `Planner`/`Watcher` sugar that maps to `Manager` with `role_profile`. Watcher spawns require a watcher-capable `chatBackend` shell and `charter`; policy fields are clamped by the host. An unknown id errors with the available agent **and** shell ids. `report_to` sets where the worker's `agent report` lands (Tab id or `main`/`general` alias). `origin_peer` is **never client-supplied** — the mesh relay stamps it from the authenticated sender so a worker spawned via `agent spawn --mesh PEER` reports back across the mesh; see *Remote mesh workers*. |
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

#### Plan task mutation authority

Task-mutating calls (socket `plan.task.*`, REST `/api/plans/{id}/tasks/{tid}*`)
carry caller/session/binding/owner-epoch authority (REST: matching
`X-CodeTerm-*` headers). Once owned, the caller must match the recorded owner.
`GET /api/tasks` and socket `task.list` return the same object: `tasks` plus an
optional `empty_reason`. Its `kind` is `plan_not_found`, `plan_archived`,
`plan_outside_project`, `no_plans_in_scope`, `no_tasks_in_scope`,
`all_tasks_inactive`, or `no_match_for_filters`. The field is absent when tasks
exist; consumers ignore unknown kinds and never invent a local explanation.

A live authenticated Plan owner may recover `update`, `assign`, `respawn`, or
`close` after its tab disappears; retirement and recovery commit atomically.
Mutating dispatch into canonical or another task's lane is refused with `-32013`;
competition provisions one lane per competitor. Advisory findings ride successful
results in `safety_nudges` and `_hints`. See `codeterm docs ORCHESTRATION`.
Related: `codeterm docs API-TABS`, `API-INTEGRATIONS`, `API-MEMORY`, `API-AGENTS`,
`API-TRANSPORT`, and `API-CLI`.
<!-- revision: 119 -->
