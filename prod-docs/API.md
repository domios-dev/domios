# Domios API Reference
Transports: authenticated REST, local socket JSON-RPC and `domios`.

### One behavior across transports

REST, socket and CLI share authorization, PTY cleanup, events and hierarchy checks.

The CLI socket needs no TCP. Unix peer credentials or Windows process-token SID restrict it to the daemon's OS user; the daemon owns authentication, signing and transport.

Socket/mesh envelopes require `protocol_version`; older or versionless clients get `-32002`. Wire: `API-TRANSPORT`; peer notice routes: `MESH`.

Human `domios workspace list` shows authoritative topology in creation order; `domios tab inspect <id>` inspects one tab (whole-list JSON is automation-only). `workspace current --json` returns `{ "id", "source" }`; sources, in resolution order: `explicit` (`--workspace`), `caller-tab` (`$CODETERM_TAB_ID` membership), `workspace-env` (validated `$CODETERM_WORKSPACE_ID`), `focus-fallback` (active workspace). No match: exit 1, empty stdout (`--json` writes the error to stdout).

## REST API
Base: `http://<host>:7685/api`.

### Authentication
The Bearer token lives in owner-only `<data_dir>/.secrets` as `domios.instance:api_token`, read once at startup and passed to tabs as `CODETERM_API_TOKEN`, never stored in `config.toml`. Local-only `domios system api-token` prints the credential and refuses `--mesh`. Control endpoints require it even on loopback; public health/pairing grants no control.

Default configuration:
```toml
[api]
host = "127.0.0.1"
port = 7685
cors = false
```
Then: `Authorization: Bearer your-secret-token`

Non-loopback binds require an explicit host and non-empty token; remote requests work only while Mesh is enabled. Turning Mesh off blocks remote HTTP while preserving localhost. CORS is allowlist-only:

```toml
[api]
host = "192.0.2.10"
cors = true
cors_allow_origins = ["https://console.example"]
```

Config readback exposes actual and configured listener state through `api_listener.active`, `.configured`, `.restart_required` (enabled, address, CORS/origins, authentication); writes do not imply a runtime rebind. Loopback hosts canonicalize to valid socket/callback URLs; recovery follows the generation contract below. Empty CORS allowlists deny cross-origin requests without disabling the listener; `*` and malformed origins are rejected.

### Endpoints

#### CI failure events

POST `/api/ci/webhook` uses a dedicated Bearer credential from the secret-store entry `ci-webhook`, a JSON registration `{token, repo, plan}`. It accepts `{repo, run, attempt, sha, job, step, test, exit, log_url}`; `test` and `exit` may be null when unavailable. Run and attempt are positive decimal strings, SHA is 40 hex characters, and the log URL must name that repository/run/attempt on GitHub. Authenticated events route one message and notification to the registered plan's live, session-verified Orchestrator. Missing owners fail delivery; events never fall back to another tab. Duplicate repository/run/attempt/job events are suppressed by the gateway's bounded process-local cache. Requests are capped at 16 KiB and retain the API's Tailnet access restrictions.

#### Startup readiness

Health `status: "ok"` proves liveness; `startup.state: "ready"` proves readiness. Only loopback health/events bypass auth. Await `ready` on `/api/startup/events` (`startup-state`). Other requests get HTTP 503 `starting`/`startup_failed` without queuing. CLI: `API-CLI`.

#### Account machines

Local account routes refuse Mesh callers. POST `/api/account/mesh/session-revoke` accepts `{installation_id?: string | null}`; omission selects this machine. It signs out the selected installation's account sessions and returns `{ok: true, result: {target_installation_id, revoked_at}}` or the standard typed refusal. The local installation key and saved pairings remain. The UI asks for confirmation before this action.

#### Local previews
| Method | Path | Body | Description |
|--------|------|------|-------------|
| POST | `/api/tabs/{id}/preview/read` | `{path, kind}` | Tab-scoped read; see `API-STATE` |
| POST | `/api/tabs/{id}/preview/check` | `{paths}` | Tab-scoped existence check |

