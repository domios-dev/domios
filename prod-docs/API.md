# CodeTerm API Reference
CodeTerm exposes authenticated REST, local socket JSON-RPC, and the `codeterm` CLI.

### Architecture: Unified Service Layer

All multiplexer operations are implemented in one **ServiceContext**
(`src-tauri/src/service/mod.rs`). REST routes and Tauri IPC delegate to it,
preserving PTY cleanup, events, hierarchy validation, and semantics.

The CLI's local socket dispatches internal `api.request` JSON-RPC through Axum
without TCP. Unix peer credentials or the Windows process-token SID restrict it
to the daemon's OS user; the daemon owns authentication, signing, and transport.

Every socket and mesh RPC envelope carries a `protocol_version`; an older or
versionless client is refused with `-32002`. Wire shape: `codeterm docs
API-TRANSPORT`.

Human `codeterm workspace list` shows authoritative topology in stable creation
order. Inspect one tab with `codeterm tab inspect <id>`; whole-list JSON is automation-only.
`workspace current --json` returns `{ "id", "source" }` with `source` one of
`explicit`, `caller-tab`, `workspace-env`, `focus-fallback`. Resolution is never
positional: `--workspace`, `$CODETERM_TAB_ID` through workspace membership, a
validated `$CODETERM_WORKSPACE_ID`, then the active workspace. With no rung,
stdout stays empty and the command exits 1 (`--json` puts the error on stdout).

## REST API
Base URL: `http://<host>:7685/api`
This reference covers agent- and operator-facing routes; web-mirror and debug internals are not listed.

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
closes. Listener/server exit autonomously retires
that generation and creates exactly one replacement; bind ownership failure
waits for an explicit port-release or restart event instead of retrying on a timer.
The internal `get_rest_api_status` command returns `generation`, `phase`,
`diagnostic`, and `message`. Diagnostic values are `listener_exited`,
`bind_ownership_failed`, or `workers_blocked`; the last means the listener
remains bound but the four REST runtime workers cannot answer health.

Each successful outage transition also owns a monotonic recovery id. Recovery
events are queued, so two worker stalls recovered within one listener generation
each produce one mesh resync while observing an event twice produces none.
REST chat/transcript routes parse a bounded 4 MiB tail on blocking workers.

#### Workspaces
| Method | Path | Body | Description |
|--------|------|------|-------------|
| * | `/api/workspaces/...` | *(per route)* | Workspace and workspace-scoped tab routes: list, create, focus, rename, close, recently-closed, reopen, and tab create/rename/move — all in API-TABS |

