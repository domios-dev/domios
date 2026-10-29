# Domios API Reference
Transports: authenticated REST, local socket JSON-RPC and `domios`.

### One behavior across transports

REST, socket and CLI share authorization, PTY cleanup, events and hierarchy checks.

The CLI socket needs no TCP. Unix peer credentials or Windows process-token SID restrict it to the daemon's OS user; each control request also authenticates its credential.

Socket/mesh envelopes require `protocol_version`; older or versionless clients get `-32002`. Wire: `API-TRANSPORT`; peer notice routes: `MESH`.

`domios workspace list`: ordered topology; `tab inspect <id>`: one tab. `workspace current --json` -> `{id,source}` via explicit placement, caller tab, validated workspace env, then focus. No match:exit1; JSON errors:stdout.

POST `/api/blueprints/{id}/webhook` uses a per-graph secret; POST `/api/blueprints/{id}/webhook/configure` manages its lifecycle for a local authenticated caller. Signing, limits and payload rules: `domios docs BLUEPRINTS` and the integrations reference below.

## REST API
Base: `http://<host>:7685/api`.

### Authentication
Host credential: owner-only `.secrets` (`domios.instance:api_token`), not injected into tab env. Tab credentials revoke on close/relaunch/session replacement/restart; identity must match. `system api-token` stays host-only. `domios web link` and GET `/api/network/web-token` allow host/owner or authenticated local General only. Other agents, Neighbor-derived tabs, peers, service pushes and missing auth refuse. Links print only to caller; no logs, telemetry, notifications or shared memory. Export refuses `--mesh`. Loopback needs auth; public health/pairing grants none.

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

Readback `api_listener.active`, `.configured`, `.restart_required` expose actual/configured enabled/address/CORS/origins/auth state; writes never imply rebind. Loopback hosts canonicalize to valid socket/callback URLs; recovery uses the generation contract below. Empty CORS lists deny cross-origin access, not the listener; `*`/malformed origins refuse.

### Endpoints

Route inventories: API-PLAN (plans/tasks), API-TEAMS (authority), API-LIFECYCLE (host/hooks/account/per-friend Neighbor receiving), API-EVENTS (Blueprints/notifications), API-SERVICES (Mesh/plugins).

#### CI failure events

POST `/api/ci/webhook`: `API-EVENTS`.

#### Startup readiness

Health `status: "ok"` proves liveness. `GET /api/startup` and `/api/startup/events` (`startup-state`) expose desktop readiness and per-subsystem states. The desktop becomes usable when `desktop_ready` is true; session backends and optional areas continue restoring. Full service readiness is `startup.state: "ready"`. Pending area requests return HTTP 503 with `error.code: "starting"` and `error.subsystem`; clients keep that area loading until its readiness event. Mutations are never queued or replayed. Only loopback health/startup observations bypass auth. CLI: `API-CLI`.

#### Account machines

Local account routes refuse Mesh. POST `/api/account/mesh/session-revoke {installation_id?:string|null}` defaults to this machine. Signs out selected installation sessions; returns `{ok:true,result:{target_installation_id,revoked_at}}` or typed refusal. Installation key/pairings remain; UI confirms first.

#### Neighbor read receipts

`POST /api/account/neighbors/read`: local owner UI only, exactly `{friendship_id, through_message_id}` (64/32 lowercase hex characters). Reports inbound visibility in the open conversation, never history fetching. Tab/Mesh callers refuse; host reports concierge pickup separately. Read-up-to reservations persist before relay dispatch, suppress duplicate/older batches, and stop for paused/ended friends or Neighbors off.

Outgoing Neighbor history entries expose optional `delivered_at` and `read_at` as UNIX milliseconds. `read_at` implies delivery. Accepted sending is one tick, delivered is two ticks, read is two blue ticks. `neighbor-history-changed` carries the affected `friendship_id` after receipt persistence. See `NEIGHBORS` for the signal and catch-up behavior.

#### Plugin media and Neighbor memory

| Method | Path | Body | Description |
|--------|------|------|-------------|
| GET | `/api/plugins/{plugin}/media/{frame}/{handle}` | — | Passive plugin image/audio/video delivery; the opaque live frame/handle pair authorizes access. Also accepts HEAD. Supports one byte Range (206; invalid ranges 416), `no-store` and `nosniff`; unknown, expired or revoked authority returns 404. No filesystem paths. |
| POST | `/api/mem/neighbor-save` | `{kind,area,title,body,summary?,tags?,importance?}` | Authenticated Neighbor-derived tab only; requires a live matching friendship and a memory write grant for the exact area. Saves only `fact` or `note`, with server-derived authorship and global scope; returns `{id}`. Credentials and oversized content refuse. HTTP 403 returns `{code,reason,message}` for denied authority, area, kind, credentials or size. Local owner and ordinary tabs cannot use this route. |

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
| POST | `/api/automation/blueprints/import` | `{caller_tab_id, recipe, assignments, scope?}` | Local host apply by name, stopped; scope is general, workspace:<id>, or team:<id> and must match the caller; recipe is YAML text or the matching document object; see BLUEPRINTS. |
| GET | `/api/automation/blueprints/project` | Query `workspace_id`, optional `path` | Local host read-only repository listing or bounded source view. No execution or approval. |
| GET | `/api/automation/blueprints/viewer/history` | Query `workspace_id`, explicit `team_id` (empty for no Team), `id` | Retained definition versions with actual root/callee semantic diffs. |
| GET | `/api/identify` | — | Version, platform, features |
| POST | `/api/cli/rpc` | `{method, params?, protocol_version}` | Internal CLI fallback for authenticated loopback callers only. It dispatches the same RPC methods as the local socket; mesh peer tokens are refused. See API-CLI. |
| GET | `/api/network` | — | Local IP, Tailscale IP, API URLs |
| GET | `/api/network/web-token` | — | Token-bearing web UI link; host/owner or authenticated local General only. |
| GET | `/api/keepawake` | — | Get keep-awake status (see below) |
| POST | `/api/keepawake/start` | `{duration_secs?, display?}` | Start / restart keep-awake session |
| POST | `/api/keepawake/stop` | — | Stop keep-awake session |
| POST | `/api/graph/events/{subscription_id}` | `{plugin_id, event_id, payload}` | Plugin event ingress. Uses the subscription's Bearer token, accepts at most 64 KiB, and returns 202 for admission; refusal codes map to 401, 403, 404, 409, 413, 422, 429 or 503. |