#### System
| Method | Path | Body | Description |
|--------|------|------|-------------|
| GET | `/api/health` | — | Health check. Returns `version`, `build` (git short hash), `name`, `platform`, `status`. |
| POST | `/api/automation/blueprints/export` | `{workspace_id, team_id, id}` | Local host `{name, yaml}` authoring file; see BLUEPRINTS. |
| POST | `/api/automation/blueprints/solutions/show` | `{workspace_id, id}` | Local host read-only saved solution, with the same workspace visibility as solutions/list. Accepts a bare ID or `mem:<id>`. Returns `{document, expanded_calls}`; see below. |
| POST | `/api/automation/blueprints/import` | `{caller_tab_id, recipe, assignments}` | Local host apply by name, stopped; recipe is YAML text or the matching document object; see BLUEPRINTS. |
| GET | `/api/automation/blueprints/project` | Query `workspace_id`, optional `path` | Local host read-only repository listing or bounded source view. No execution or approval. |
| GET | `/api/automation/blueprints/viewer/history` | Query `workspace_id`, explicit `team_id` (empty for no Team), `id` | Retained definition versions with actual root/callee semantic diffs. |
| GET | `/api/identify` | — | Version, platform, features |
| POST | `/api/cli/rpc` | `{method, params?, protocol_version}` | Internal CLI fallback for authenticated loopback callers only. It dispatches the same RPC methods as the local socket; mesh peer tokens are refused. See API-CLI. |
| GET | `/api/network` | — | Local IP, Tailscale IP, API URLs |
| GET | `/api/keepawake` | — | Get keep-awake status (see below) |
| POST | `/api/keepawake/start` | `{duration_secs?, display?}` | Start / restart keep-awake session |
| POST | `/api/keepawake/stop` | — | Stop keep-awake session |
| POST | `/api/graph/events/{subscription_id}` | `{plugin_id, event_id, payload}` | Plugin event ingress. Uses the subscription's Bearer token, accepts at most 64 KiB, and returns 202 for admission; refusal codes map to 401, 403, 404, 409, 413, 422, 429 or 503. |

REST listener lifecycle and recovery: MESH, *Local REST availability*. REST chat/transcript routes parse a bounded 4 MiB tail.

Saved solution show returns a bounded, credential-redacted `document` and recursive `expanded_calls: [{node, pin: {id, hash}, graph, children}]`, matching applied Blueprint graph expansion. Each node's `call.ref` identifies the authored reference. Missing, corrupt or foreign-scope pinned revisions produce `{node, pin, graph: null, children: [], unavailable: "pinned_revision_unavailable"}` without failing the response. Expansion limits are eight call levels, 64 call entries and 256 KiB of callee documents; an exceeded depth or byte budget produces `unavailable: "budget_exhausted"`. Callees use the saved solution's recorded Team/workspace, never current callee heads. Archived or foreign-workspace root IDs are refused, as are peer-origin requests.

#### Workspaces
| Method | Path | Body | Description |
|--------|------|------|-------------|
| * | `/api/workspaces/...` | *(per route)* | Workspace and workspace-scoped tab routes: list, create, focus, relocate cwd, rename, close, recently-closed, reopen, and tab create/rename/move — all in API-TABS |

POST routes whose fields only refine a complete action (agent restart, hooks, serve enable, keepawake start, session resume, reopen-closed, workspace tab create) accept no body and apply documented defaults. Workspace close has a caller-sensitive confirmation body (API-TABS). `POST /api/workspaces` requires `name` (`400`).

#### Closed History & Restore

A bounded persistent ring records closed tabs/workspaces, evicting oldest first. Each close has an opaque `group_id` of kind `single`, `cascade`, or `workspace`.

| Method | Path | Body / Query | Description |
|--------|------|------|-------------|
| POST | `/api/closed-groups/{group_id}/reopen` | — | Reopen a whole closed group by `group_id` (from `recently_closed`). Recreates a missing workspace; a live workspace at the same `cwd` is reused, never duplicated. Returns `{workspace_id, tab_id}` for the activated tab. |

Restore normalizes the recorded `cwd` before matching. `GET /api/mux/state` exposes the newest-first union as `recently_closed: ClosedTabView[]`; each view adds `index`, `orphaned`, `group_id`, and `group_kind` to its `ClosedTab` fields. Socket equivalents and shapes: API-TRANSPORT.

