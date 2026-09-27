# Agent and orchestration API
REST endpoints for agent lifecycle, plans, teams, updates, batches, and orchestration state.

#### Agent Restart (POST /api/tabs/{id}/agent/restart)

Restart an agent whose process is observed live in a tab. The body is optional (defaults below). Default is graceful: send the provider-specific exit command and wait up to `grace_secs` for the shell-ready event, then tear down the old PTY, create a replacement, and relaunch, resuming the recorded provider session if it still exists on disk, else starting fresh. A provider observed to have exited into its surviving shell is recovered in place: CodeTerm skips the exit and teardown legs, takes the tab's newest session from its binding lineage (or, without lineage, its lifecycle journal), requires the journal to prove that session was nonce-verified and exactly one provider to own its session record, then relaunches that provider with resume through the ordinary launch pipeline in the same PTY.

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
  "restart_accepted": true, "old_pty_torn_down": true, "agent_ready": true, "recovered_in_place": false,
  "old_pty_id": "f0c92c76-1cce-4a65-b5e4-a68c69ae1e2e", "new_pty_id": "b264031d-5a5a-461b-bba6-189405223f80",
  "tab_id": "a3f1b2c7", "provider": "claude", "session_id": "0193e1a4-7b2c-7d4e-9f80-1234567890ab",
  "session_was_stale": false, "session_action": "resume", "full_access": true, "force": false
}
```

The three transition fields are independent observations. `restart_accepted` means validation and rate limiting accepted the request. `old_pty_torn_down` means CodeTerm observed the prior PTY leave the tab and its process tree complete. `agent_ready` means an `AgentReady` transition was observed for `new_pty_id`, which must differ from `old_pty_id`; a pre-restart `AgentReady` attached to the old PTY never satisfies this contract. `recovered_in_place` marks an exited-provider recovery: nothing is torn down, `new_pty_id` equals `old_pty_id`, and the shell's prior stage is retired before launch, so only the relaunch can report `AgentReady`. `session_was_stale` is `true` when the recorded session JSONL no longer exists on disk; the restart proceeds without `--resume` in a fresh session.

**Errors:**
- `404` — tab id not found
- `400` — the agent process is unobserved, the provider is unsupported, a chat-backend session is closed, or exit recovery is refused: `exit_recovery_no_live_shell`, `exit_recovery_no_verified_session` (no agent and no nonce-verified prior session), or `exit_recovery_session_not_owned` (no single provider owns that session record)
- `429` — rate-limited (returns `Retry-After` header + `{"retry_after_secs": N}` body); restart is throttled per-tab to avoid restart loops
- `500` — teardown or relaunch failed
- `504` — the replacement PTY did not reach `AgentReady` inside the bounded readiness deadline

Failures after acceptance include `error` plus the same transition fields. Every boolean stays false unless that leg was observed; `new_pty_id` appears only if a replacement PTY was linked. Rejections (`400`, `404`, `429`) precede acceptance and keep compact error bodies. A failed restart is never returned in the ready `202` shape.

CLI: `codeterm agent restart --tab TAB_ID [--force] [--grace-secs N]`.

### Plan Task Attempts

Task execution attempts are append-only history on `Task.execution_attempts`. The CLI/API uses the stable `TaskExecutionAttempt.id` as `attempt_id`; `executor_tab_id` is the executor identity, not the primary key. `requested_model` and `provider_resolved_model` preserve the selected and provider-resolved ids; `session_model` records the model detected from that attempt's bound session and stays null until observed. `model_identity_mismatch` records a difference and its cause. Respawn creates a new Worker tab and live attempt, never reactivating the selected historical attempt.

### Team-bound plan creation

An orchestrator's first plan creation is atomic: its response includes `plan.team_id`, top-level `team_id`, and `team_minted`. When `team_minted` is true, `_hints` includes `codeterm team rename <team_id> <name>` and `codeterm docs ORCHESTRATION`. The `mux-state-changed` snapshot includes the Team registry in `teams` and each tab's `team_id`; committed plan mutations also emit `plan-changed`. A Team with an active plan refuses a second plan with the unchanged `team_already_has_active_plan` error.

### Team-bound plan activation and transfer

Every active plan must reference exactly one live Team, and a live Team may have only one active plan. `POST /api/plans/{id}/unarchive` and `POST /api/plans/{id}/resume` reject a missing or stale Team binding; unarchive also rejects a Team that already owns another active plan. `team.adopt_plan` activates an archived/teamless plan atomically, replaces its retired owner with the target Team's sole live Orchestrator, records owner history, clears the stale planner binding, and relinks live task hierarchy to that Orchestrator. A Team with zero or multiple live Orchestrators cannot adopt implicitly; use an explicit transfer destination when applicable.

`POST /api/plans/{id}/transfer` accepts `{to: OrchestratorRef, reason?}` and requires `X-CodeTerm-Tab-Id` to identify the source Team's Orchestrator or planner-profile Manager. Missing callers, Workers, and foreign-Team tabs receive `403` without mutation. The destination must be a live Orchestrator belonging to a live Team. Owner, owner history, Team binding, and stale planner retirement are one storage mutation, followed by hierarchy relinking under the Team lock.

### Team staffing and Orchestrator recovery (POST /api/teams/{team_id}/members)

`{provider, role, title?, task?, model?}` adds one member to an existing Team; `role` is `worker`, `planner`, `orchestrator`, or `cartographer`. This endpoint, the UI add-member action, the socket `team.add_member` verb, and `codeterm agent spawn <provider> --team <id> --role Orchestrator` are one service path. `codeterm team attach-cartographer <team> --provider <provider> [--model <model>]` uses that same path.

A Team may have one optional Cartographer Manager. The first attachment spawns a tab with the built-in `cartographer` role kind; repeated attachments return the same `tab_id` with `reused: true` and do not create another tab. It is a non-anchoring Manager without Team-management, plan, or spawn authority. Team close removes the Cartographer tab and its Team binding with every other member; the default Orchestrator/Planner pair does not include one unless explicitly attached.

A Team holds **at most one live Orchestrator**, enforced as a headcount of live anchoring tabs, not a role ban: a removed tab is gone from the multiplexer and a dead agent's identity is cleared on lifecycle exit, so neither counts. Unlinking the final Orchestrator atomically retires its owner reference while keeping the discoverable Team and active Plan id/history, so the Team accepts the documented replacement command and the new anchor takes that Plan over through the tab-gone owner rebind. Descendant role bindings follow the ordinary unlink rules. The response carries `plan_takeover: {plan_id, owner_tab_id}` when ownership moved. A takeover failure fails staffing, removes the spawned replacement and lane, and leaves the retained Team and Plan owner/history unchanged. Because `plan.owner` moves, the plan heartbeat targets the new tab from the next scan on.

`X-CodeTerm-Tab-Id` is optional on this endpoint alone: a Team whose Orchestrator is gone has no membership left to spend. A caller-less add can place only an anchoring role; `worker`, `planner`, and `cartographer` still need a spawn-capable member. This bootstrap action only creates a member and cannot delete Team state; destructive Team and workspace closes without a caller tab require explicit target confirmation. Every structural refusal leads with an error-code constant from `codeterm-types::team::staffing` and is logged with Team, requested role, and cause: `team_orchestrator_already_live`, `team_member_role_unknown`, `team_role_requires_spawner`, `team_caller_unauthorized`, `team_caller_cannot_spawn`, `team_workspace_unresolved`. A structural refusal is never surfaced as a retry prompt.

### Close Team (POST /api/teams/{team_id}/close)

Closes every Team member through the Team cascade path, removes the Team registry entry, archives and detaches every bound plan, retires each plan owner into owner history, clears its owner/planner routing authority, finalizes open attempts and lanes, deletes Team crons, and emits one committed `mux-state-changed`. This is the only user operation that removes a Team. Ordinary tab closes keep the Team record; the one automatic exception is a pair-minted startup lease, whose forming, empty, fully owned Team is reaped once its two reserved launch attempts are gone. A present `X-CodeTerm-Tab-Id` header must identify the Team's Orchestrator or planner-profile Manager. With no caller tab header, a confirmed human sends `{force: true, confirm: "<team_id>"}` after the close dialog; the service logs this as `human-attributed (headerless, confirmed)`. Missing or mismatched confirmation is refused as Unattributed and logged. Unauthorized callers return `403`, and an unknown or already closed Team returns structured `404`.

| Method | Path | Body / Query | Description |
|--------|------|--------------|-------------|
| GET | `/api/plans/{id}/tasks/{tid}/attempts` | — | List execution attempts recorded for one task |
| GET | `/api/plans/{id}/tasks/{tid}/attempts/{attempt_id}` | — | Show one attempt by stable attempt id, including derived report command, derived prompt, and last report summary when present |
| POST | `/api/plans/{id}/tasks/{tid}/respawn` | `{attempt_id, provider?, model?, parent_tab_id?, workspace_id?, title?}` | Build a Worker prompt from the task body, selected prior attempt, last report/status, lane branch/worktree hints, and exact `codeterm agent report --plan ... --task ...` command; spawn a new Worker and append a new `execution_attempts` entry |

Task dispatch requires explicit `execution_intent` (`read_only` or `mutating`). Mutating dispatch requires known provider-allowance telemetry at or above `provider_reserve_pct` (default 5); a stale snapshot emits a warning but uses its last known reserve value instead of deadlocking dispatch. This preflight runs before lane provisioning or task mutation. An interrupted mutating attempt projects `handoff_required` with a digest-verified Git checkpoint until a fresh owner adopts the next owner epoch.

`POST /api/plans/{id}/tasks/{tid}/adopt-checkpoint` adopts the sealed checkpoint for the nonce-authenticated Worker named by the CodeTerm identity headers. The equivalent CLI command is `codeterm plan task adopt-checkpoint <plan> <task>`. Successful adoption increments `owner_epoch`, creates a fresh `awaiting_worker_ack` attempt, and fences prior owner sessions.

### Keep-Awake (GET/POST /api/keepawake*)

Prevents machine sleep (optionally display dimming) while long-running agents work. Global machine state, not tab- or workspace-scoped; all three endpoints require the Bearer token.

**Status shape (`KeepAwakeStatus`):**
```json
{"active": true, "display": false, "expires_at": 1744387200000}
```
- `active` — whether a keep-awake session is running.
- `display` — whether the display is also kept on.
- `expires_at` — unix milliseconds when the session auto-expires, or `null` for an indefinite session.

**GET /api/keepawake** — returns the current `KeepAwakeStatus`. When no session is active, `active=false`, `display=false`, `expires_at=null`.

**POST /api/keepawake/start** — body and all its fields are optional:
```json
{"duration_secs": 3600, "display": false}
```
- `duration_secs` — session length in seconds. Omit or `null` for an indefinite session.
- `display` — `true` to also prevent display sleep/dimming. Default `false`.

Any existing session is stopped first, then the new one is started. Returns the fresh `KeepAwakeStatus`.

**POST /api/keepawake/stop** — no body. Stops any active session. Returns `{"ok": true}`.

**Tauri event `keepawake-changed`** — emitted on every transition: start, stop, natural expiry (`duration_secs` elapsed), and macOS `caffeinate` spawn failure. Payload is `KeepAwakeStatus`. Frontends subscribe with `listen("keepawake-changed", (ev) => ...)`.

**Persistence:** state is written to `~/.codeterm/keepawake.json` (dev: `~/.codeterm-dev/keepawake.json`) and restored on startup: indefinite sessions resume as-is, timed ones with the remaining duration, expired ones are discarded.

**Platform support:**
- **macOS** — spawns `caffeinate -i -s` (plus `-d` when `display=true`).
- **Windows** — calls `SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED)`, OR-ing in `ES_DISPLAY_REQUIRED` when `display=true`.
- **Linux** — no-op; logs a warning and still returns a status object.

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

One request creates a **new tab** in the parent's workspace, assigns a role, launches an agent (claude/aider/any), and optionally sends a task. The response returns immediately; PTY readiness detection and agent launch run in a background thread.

The hierarchy (Orchestrator -> Worker) is a logical chain, not nested layouts; each agent gets its own tab. Spawns never switch the user's active tab.

**Request body:**
```json
{
  "agent": "claude",           // agent command (default: "claude")
  "agent_name": "architect",   // maps to --agent flag (.claude/agents/ file)
  "title": "Worker-1",         // tab title (default: agent_name or agent command)
  "description": "Reviews API contracts", // tab activity summary; title is one word
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

The selected model's advertised reasoning levels constrain `reasoning_effort`; the provider's flat list applies when the model has none (`codeterm agent models <provider>` shows the sets).

Provider listings may include `default_reasoning_effort` on the provider descriptor (fallback) and on each `available_models` entry (that model's live default). `codeterm agent models <provider>` marks the model-specific default beside its reasoning levels, falling back to the provider value only when the model has none.

All fields are optional; `agent` uses the configured provider unless a unique model id selects another provider. Titles are one word; activity summaries go in `description`. Permissions come only from the global `agent.full_access` setting, which callers cannot override; before the user's first choice, spawn is rejected before tab creation with an instruction to choose access in CodeTerm. `audience` controls user notifications only. With an authoritative model catalogue, an unknown `model` is rejected before tab creation (`codeterm agent models <provider>` refreshes and lists valid ids); free-form providers validate against a published live model list and pass ids through only when no live list exists. An id advertised by several providers requires an explicit provider, and provider aliases are never passed across providers. Reasoning effort is capability-validated before launch, and response provenance separates host-owned `planned` plan-task intent from client `requested`, provider `resolved`, and exact-session `actual`; ordinary spawn payloads cannot set `planned`. `task` goes through the provider's declared launch delivery; `agent_name` maps to the provider's named-agent launch option when supported. The CLI flag `codeterm agent spawn --non-user-facing` sends `audience=automation`. A `Worker` with `execution_intent: "read_only"` needs no task binding: this canonical verification Worker owns no task, so it reports against and transitions none, and `authorized_gates` is its whole command scope, rendered into its brief. `execution_intent: "mutating"` without `plan_task`/`task_id` is refused, and `authorized_gates` is rejected alongside `plan_task` (a task-bound Worker declares gates on `plan.task.dispatch`, where they are enforced). A task-bound spawn with `execution_intent: "mutating"` is also refused before tab creation unless its effective `cwd` verifies as an isolated lane this task may write in: no checkout, a directory that is not a worktree on a branch, the plan's canonical checkout, or another task's recorded lane all fail closed, and the refusal names `codeterm plan task dispatch`, which provisions the lane. Read-only verification still runs in the canonical checkout. A `Worker` may also carry a `role_profile` naming a leaf kind, such as the builtin `cartographer`, which layers that kind's contract onto the Worker base; a managing kind id (`planner`, `watcher`, `orchestrator`, `general_agent`) is refused rather than downgraded. `codeterm plan task dispatch` sets it from the task's `executor_role` when that role names a builtin leaf.

An unknown id on an authoritative catalogue or a published free-form live list
is refused before spawn or task dispatch creates a tab. REST spawn returns HTTP
422 with `error.code` set to `invalid_model`, including `provider`,
`requested_model`, `closest_model_ids`, and `refresh_command`. A bare id owned by
multiple providers returns `ambiguous_model` with `ambiguous_providers`; choose
the provider explicitly. Neither refusal has a transient retry hint. CLI
`--json` preserves these fields under the error envelope's `data` for spawn,
dispatch, and team pair.

**Response** (returned immediately, before agent is fully started):
```json
{"tab_id": "a3f1b2c7", "tab_id": "d5e6f7a8", "pty_id": "c9d0e1f2", "agent": "claude", "title": "Worker-1", "role": "Worker"}
```

### Remote mesh workers

A worker spawned on another peer with `codeterm agent spawn <agent> --mesh PEER [--report-to <tab|main|general>]` gets a **durable cross-mesh report target**: the CLI relays the spawn to `PEER`, whose relay ingress stamps an authenticated `origin_peer` onto the body, and the spawned Tab persists `report_target_mesh_peer = <caller's peer>` + `report_target_tab_id = <report_to>` (defaults to the caller's Tab).

The worker just runs `codeterm agent report "..."` (no `--mesh`/`--tab`). Its daemon resolves the remote target and relays the report over the same signed `/send` path (with `kind: "report"`), so it surfaces on the origin peer as `[Report from <worker>@<peer>]`, identical to a local report. If no target resolves, `agent report` fails with an actionable hint, never a silent drop.

Report routing precedence: **remote mesh target → `parent_tab_id` → `report_target_tab_id` → `spawned_by_tab_id`**; never self.

### Plan-authorized task-less evidence reports
The live authenticated Plan owner grants a review or canonical-QA tab with `plan.review.authorize-reporter` / `codeterm plan review authorize-reporter`. The durable grant records the target's exact tab id, session id, and a one-way binding-nonce digest; it never exposes the bearer nonce through Plan history. The target reports with `plan_id` only: `task_id`, `status`, and task provenance remain absent. Success appends an immutable `plan_evidence_report` Plan stride, publishes the ordinary `agent-report` event to the live owner, and returns `plan_report` with the stride id and `task_transition:false`.
Authorization is capability-based; role labels grant nothing. An absent caller session is resolved from the exact registered tab only with a matching stored and hook-reported nonce; an explicitly wrong session still refuses. Foreign, unbound, stale/rebound, task-bound, or ungranted callers change no Plan history or report store. A session change requires a fresh owner grant. Registered Planner and task-bound Worker paths are unchanged.

Each inbound mesh report registers the worker in the orchestrator-side registry (`GET /api/remote-workers`, `remote-workers-changed` event); the sidebar renders it as a read-only "remote tab" ghost node under its orchestrator (peer chip + status, no local PTY).

### Batch Commands (POST /api/batch)

Run multiple operations sequentially in one HTTP request. Later commands reference a command's `id` as `$id`.

**Request body:**
```json
{
  "commands": [
    {"method": "spawn", "id": "boss", "params": {"title": "Boss", "description": "Coordinates repository work", "role": "Orchestrator", "agent": "claude", "cwd": "C:/project", "can_spawn": true}},
    {"method": "spawn", "params": {"title": "Worker-1", "description": "Analyzes the code", "role": "Worker", "parent_tab_id": "$boss", "agent": "claude", "task": "analyze code"}},
    {"method": "spawn", "params": {"title": "Worker-2", "description": "Runs focused tests", "role": "Worker", "parent_tab_id": "$boss", "agent": "claude"}},
    {"method": "notify", "params": {"title": "All spawned", "body": "4 agents ready"}}
  ]
}
```

**Available methods:** `spawn`, `send`, `rename`, `close`, `role`, `notify`, `can-spawn`, `report`

**$ref syntax:** any string value starting with `$` is replaced with the `tab_id` from the result of the command with that `id`, e.g. `"parent_tab_id": "$boss"` resolves to the "boss" spawn's tab_id.

**Response:**
```json
{"results": [{"tab_id": "...", "tab_id": "...", ...}, {"ok": true}, ...]}
```

A failing command returns `{"error": "message", "method": "..."}`; later commands still execute.

**Batch method parameters:**

| Method | Required params | Optional params |
|--------|----------------|-----------------|
| `spawn` | — | `agent`, `agent_name`, `title`, `description`, `cwd`, `role`, `parent_tab_id`, `can_spawn`, `task`, `args` |
| `send` | `tab_id`, | `text`, `enter` (default: true) |
| `rename` | `tab_id`, `title` | — |
| `close` | `tab_id` | — |
| `role` | `tab_id`, `role` | `parent_tab_id`, `can_spawn` |
| `notify` | — | `title`, `body`, `level` |
| `can-spawn` | `tab_id` | `can_spawn` (default: true) |
| `report` | `tab_id`, `message` | — |

Plan surface contracts — planner authority over task structure, partial `plan goal set`, lane persistence on dispatch, and recording an external executor: see `codeterm docs API-PLAN`.

<!-- revision: 35 -->
