# Agent and orchestration API

REST endpoints for agent lifecycle, plans, teams, updates, batches, and orchestration state.

#### Agent Restart (POST /api/tabs/{id}/agent/restart)

Restart an agent whose process is currently observed live in a tab. The body is optional; no body uses the defaults below. Default behaviour is graceful: send the provider-specific exit command and wait up to `grace_secs` for the shell-ready event. CodeTerm then tears down the old PTY, creates a replacement PTY, and relaunches. If the recorded provider session still exists on disk, the provider resume command is used; otherwise CodeTerm launches a fresh session. A provider already observed to have exited into its surviving shell is absent, not restartable metadata, and is refused before acceptance.

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
  "full_access": true,
  "force": false
}
```

The three transition fields are independent observations. `restart_accepted` means validation and rate limiting accepted the request. `old_pty_torn_down` means CodeTerm observed the prior PTY leave the tab and its process tree complete. `agent_ready` means an `AgentReady` transition was observed for `new_pty_id`, which must differ from `old_pty_id`. A pre-restart `AgentReady` value attached to the old PTY never satisfies this contract.

`session_was_stale` is `true` when the recorded session id pointed at a JSONL that no longer exists on disk; the restart proceeds without `--resume` and a fresh session is created.

**Errors:**
- `404` — tab id not found
- `400` — the agent process is absent or unobserved, the tab has no agent, or the provider is unsupported
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

### Team staffing and Orchestrator recovery (POST /api/teams/{team_id}/members)

`{provider, role, title?, task?, model?}` adds one member to an existing Team; `role` is `worker`, `planner`, or `orchestrator`. This endpoint, the UI add-member action, the socket `team.add_member` verb, and `codeterm agent spawn <provider> --team <id> --role Orchestrator` are one service path.

A Team holds **at most one live Orchestrator**, enforced as a headcount of live anchoring tabs rather than a ban on the role: a removed tab is gone from the multiplexer and a dead agent's identity is cleared on lifecycle exit, so neither counts. Unlinking the final Orchestrator atomically retires its owner reference while retaining the discoverable Team and active Plan id/history. The Team therefore accepts the documented replacement command, and the new anchor takes that same Plan over through the tab-gone owner rebind. Descendant role bindings follow the ordinary unlink rules. The response carries `plan_takeover: {plan_id, owner_tab_id}` when ownership moved. A takeover failure fails staffing, removes the spawned replacement and lane, and leaves the retained Team and Plan owner/history unchanged. Because `plan.owner` moves, the plan heartbeat targets the new tab from the next scan on.

`X-CodeTerm-Tab-Id` is optional on this endpoint alone: a Team whose Orchestrator is gone has no membership left to spend. A caller-less add can place only an anchoring role; `worker` and `planner` still need a spawn-capable member. Every structural refusal leads with an error-code constant from `codeterm-types::team::staffing` and is logged with the Team, the requested role, and the cause — `team_orchestrator_already_live`, `team_member_role_unknown`, `team_role_requires_spawner`, `team_caller_unauthorized`, `team_caller_cannot_spawn`, `team_workspace_unresolved`. A structural refusal is never surfaced as a retry prompt.

### Close Team (POST /api/teams/{team_id}/close)

Explicitly closes every Team member through the Team cascade path, removes the Team registry entry, archives and detaches every bound plan, retires each plan owner into owner history, clears its owner/planner routing authority, and emits one committed `mux-state-changed`. This is the only user operation that removes a Team. Ordinary tab closes retain the Team record; the single automatic exception is a pair-minted startup lease, whose forming, empty, fully owned Team is reaped once its two reserved launch attempts are gone. The `X-CodeTerm-Tab-Id` header is required and must identify the Team's Orchestrator or planner-profile Manager. Missing header returns `400`, unauthorized or foreign callers return `403`, and an unknown or already closed Team returns structured `404`.

| Method | Path | Body / Query | Description |
|--------|------|--------------|-------------|
| GET | `/api/plans/{id}/tasks/{tid}/attempts` | — | List execution attempts recorded for one task |
| GET | `/api/plans/{id}/tasks/{tid}/attempts/{attempt_id}` | — | Show one attempt by stable attempt id, including derived report command, derived prompt, and last report summary when present |
| POST | `/api/plans/{id}/tasks/{tid}/respawn` | `{attempt_id, provider?, model?, parent_tab_id?, workspace_id?, title?}` | Build a Worker prompt from the task body, selected prior attempt, last report/status, lane branch/worktree hints, and exact `codeterm agent report --plan ... --task ...` command; spawn a new Worker and append a new `execution_attempts` entry |

Task dispatch requires explicit `execution_intent` (`read_only` or `mutating`). Mutating dispatch requires known provider-allowance telemetry at or above `provider_reserve_pct` (default 5); a stale snapshot emits a warning but uses its last known reserve value instead of deadlocking dispatch. This preflight runs before lane provisioning or task mutation. An interrupted mutating attempt projects `handoff_required` with a digest-verified Git checkpoint until a fresh owner adopts the next owner epoch.

`POST /api/plans/{id}/tasks/{tid}/adopt-checkpoint` adopts the sealed checkpoint for the nonce-authenticated Worker named by the CodeTerm identity headers. The equivalent CLI command is `codeterm plan task adopt-checkpoint <plan> <task>`. Successful adoption increments `owner_epoch`, creates a fresh `awaiting_worker_ack` attempt, and fences prior owner sessions.

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

**POST /api/keepawake/start** — request body is optional; when present, all fields are optional:
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
  "workspace_id": "b4e2c8d1", // optional, defaults to active workspace
  "plan_id": "pl_a9zejn",      // plan a Planner binds, or a verification Worker runs under
  "plan_task": "task_8n7m9o",  // task a Worker binds; omitted by a verification Worker
  "execution_intent": "read_only", // read_only | mutating; mutating requires a task binding
  "authorized_gates": ["./build.sh verify"], // exact commands a task-less Worker may claim
  "audience": "automation"     // optional: suppress user bells for bench/probe tabs
}
```

All fields are optional. `agent` defaults to `"claude"`. Permission behavior comes only from the global `agent.full_access` setting; spawn callers cannot override it. If the user has not made the first choice yet, the spawn is rejected before tab creation with an instruction to choose access in CodeTerm. `audience` controls user notifications only. When `model` is provided and the provider advertises models, unknown ids are rejected before tab creation; run `codeterm agent models <provider>` for valid ids. Reasoning effort is capability-validated before launch, and response provenance separates host-owned `planned` plan-task intent from client `requested`, provider `resolved`, and exact-session `actual`; ordinary spawn payloads cannot set `planned`. When `task` is provided, it is passed through the provider's declared launch delivery. When `agent_name` is provided, it maps to the provider's named-agent launch option when supported. The CLI flag `codeterm agent spawn --non-user-facing` sends `audience=automation`. A `Worker` spawn needs no task binding when `execution_intent` is `read_only` — the canonical verification Worker: it owns no task, so it can report against none and transition none, and `authorized_gates` is its whole command scope, rendered into its brief. `execution_intent: "mutating"` without `plan_task`/`task_id` is refused, and `authorized_gates` is rejected alongside `plan_task` (a task-bound Worker declares gates on `plan.task.dispatch`, where they are enforced).

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

## Plan surface contracts

Planner authority over task structure, partial `plan goal set`, lane persistence on dispatch, and recording an external executor: see `codeterm docs API-PLAN`.

<!-- revision: 15 -->