#### Tabs
| Method | Path | Body / Query | Description |
|--------|------|------|-------------|
| GET | `/api/shells` | `?refresh=true` | Shell profiles; refresh replaces cache. |
| GET | `/api/tabs` | `?workspace_id=<id>&agents=<bool>` or `?all_workspaces=true` | Summaries: `workspace_id` scopes one, `all_workspaces=true` lists all; `focused_workspace=true` uses caller identity, else focus. Rows: `workspace_id`, `workspace_name`, `scope` (`all_workspaces` \| `workspace`), `scope_workspace_id`, `scope_workspace_name`, `other_workspace_tab_count`, `agent_liveness` (`live` \| `absent` \| `unobserved`), `agent_label` (model/provider; `?` unobserved, `!` absent), `status` matching `/status` (`working` \| `question` \| `idle` \| `error` \| `exited`). Writer locks held over 5 s return `503`. |
| POST | `/api/tabs/{id}/wake` | — | Local-owner request to wake a sleeping tab; returns `202 {accepted:true}` immediately. Backend materialization runs off the async runtime; observe tab state for readiness. Repeated requests materialize once while a wake runs. A failed wake publishes `spawn_failed` and `boot_failure_reason`; an explicit new wake request retries it. |
| GET | `/api/tabs/inspect` | `?tab_ids=<id>,<id>` | Full tab payload for the selected tab IDs only. |
| POST | `/api/tabs` | `{title?, description?, cwd?, workspace_id?, team_id?, creator_tab_id?, detach_from_team?, cmd?, eager?, chat_backend?, system_prompt?, preset?, model?}` | Create tab: `title` one word, max 64 chars; `description` trimmed one line, max 200 chars, empty clears. Workspace: explicit, creator-owned, then focus. PTY shells inherit the live creator Team unless `detach_from_team`; explicit `team_id` is membership-checked; chat backends never inherit. `cmd` uses `/send` plus Enter. `chat_backend` opens an installed `chatBackend` shell instead of a PTY; only these accept `system_prompt`, `preset` (plugin `defaultPreset` default), and `model`. |
| POST | `/api/tabs/{id}/send` | `{text?, raw?, actions?: [{type:"key", key} \| {type:"delay", ms}], batch?: [{text, delay_ms?}], source_tab?}` | The ordinary write path, also used by mesh relay and service push tokens. Plain text and every batch item submit; `actions`/`raw` are only for explicit low-level terminal input. Delivery results (`sent`, `pending`, or a structured error with `bytes_written`): API-CHAT. |
| POST | `/api/tabs/{id}/chat-send` | *(per route)* | Chat send and transcript routes: API-CHAT |
| POST | `/api/tabs/{id}/attachments` | `{kind: "voice"\|"file", name, size, data}` (`data` is base64, at most 25 MiB decoded) | Target-host attachment ingest for mesh sends. Refuses oversized (`413`), malformed, empty or size-mismatched (`400`) payloads, sanitizes `name`, stores bytes in this host's upload directory. Returns `{path, kind, size, reference}`: `path` is host-local; for `voice`, `reference` is the voice marker transcribed here. Peers reach it through the signed relay with `/send` access. |
| POST | `/api/mesh/attachments/transfer` | `{ordinary_tab_id, peer_id, path, kind}` | Origin-host handoff: sends a local attachment's bytes to the owning peer's `POST /api/tabs/{id}/attachments` and returns that peer's `{path, kind, size, reference}`. Transfer failure is an error, never the original path. Remote sends from this host (`/send`, `/chat-send`, `/delayed-messages`) whose text still names an attachment stored here are refused with `source_attachment_path`. |
| GET | `/api/tabs/{id}/chat-send` | *(per route)* | Chat send and transcript routes: API-CHAT |
| POST | `/api/tabs/{id}/stop` | — | Interrupt an active turn through structured abort or the provider-declared stop sequence. `stopped:true` follows a terminal status event; with no active turn nothing is sent (`{stopped:false, reason:"not_working", status}`), and without a stop capability the response is `{stopped:false, reason:"stop_unsupported"}`. |
| POST | `/api/tabs/{id}/exec` | `{command, timeout_ms?, strip_ansi?}` | Run a shell command until the shell reports it finished or exits: `{output, offset, exit_code, completion}` (`command_finished` \| `shell_exited` \| `timeout`). `timeout_ms` (default 10000) is a failure deadline that adds `timeout:true`. `409` for an agent tab or an exited shell. `domios tab exec` exits with the command's code, `124` on timeout |
| GET | `/api/tabs/{id}/output` | `?offset=N&strip_ansi=true` | Get output buffer (cursor-based) |
| GET | `/api/tabs/{id}/text` | `?formatted=false` | Visible terminal text (VT100-parsed, incl. alternate buffer), backend-served so hidden tabs work. Response: `{"text", "formatted", "tab_id"}`; `?formatted=true` keeps raw ANSI including cursor position and visibility. |
| GET | `/api/tabs/{id}/usage` | — | Latest usage/cost counters for this tab |
| GET | `/api/tabs/{id}/agent/runtime/login` | — | Read the exact runtime's pending URL, user code and `expiresAtMs`; authenticated local callers and permitted paired peers only; `Cache-Control: no-store` |
| POST | `/api/tabs/{id}/agent/runtime/login` | `{method:"device"}` or `{action:"notify"}` or `{action:"cancel"}` | Start a declared login flow, request one phone notification or cancel it. API-key entry stays local. |
| GET | `/api/tabs/{id}/agent/runtime/usage` | — | Exact-tab limits, account and reset credits |
| POST | `/api/tabs/{id}/agent/runtime/reset-credits` | `{action:"read"}` or `{action:"redeem", idempotencyKey, confirmed:true, creditId?}` | Read support, or redeem one reset credit (UUID v4 key + confirmation; `creditId` omitted = provider picks); returns outcome and before/after counts |
| GET | `/api/tabs/{id}/output/stream` | — | SSE real-time output stream |
| GET | `/api/agent-runs/{tab_id}/log` | — | Agent run log for a tab |
| POST | `/api/tabs/{id}/focus` | — | Focus tab |
| POST | `/api/tabs/{id}/close` | `?force=true` | Close tab + kill PTY. With `X-CodeTerm-Tab-Id`, closing a Worker that executes an `in_progress` task returns `409` unless `force=true`; without it the close proceeds and returns `warnings`. The Orchestrator is notified either way (see `tab.close` in API-TRANSPORT). |
| POST | `/api/tabs/{id}/restart` | `{cwd?}` | Replace a plain shell PTY in the same tab while preserving tab and Team metadata. Agent and chat-backend tabs are rejected, including an agent-role tab whose provider exited (`shell_restart_agent_member`; resume it with `agent restart`). |
| POST | `/api/tabs/{id}/rename` | `{title}` | Rename tab. |
| POST | `/api/tabs/{id}/describe` | `{description}` | Set or clear the tab description. Server trims whitespace, treats empty as clear, and rejects values over 200 chars. |
| POST | `/api/tabs/{id}/resize` | `{cols, rows}` | Resize terminal |
| GET | `/api/tabs/{id}/chat` | *(per route)* | Chat send and transcript routes: API-CHAT |
| GET | `/api/tabs/{id}/agent/runtime/children/{thread}/history` | `rootThreadId`, `bindingNonce`, `epoch`, optional `cursor` | Read one recorded subagent's history under the current parent binding. Returns `child` metadata, `items`, `nextCursor` and echoed binding fields. Foreign, unrecorded and stale children are refused; this grants no send or rebind authority and creates no tab. |
| * | `/api/tabs/{id}/session/...` | *(per route)* | Read binding state or rebind an existing provider session without restart: API-TABS. |
| POST | `/api/tabs/{id}/repair` | `{provider? \| session_id? \| from_evidence? \| from_journal?}` | Repair a live PTY provider/session without restart; self or bound Plan-owner authority. Modes, caller headers, response and refusals: API-REPAIR. |
| POST | `/api/tabs/{id}/adopt-agent` | See API-ADOPT | Adopt a live PTY; see API-ADOPT. |
| * | `/api/tabs/{id}/delayed-messages`, `/api/delayed-messages/{id}` | *(per route)* | List, queue, edit, delete, and send-now for delayed Chat UI messages — all in API-CHAT |
| POST | `/api/tabs/{id}/hook/{permission,session_state,stop,session_start}` | *(per-hook, see below)* | Agent lifecycle hooks for one tab. Payloads, modes, nonce rejection and responses live in API-TABS |
| POST | `/api/tabs/{id}/agent/switch-provider` | `{provider, model?, reasoning_effort?, force?}` | Daemon-owned in-place handoff (also General/self); 202 + operation id; see PROVIDER-SWITCH |
| GET | `/api/tabs/{id}/agent/switch-provider/{operation}` | `?wait=true` optional | Read pending/running/completed/failed operation and outcome. |
| GET | `/api/tabs/{id}/agent/provider-history` | `?file=PATH` | Read this tab's archived previous provider transcript. |
| POST | `/api/tabs/{id}/agent/restart` | `{force?: bool, grace_secs?: u64, provider?: string, handoff_history?: bool}` | Restart/resume the owned agent session; guarded fresh launch or optional PTY provider handoff. Full contract: API-AGENTS. |
| POST | `/api/tabs/{id}/agent/model` | `{model: string, reasoning_effort?: string, runtime_mode?: string}` | Capability-selected model control: `next_turn` stages without restart, Live is tab-bound, otherwise idle RestartResume. Preserves provider defaults. Details: API-AGENTS. |
| GET | `/api/tabs/{id}/agent/service-tiers` | none | Reads the exact tab's session-bound model tier catalogue and requested/confirmed service tier selections. Returns typed `no_runtime`, `no_tiers`, or `not_found` refusals. |