POST routes whose fields only refine a complete action (agent restart, hooks, serve enable, keepawake start, session resume, reopen-closed, workspace tab create) accept no body and apply documented defaults. Workspace close has a caller-sensitive confirmation body; see [API-TABS](API-TABS.md). `POST /api/workspaces` requires `name` (`400`).

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
| GET | `/api/tabs` | `?workspace_id=<id>&agents=<bool>` or `?all_workspaces=true` | List compact tab summaries. Without `workspace_id`, the default is every local workspace; `workspace_id` is the explicit one-workspace scope. `focused_workspace=true` scopes to the focused workspace, using the caller tab identity when supplied. Every row carries `workspace_id`, `workspace_name`, `scope` (`all_workspaces` \| `workspace`), `scope_workspace_id`, `scope_workspace_name`, and `other_workspace_tab_count`. Each row carries `agent_liveness` (`live` \| `absent` \| `unobserved`) — the one observation of whether an agent process is running in that tab — and `agent_label`, the rendered bracket for it: the short model name when a provider catalogue claims the model id, the provider name when none does, suffixed `?` when unobserved and `!` when absent. While tab state initializes or transitions, the route returns `503` (`tab summary unavailable while tab state is initializing or changing`) instead of waiting on state locks. |
| GET | `/api/tabs/inspect` | `?tab_ids=<id>,<id>` | Return the full tab payload for the selected tab IDs only. |
| POST | `/api/tabs` | `{title?, description?, cwd?, workspace_id?, team_id?, creator_tab_id?, detach_from_team?, cmd?, eager?, chat_backend?, system_prompt?, preset?, model?}` | Create a tab. `title` is one word; put activity prose in `description`. Workspace resolution: explicit `workspace_id`, then the workspace owning `creator_tab_id` (the CLI always sends the calling tab), then the focused workspace. A PTY shell inherits the live creator's Team unless `detach_from_team` is true; explicit `team_id` is membership-checked. Chat-backend tabs do not inherit Team membership. `description` is trimmed, empty clears it, and values over 200 chars are rejected. `cmd` goes through the `/send` path with Enter appended. When `chat_backend` names an installed `chatBackend` plugin id, an **open agent shell** tab is created instead of a PTY: `system_prompt` overrides the resolved preset's prompt, `preset` selects one (omit → the plugin's `defaultPreset`), `model` picks a model. These three need `chat_backend`. |
| POST | `/api/tabs/{id}/send` | `{text?, raw?, actions?: [{type:"key", key} \| {type:"delay", ms}], batch?: [{text, delay_ms?}], source_tab?}` | The ordinary write path — mesh relay and service push tokens use it. Plain text and every batch item submit. Use `actions` or `raw` only for explicit low-level terminal input. Returns `delivery:"sent"` after confirmed delivery, `delivery:"pending"` when an enveloped send is accepted by a background route, or a structured delivery error (including whether bytes were written). |
| POST | `/api/tabs/{id}/chat-send` | `{text?, client_token?}` (shares the `/send` body type; only these two are read) | Chat UI user turn. Adds a ledger entry on top of delivery and returns one `ChatSendStatus` (`send_id`, `state`, `ledger_id`, `sequence`, `correlationId?`). `client_token` is echoed verbatim, never interpreted. The CLI does not use this route. |
| POST | `/api/tabs/{id}/attachments` | `{kind: "voice"\|"file", name, size, data}` (`data` is base64, at most 25 MiB decoded) | Target-host attachment ingest for mesh sends. Refuses oversized (`413`), malformed, empty or size-mismatched (`400`) payloads, sanitizes `name`, and stores the bytes in this host's upload directory. Returns `{path, kind, size, reference}`: `path` is local to this host; for `voice`, `reference` is the voice marker transcribed here. Peers reach it through the signed relay with the same access as `/send`. |
| POST | `/api/mesh/attachments/transfer` | `{ordinary_tab_id, peer_id, path, kind}` | Origin-host handoff: reads a local attachment, sends its bytes to the owning peer's `POST /api/tabs/{id}/attachments`, and returns that peer's `{path, kind, size, reference}`. A transfer failure is an error, never the original path. Remote sends from this host (`/send`, `/chat-send`, `/delayed-messages`) are refused with `source_attachment_path` while their text still names an attachment stored here. |
| GET | `/api/tabs/{id}/chat-send` | — | Read the chat-send ledger for this tab: `ChatSendStatus[]`, reconciled against session-turn evidence on read. `state` ∈ `accepted` \| `unknown` \| `confirmed` \| `failed`. Only lists sends started through `POST /api/tabs/{id}/chat-send`. |
| POST | `/api/tabs/{id}/stop` | — | Stop the tab's PTY process |
| POST | `/api/tabs/{id}/exec` | `{command, timeout_ms?, strip_ansi?}` | Send command + wait for output |
| GET | `/api/tabs/{id}/output` | `?offset=N&strip_ansi=true` | Get output buffer (cursor-based) |
| GET | `/api/tabs/{id}/text` | `?formatted=false` | Visible terminal text (VT100-parsed, alternate buffer included). Backend-served, works for hidden tabs. Response: `{"text", "formatted", "tab_id"}`; `?formatted=true` keeps raw ANSI including cursor position and visibility. |
| GET | `/api/tabs/{id}/usage` | — | Latest usage/cost counters recorded for this tab |
| GET | `/api/tabs/{id}/output/stream` | — | SSE real-time output stream |
| GET | `/api/agent-runs/{tab_id}/log` | — | Agent run log for a tab |
| POST | `/api/tabs/{id}/focus` | — | Focus tab |
| POST | `/api/tabs/{id}/close` | — | Close tab + kill PTY |
| POST | `/api/tabs/{id}/restart` | `{cwd?}` | Replace a plain shell PTY in the same tab while preserving tab and Team metadata. Agent and chat-backend tabs are rejected, including an agent-role tab whose provider exited (`shell_restart_agent_member`; resume it with `agent restart`). |
| POST | `/api/tabs/{id}/rename` | `{title}` | Rename tab. |
| POST | `/api/tabs/{id}/describe` | `{description}` | Set or clear the tab description. Server trims whitespace, treats empty as clear, and rejects values over 200 chars. |
| POST | `/api/tabs/{id}/resize` | `{cols, rows}` | Resize terminal |
| GET | `/api/tabs/{id}/chat` | `?since=N` | Stateless transcript read, resolved from the tab's own session record. `{messages, count, pending}`. A tab with no bound session answers `pending: true` with an empty list — never another tab's transcript. Unknown tab → 404 |
| * | `/api/tabs/{id}/session/...` | *(per route)* | Tab session binding: read the daemon's binding fact, or rebind a running tab to an existing provider session without restarting it — both in API-TABS |
| POST | `/api/tabs/{id}/repair` | `{provider? \| session_id? \| from_evidence? \| from_journal?}` | Repair a live PTY tab's provider/session identity without restarting it. Modes are mutually exclusive; an empty body defaults to `from_evidence`. Caller proof comes from `X-CodeTerm-Tab-Id` and `X-CodeTerm-Binding-Nonce`, plus `X-CodeTerm-Session-Id` when the launch already knew it; the proven caller must be the target tab or the registered owner of the target's bound plan task. Provider writes require a nonce-verified session record and go through the journaled identity boundary. Response shape, journal mode, and refusals: see API-REPAIR. |
| POST | `/api/tabs/{id}/adopt-agent` | See API-ADOPT | Adopt a live PTY; see API-ADOPT. |
| * | `/api/tabs/{id}/delayed-messages`, `/api/delayed-messages/{id}` | *(per route)* | List, queue, edit, delete, and send-now for delayed Chat UI messages — all in API-TABS |
| POST | `/api/tabs/{id}/hook/{permission,session_state,stop,session_start}` | *(per-hook, see below)* | Agent lifecycle hooks for one tab. Payloads, modes, nonce rejection and responses live in API-TABS |
| POST | `/api/tabs/{id}/agent/restart` | `{force?: bool, grace_secs?: u64}` | Restart an observed live agent, or resume an exited provider's verified owned session in its shell. See [Agent Restart](#agent-restart-post-apitabsidagentrestart). |

#### Chat attachment authority

The desktop `chat_start` command and web-mirror
`POST /api/tabs/{id}/chat-raw/start` return the same bounded snapshot shape.
`source_session_id` and `session_binding_nonce` are derived from the Tab and
form one authority generation. A client adopts snapshots and
`tab-chat-msg-{id}` deltas only when both values match the Tab's current
generation.

`pending=true` without session authority means the Tab awaits its own session and
never falls back to another; with authority and a subscription it awaits its own
store. `pending=false` is a complete bounded snapshot, possibly empty. Visible Chat
owns one subscription and takes a fresh snapshot on reactivation or resync.

#### Agent Hierarchy
| Method | Path | Body / Query | Description |
|--------|------|------|-------------|
| POST | `/api/tabs/{id}/role` | `{role, parent_tab_id?, can_spawn?, plan_id?}` | Set tab role (`Orchestrator` or `Worker`; `Conductor` accepted as an alias for `Orchestrator`). A `Manager` with `role_profile: "planner"` requires `plan_id` and resolves its parent from the plan's live Orchestrator owner when `parent_tab_id` is omitted, rather than refusing; an explicit `parent_tab_id` still wins when given. |
| DELETE | `/api/tabs/{id}/role` | — | Clear role: detach a Worker from its Orchestrator, or retire an Orchestrator while retaining the final anchor's Team and active Plan for replacement |
| GET | `/api/tabs/{id}/status` | — | Tab status + role + children info, including `pending_prompt` when a numbered provider prompt is active (`question` and `options`). `codeterm tab status --json` returns the same prompt data. |
| POST | `/api/tabs/{id}/report` | `{message, caller_tab_id?, caller_session_id?, session_binding_nonce?, plan_id?, task_id?, status?, ...}` | Send an authenticated hierarchy report. A Plan-authorized task-less reporter uses `plan_id` without `task_id`/`status`; success returns `plan_report` and persists a task-less Plan stride before Orchestrator delivery. |
| GET | `/api/tabs/{id}/reports` | `?clear=true` | Read reports from children (optionally clearing them) |
| POST | `/api/tabs/{id}/can-spawn` | `{can_spawn: bool}` | Toggle whether tab can spawn children |
| POST | `/api/tabs/{id}/agent-type` | `{agent_type}` | Mark a tab as running a specific agent provider |
| POST | `/api/tabs/{id}/prompt/respond` | `{value}` | Respond to the active numbered prompt. `codeterm tab respond <option> --tab <id>` uses the one-based option shown by status and refuses when the prompt is no longer on screen. |
| POST | `/api/tabs/{id}/prompt/clear` | — | Clear a pending permission prompt |
| GET | `/api/agents/providers` | — | Provider catalog. Entries expose `id`, `display_name`, models, reasoning efforts/defaults, settings, and `kind`: `agent` (PTY) or `shell` (chat backend). Per-model efforts and defaults override the provider list (API-AGENTS). Shells also expose presets and Chat UI support. `availability` is `available`, `unavailable`, or `unknown`; PTY lookup uses `binary_name`; backends use optional `cli_binary`. Spawn refuses `unavailable` before creating a tab; `unknown` warns. Resolver and cache details: [provider contract](../.agents/skills/provider-authoring/SKILL.md#launch-binary-availability). |
| GET | `/api/providers` | — | Authenticated provider list alias for agent/provider selectors |
| POST | `/api/agents/spawn` | `{agent?, agent_name?, title?, description?, cwd?, role?, role_profile?, parent_tab_id?, plan_id? (alias `plan`), can_spawn?, task?, task_id?, plan_task? (alias `plan-task`), execution_intent?, authorized_gates?, args?, workspace_id?, model?, reasoning_effort? (alias `reasoning-effort`), system_prompt? (alias `system`), preset?, charter?, tick_interval_secs?, nudge_cooldown_secs?, max_actions_per_tick?, audience?, report_to?, origin_peer?, team_id?, creator_tab_id?}` | One-call agent spawn (see below). `audience` is `user` (default) or `automation`; automation-owned prompts never raise user-visible Input-needed notifications. `description` follows the same trim/empty-clear/200-char validation as tab description. `reasoning_effort` is capability-validated before launch, never silently downgraded (see API-AGENTS). Routing is **by capability, not identity**: if `agent` matches a `chatBackend` shell id, the request opens an open agent shell (`system_prompt`/`preset`/`model` and watcher `mode`/`engine` reach its `openSession` ctx); otherwise the PTY agent path. `role` accepts `Orchestrator`, `Manager`, `Worker`, plus `Planner`/`Watcher` sugar that maps to `Manager` with `role_profile`. A `Planner` spawn requires `plan_id` and resolves an omitted parent as the role endpoint does. Watcher spawns require a watcher-capable `chatBackend` shell and `charter`; policy fields are clamped by the host. An unknown id errors with the available agent **and** shell ids. `report_to` sets where the worker's `agent report` lands (Tab id or `main`/`general` alias). `origin_peer` is **never client-supplied**: the mesh relay stamps it from the authenticated sender so a mesh-spawned worker reports back; see *Remote mesh workers*. `team_id` + `creator_tab_id` (`codeterm agent spawn --team`) attach a ROLELESS agent to an existing Team: `creator_tab_id` must already belong to `team_id`, and the pair is rejected alongside `role` or `plan_task`. Membership is checked before tab creation, so a refused caller leaves nothing behind. Socket `team.adopt_agent` applies the same check to an already-running tab; see API-TRANSPORT. A **roled** Team member (including a replacement Orchestrator) uses `POST /api/teams/{team_id}/members` / socket `team.add_member`. The task-less verification Worker (`execution_intent: "read_only"`, `authorized_gates`) is specified in API-AGENTS. |
| POST | `/api/agents/helper` | `{title, task}` | Shared spawn for UI AI helper buttons (Configure with AI, Mesh Set up with AI). The daemon resolves the General Agent provider, places the tab in the General workspace with the General working directory regardless of the active workspace, and starts the task as a roleless visible launch that always carries a first turn. Any other field is refused. A missing General workspace, launch failure or undelivered first turn returns an error and rolls the tab back. `delivery_state` is `confirmed` only when the first turn was submitted through the PTY with submit evidence, and `launched` when the provider consumed it inline at launch without first-turn evidence. |
| GET | `/api/remote-workers` | `?tab=<id>` | Orchestrator-side view of remote (cross-mesh) workers, populated from inbound mesh reports. With `?tab=` returns that orchestrator's workers; otherwise the full `{ tab_id: RemoteWorker[] }` map. `RemoteWorker = {peer, remote_tab, title, model, status, last_message, last_seen_ms}`. Live updates via the `remote-workers-changed` event. |

Configure-with-AI, provider install, auto-confirm, the Pebble webhook and plugin receiver targets: `codeterm docs API-INTEGRATIONS`.

#### Interactive chat prompts

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

#### Pasteboard (cross-mesh ephemeral payloads)
| Method | Path | Body | Description |
|--------|------|------|-------------|
| POST | `/api/pasteboard` | `{body, content_type?, ttl_ms?, pop_on_read?}` | Stash a payload. Returns `{id: "paste:NNNNNN", expires_at}`. TTL default 1h, max 24h. 20-entry cap with oldest-first eviction. No disk persistence: a restart wipes every paste. |
| GET | `/api/pasteboard/{id}` | — | Fetch body. `?pop=true` deletes after read. Returns 404 if expired/unknown. |
| DELETE | `/api/pasteboard/{id}` | — | Explicit delete. |
| GET | `/api/pasteboard` | — | List local pastes (id, size, ttl_remaining_ms, content_type). |

#### Plan task mutation authority

Task-mutating calls (socket `plan.task.*`, REST `/api/plans/{id}/tasks/{tid}*`)
carry caller/session/binding/owner-epoch authority (REST: matching
`X-CodeTerm-*` headers). Once owned, the caller must match the recorded owner.
`GET /api/tasks` and socket `task.list` return the same object: `tasks` plus an
optional `empty_reason`. Its `kind` is `plan_not_found`, `plan_archived`,
`plan_outside_project`, `no_plans_in_scope`, `no_tasks_in_scope`,
`all_tasks_inactive`, or `no_match_for_filters`. The field is absent when tasks
exist; consumers ignore unknown kinds and never invent a local explanation.

Batch writes (socket `plan.batch`, REST `POST /api/plans/{id}/batch`, both
reached by `codeterm plan record`) cross the same boundary from either entrance:
an op naming a task another tab holds is refused unless the caller is that holder
or the plan's owner. Callers name themselves with `caller_tab_id`; an
unidentified caller cannot claim a hold. The refusal carries `STALE TASK BINDING`
(socket `-32003`, REST `403`) with `type: "stale_task_binding"` plus
`attempted_by_tab`, `plan_id` and `task_id`,
and nothing is written. A bearer token proves a client is permitted, never that
it holds the task.

A live authenticated Plan owner may recover `update`, `assign`, `respawn`, or
`close` after its tab disappears; retirement and recovery commit atomically.
Mutating dispatch into canonical or another task's lane is refused with `-32013`;
competition provisions one lane per competitor. Advisory findings ride successful
results in `safety_nudges` and `_hints`. See `codeterm docs ORCHESTRATION`.
Related docs: `API-PLAN`, `API-TABS`, `API-INTEGRATIONS`, `API-MEMORY`,
`API-TRANSPORT`, `API-CLI`, and `TELEGRAM-SETUP`.
Local `codeterm send` uses the socket `tab.send_text` route: it prints
`OK (sent)` after confirmed delivery, `OK (pending)` for accepted or
unconfirmed delivery, and an explicit refusal when no payload bytes were written.
The resolved payload echo is emitted for confirmed delivery and for pending sends that report written bytes.
<!-- revision: 170 -->