REST listener lifecycle and recovery: MESH, *Local REST availability*. REST chat/transcript routes parse a bounded 4 MiB tail.

Saved solution show: redacted `document`, recursive `expanded_calls:[{node,pin:{id,hash},graph,children}]`, saved scope/exact hashes. Missing/corrupt/foreign pins: `pinned_revision_unavailable`; limits8levels/64calls/256KiB: `budget_exhausted`. Archived/foreign roots and peer requests refuse.

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
| POST | `/api/tabs/{id}/agent/switch-provider` | `{provider, model?, reasoning_effort?, force?}` | Daemon-owned in-place handoff (also General/self); 202 + operation id; see PROVIDER-SWITCH |
| GET | `/api/tabs/{id}/agent/switch-provider/{operation}` | `?wait=true` optional | Read pending/running/completed/failed operation and outcome. |
| GET | `/api/tabs/{id}/agent/provider-history` | `?file=PATH` | Read this tab's archived previous provider transcript. |
| POST | `/api/tabs/{id}/agent/restart` | `{force?: bool, grace_secs?: u64, provider?: string, handoff_history?: bool}` | Restart/resume the owned agent session; guarded fresh launch or optional PTY provider handoff. Full contract: API-AGENTS. |
| POST | `/api/tabs/{id}/agent/model` | `{model: string, reasoning_effort?: string, runtime_mode?: string}` | Capability-selected model control: `next_turn` stages without restart, Live is tab-bound, otherwise idle RestartResume. Preserves provider defaults. Details: API-AGENTS. |
| GET | `/api/tabs/{id}/agent/service-tiers` | none | Reads the exact tab's session-bound model tier catalogue and requested/confirmed service tier selections. Returns typed `no_runtime`, `no_tiers`, or `not_found` refusals. |

#### Agent Hierarchy
| Method | Path | Body / Query | Description |
|--------|------|------|-------------|
| POST | `/api/tabs/{id}/role` | `{role, parent_tab_id?, can_spawn?, plan_id?}` | Orchestrator/Worker (`Conductor` aliases Orchestrator). Manager with `role_profile:"planner"` requires `plan_id`; omitted parent resolves to its live Orchestrator owner, explicit parent wins. |
| DELETE | `/api/tabs/{id}/role` | — | Clear role: detach a Worker, or retire an Orchestrator while keeping the final anchor's Team and active Plan for a replacement |
| GET | `/api/tabs/{id}/status` | — | `working`/`question`/`idle`/`error`/`exited` (as in tab list), role, children; active numbered prompt adds `pending_prompt:{question,options}`, also returned by `domios tab status --json`. |
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

Configure-with-AI, provider install, auto-confirm, Pebble and Blueprint webhooks, plugin receivers: `domios docs API-INTEGRATIONS`.

Plugin surface routes (`/api/surfaces/*`): `domios docs SURFACES`.

#### Batch
| Method | Path | Body | Description |
|--------|------|------|-------------|
| POST | `/api/batch` | `{commands: [{method, params, id?}]}` | Host credential only; execute multiple commands sequentially (see below) |

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

Task writes (socket `plan.task.*`, REST `/api/plans/{id}/tasks/{tid}*`) require caller/session/binding/owner-epoch authority, matching `X-CodeTerm-*` REST headers and any recorded owner. `GET /api/tasks`/socket `task.list` return `tasks` and optional `empty_reason.kind`: `plan_not_found`, `plan_archived`, `plan_outside_project`, `no_plans_in_scope`, `no_tasks_in_scope`, `all_tasks_inactive`, `no_match_for_filters`. Tasks present: field absent. Ignore unknown kinds; never invent explanations.

Batch writes (`plan.batch`, `POST /api/plans/{id}/batch`, via `domios plan record`) touching another tab's held task require that holder or plan owner. Credentials establish tab caller; `caller_tab_id` checks consistency, never selects human authority/claims a hold. Refusal writes nothing: `STALE TASK BINDING` (socket `-32003`, REST `403`), `type:"stale_task_binding"`, `attempted_by_tab`, `plan_id`, `task_id`. Bearer permission is not ownership.

Live authenticated Plan owners may recover `update`/`assign`/`respawn`/`close` after tab disappearance; retirement/recovery is atomic. Mutating dispatch into canonical/another task's lane refuses `-32013`; competition gets one lane/competitor. Successful results carry advisory `safety_nudges`/`_hints`. See `ORCHESTRATION`.

Related: `API-PLAN`, `API-TABS`, `API-CHAT`, `API-MEMORY`, `API-TRANSPORT`, `API-CLI`, `TELEGRAM-SETUP`. Analytics (`/api/analytics/{status,consent,event}`), account and plan routes: `ACCOUNT`.

<!-- revision: 243 -->