#### Agent Hierarchy
| Method | Path | Body / Query | Description |
|--------|------|------|-------------|
| POST | `/api/tabs/{id}/role` | `{role, parent_tab_id?, can_spawn?, plan_id?}` | Set tab role (`Orchestrator` or `Worker`; `Conductor` aliases `Orchestrator`). A `Manager` with `role_profile: "planner"` requires `plan_id`; an omitted `parent_tab_id` resolves to the plan's live Orchestrator owner instead of refusing, and an explicit one wins. |
| DELETE | `/api/tabs/{id}/role` | — | Clear role: detach a Worker, or retire an Orchestrator while keeping the final anchor's Team and active Plan for a replacement |
| GET | `/api/tabs/{id}/status` | — | Tab status (`working` \| `question` \| `idle` \| `error` \| `exited`, the value `tab list` shows) + role + children info, including `pending_prompt` when a numbered provider prompt is active (`question` and `options`). `domios tab status --json` returns the same prompt data. |
| POST | `/api/tabs/{id}/report` | `{message, caller_tab_id?, caller_session_id?, session_binding_nonce?, plan_id?, task_id?, status?, ...}` | Send an authenticated hierarchy report. A Plan-authorized task-less reporter uses `plan_id` without `task_id`/`status`; success returns `plan_report` and persists a task-less Plan stride before Orchestrator delivery. |
| GET | `/api/tabs/{id}/reports` | `?clear=true` | Read reports from children (optionally clearing them) |
| POST | `/api/tabs/{id}/can-spawn` | `{can_spawn: bool}` | Toggle whether tab can spawn children |
| POST | `/api/tabs/{id}/agent-type` | `{agent_type}` | Mark a tab as running a specific agent provider |
| POST | `/api/tabs/{id}/prompt/respond` | `{value}` | Respond to the active numbered prompt. `domios tab respond <option> --tab <id>` uses the one-based option shown by status and refuses when the prompt is no longer on screen. |
| POST | `/api/tabs/{id}/prompt/clear` | — | Clear a pending permission prompt |
| GET | `/api/agents/providers` | — | Versioned provider catalog (changes emit catalogue events to selectors): `id`, `display_name`, models, reasoning efforts/defaults (per-model values override the provider list, API-AGENTS), settings, and `kind`: `agent` (PTY) or `shell` (chat backend, also exposing presets and Chat UI support). `availability` is `available`, `unavailable`, or `unknown`; PTY lookup uses `binary_name`; backends use optional `cli_binary`. Spawn refuses `unavailable` before creating a tab; `unknown` warns. Debug builds add `stub_replay`, true when a launch of that provider is replaced by the `CODETERM_STUB_AGENT_CMD` replayer; release builds omit it. |
| POST | `/api/agents/providers/refresh` | `{provider_id}` | Explicitly refreshes a provider's model catalogue before a listing: a catalogue that requires live discovery re-probes; any other catalogue gets the reader-gated re-probe (TTL window, or a changed credential/config/provider binary). Returns `{provider_id, discovery_required, refreshed, reason}`; `reason` names why a probe published nothing and is null after a refresh. |
| GET | `/api/providers` | — | Authenticated provider list alias for selectors |
| POST | `/api/agents/spawn` | *(per route: API-SPAWN)* | One-call agent spawn; roled Team members use `POST /api/teams/{team_id}/members` (API-TEAMS). |
| POST | `/api/agents/helper` | `{title, task}` | Shared spawn for UI AI helper buttons (Configure with AI, Mesh Set up with AI). The daemon resolves the General Agent provider, places the tab in the General workspace and working directory regardless of the active workspace, and starts a roleless visible launch that always carries a first turn. Other fields are refused. A missing General workspace, launch failure or undelivered first turn errors and rolls the tab back. `delivery_state` is `confirmed` only when the first turn was submitted through the PTY with submit evidence, `launched` when the provider consumed it inline at launch without that evidence. |
| GET | `/api/remote-workers` | `?tab=<id>` | Orchestrator view of cross-mesh workers, built from inbound mesh reports. `?tab=` returns that orchestrator's workers; otherwise the full `{ tab_id: RemoteWorker[] }` map. `RemoteWorker = {peer, remote_tab, title, model, status, last_message, last_seen_ms}`. Live updates via the `remote-workers-changed` event. |

Configure-with-AI, provider install, auto-confirm, Pebble webhook and plugin receivers: `domios docs API-INTEGRATIONS`.

Plugin surface routes (`/api/surfaces/*`): `domios docs SURFACES`.

#### Batch
| Method | Path | Body | Description |
|--------|------|------|-------------|
| POST | `/api/batch` | `{commands: [{method, params, id?}]}` | Execute multiple commands sequentially (see below) |

#### Pasteboard (cross-mesh ephemeral payloads)
| Method | Path | Body | Description |
|--------|------|------|-------------|
| POST | `/api/pasteboard` | `{body, content_type?, ttl_ms?, pop_on_read?}` | Stash a payload. Returns `{id: "paste:NNNNNN", expires_at}`. TTL default 1h, max 24h. 20-entry cap, oldest evicted first. Memory-only: a restart wipes every paste. |
| GET | `/api/pasteboard/{id}` | — | Fetch body. `?pop=true` deletes after read. Returns 404 if expired/unknown. |
| DELETE | `/api/pasteboard/{id}` | — | Explicit delete. |
| GET | `/api/pasteboard` | — | List local pastes (id, size, ttl_remaining_ms, content_type). |

#### Supporter visibility
| Method | Path | Query | Description |
|--------|------|-------|-------------|
| GET | `/api/supporter/activity` | `?limit=N&team_id=...` | Latest redacted decisions, default 50 and maximum 100; optional persisted Team attribution filter. Contract: `domios docs API-MEMORY`. |

#### Plan task mutation authority

Task-mutating calls (socket `plan.task.*`, REST `/api/plans/{id}/tasks/{tid}*`) carry caller/session/binding/owner-epoch authority (REST: matching `X-CodeTerm-*` headers). Once owned, the caller must match the recorded owner. `GET /api/tasks` and socket `task.list` return the same object: `tasks` plus an optional `empty_reason`. Its `kind` is `plan_not_found`, `plan_archived`, `plan_outside_project`, `no_plans_in_scope`, `no_tasks_in_scope`, `all_tasks_inactive`, or `no_match_for_filters`. The field is absent when tasks exist; consumers ignore unknown kinds and never invent a local explanation.

Batch writes (socket `plan.batch`, REST `POST /api/plans/{id}/batch`, both reached by `domios plan record`) cross the same boundary: an op naming a task another tab holds is refused unless the caller is that holder or the plan owner. Callers name themselves with `caller_tab_id`; an unidentified caller cannot claim a hold. The refusal carries `STALE TASK BINDING` (socket `-32003`, REST `403`) with `type: "stale_task_binding"` plus `attempted_by_tab`, `plan_id` and `task_id`, and nothing is written. A bearer token proves a client is permitted, never that it holds the task.

A live authenticated Plan owner may recover `update`, `assign`, `respawn`, or `close` after its tab disappears; retirement and recovery commit atomically. Mutating dispatch into canonical or another task's lane is refused with `-32013`; competition provisions one lane per competitor. Advisory findings ride successful results in `safety_nudges` and `_hints`. See `ORCHESTRATION`.

Related: `API-PLAN`, `API-TABS`, `API-CHAT`, `API-INTEGRATIONS`, `API-MEMORY`, `API-TRANSPORT`, `API-CLI`, `TELEGRAM-SETUP`. Analytics (`/api/analytics/{status,consent,event}`), account and plan routes: `ACCOUNT`.

<!-- revision: 229 -->
