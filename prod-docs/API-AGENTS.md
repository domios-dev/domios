# Agent and orchestration API
REST endpoints for agent restart, model switch, spawn, reports, keep-awake, and batches.

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

#### Agent Model Switch (POST /api/tabs/{id}/agent/model)

Switch one idle agent tab to another model. Only that tab changes: CodeTerm never types a provider model command, because provider pickers save the choice as the machine-global default. The provider descriptor's `model_switch` says how the switch is done. `restart_resume` relaunches the tab through the restart path above, resuming its own nonce-verified session with the new model launch args and the same effort, arguments and system prompt a daemon restore would use. The tab's restorable launch record is rewritten before the relaunch, so a later restore starts the model that is running. `unsupported` (the default) refuses the switch and writes nothing to the tab.

```json
{"model": "sonnet"}
```

The current binding's launch report or first post-relaunch assistant turn can confirm the model. After the bounded confirmation deadline, a ready relaunch with no model observation still succeeds as pending; the label and `agent_session_model` follow the first turn when it arrives. A relaunch failure or a confirmed different model remains an error.

Confirmed success (`200 OK`) includes `model_confirmation: "observed"`, `observed_model`, and `evidence` (`session_start_report` or `first_post_relaunch_turn`).

When the resumed provider omits its model and no assistant turn arrives before the confirmation deadline, the success response keeps the same relaunch evidence and reports pending confirmation:

```json
{
  "ok": true, "tab_id": "a3f1b2c7", "provider": "claude", "mechanism": "restart_resume",
  "previous_model": "haiku", "requested_model": "sonnet", "launched_model": "sonnet",
  "model_confirmation": "pending", "session_id": "0193e1a4-7b2c-7d4e-9f80-1234567890ab",
  "relaunch_evidence": { "session_id": "0193e1a4-7b2c-7d4e-9f80-1234567890ab", "launched_model": "sonnet", "agent_ready": true },
  "restart": { "restart_accepted": true, "old_pty_torn_down": true, "agent_ready": true, "session_action": "resume" }
}
```

`relaunch_evidence.session_id` is the resumed session id, `launched_model` is the model passed through the provider's `--model` launch argument, and `agent_ready` confirms the relaunch reached readiness. Pending success omits `observed_model` and `evidence`; the tab identity updates when the first post-relaunch turn provides a model.

Every error body is `{"ok": false, "error": "...", "reason": "..."}` plus the fields below:
- `404 not_found` — tab id not found
- `400 invalid_model` — the model is not in the provider's catalogue; `model_error` carries `valid_model_ids` and `refresh_command`
- `409 model_switch_unsupported` — the provider declares no live switch; `provider` names it
- `409 not_switchable` — no agent, the agent is not observed live, it is mid-turn or waiting on a prompt, the tab has no verified resumable session, or the tab is the General Agent (its model comes from Settings)
- `429 rate_limited` — the tab restarted moments ago; `retry_after_secs` and `Retry-After`
- `500`/`504 relaunch_failed` — teardown, relaunch, readiness or rebinding failed; `restart` carries the observed transition fields
- `502 model_mismatch` — the current binding's launch report or first post-relaunch turn names a different model; `launched_model`, `observed_model`, and `restart` are included. Missing model confirmation by its deadline is a pending success, not an error.

CLI: `codeterm agent model --tab TAB_ID MODEL [--json]`; `--json` prints the success body, or the error envelope carrying the error body as `data`.

### Plan task attempts and Teams

Task execution attempts, respawn, dispatch preflight, Team-bound plans, Team staffing, and Team close: `codeterm docs API-TEAMS`.

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

One request creates a **new tab** in the parent's workspace, assigns a role, launches an agent (claude/aider/any), and optionally sends a task. The response returns immediately; PTY readiness detection and agent launch run in a background thread. The hierarchy (Orchestrator -> Worker) is a logical chain, not nested layouts; each agent gets its own tab. Spawns never switch the user's active tab.

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

The selected model's advertised reasoning levels constrain `reasoning_effort`; the provider's flat list applies when the model has none (`codeterm agent models <provider>` shows the sets). Provider listings may include `default_reasoning_effort` on the provider descriptor (fallback) and on each `available_models` entry (that model's live default). `codeterm agent models <provider>` marks the model-specific default beside its reasoning levels, falling back to the provider value only when the model has none.

All fields are optional; `agent` uses the configured provider unless a unique model id selects another provider. `cwd` must be an absolute, existing directory on the receiving host (the CLI resolves a relative `--cwd` against the caller's directory); anything else is refused before tab creation instead of letting the PTY fall back to the home directory. Titles are one word, at most 64 characters, with no whitespace or control characters; activity summaries go in `description` as one line, at most 200 characters, without control characters. Permissions come only from the global `agent.full_access` setting, which callers cannot override; until the user chooses, spawn is rejected before tab creation with an instruction to choose access in CodeTerm. Without full access, a model the provider declares without an unattended permission mode receives launch-scoped grants for the role's CodeTerm commands, and an Orchestrator or Planner on such a model is rejected before tab creation with an error naming the model (`team pair` checks both members first). `audience` controls user notifications only. With an authoritative model catalogue, an unknown `model` is rejected before tab creation (`codeterm agent models <provider>` refreshes and lists valid ids); free-form providers validate against a published live model list and pass ids through only when no live list exists. An id advertised by several providers requires an explicit provider, and provider aliases are never passed across providers. Reasoning effort is capability-validated before launch, and a level the provider lists without a launch flag for it is refused; response provenance separates host-owned `planned` plan-task intent from client `requested`, provider `resolved`, and exact-session `actual`; ordinary spawn payloads cannot set `planned`. `task` goes through the provider's declared launch delivery; `agent_name` maps to the provider's named-agent launch option when supported. The CLI flag `codeterm agent spawn --non-user-facing` sends `audience=automation`. A `Worker` with `execution_intent: "read_only"` needs no task binding: it owns and transitions no task, and `authorized_gates` is its whole command scope, rendered into its brief. `execution_intent: "mutating"` without `plan_task`/`task_id` is refused, and `authorized_gates` is rejected alongside `plan_task` (a task-bound Worker declares gates on `plan.task.dispatch`, where they are enforced). A task-bound spawn with `execution_intent: "mutating"` is also refused before tab creation unless its effective `cwd` verifies as an isolated lane this task may write in: no checkout, a directory that is not a worktree on a branch, the plan's canonical checkout, or another task's recorded lane all fail closed, and the refusal names `codeterm plan task dispatch`, which provisions the lane. A `Worker` may also carry a `role_profile` naming a leaf kind, such as the builtin `cartographer`, which layers that kind's contract onto the Worker base; a managing kind id (`planner`, `watcher`, `orchestrator`, `general_agent`) is refused rather than downgraded. `codeterm plan task dispatch` sets it from the task's `executor_role` when that role names a builtin leaf.

`audience` is `user` (default) or `automation`; an automation or hidden tab's prompts and exits never notify the user and are reported to its parent or report target instead (`codeterm docs NOTIFICATIONS`). `description` is validated like a tab description, and `reasoning_effort` is never silently downgraded. Routing is **by capability, not identity**: an `agent` matching a `chatBackend` shell id opens an agent shell (`system_prompt`/`preset`/`model` and watcher `mode`/`engine` reach its `openSession` ctx); otherwise the PTY agent path, which refuses the chatBackend-only fields (`system_prompt`, `preset`, `charter`, `tick_interval_secs`, `nudge_cooldown_secs`, `max_actions_per_tick`) and names them, just as a chatBackend shell refuses PTY launch `args`. `role` accepts `Orchestrator`, `Manager`, `Worker`, plus `Planner`/`Watcher` sugar that maps to `Manager` with `role_profile`. A `Planner` spawn requires `plan_id` and resolves an omitted parent as the role endpoint does. Watcher spawns require a watcher-capable `chatBackend` shell and `charter`; policy fields are clamped by the host. An unknown id errors with the available agent **and** shell ids. `report_to` sets where the worker's `agent report` lands (Tab id or `main`/`general`). `origin_peer` is **never client-supplied**: the mesh relay stamps it from the authenticated sender so a mesh-spawned worker reports back (*Remote mesh workers*). `team_id` + `creator_tab_id` (`codeterm agent spawn --team`) attach a ROLELESS agent to an existing Team: `creator_tab_id` must belong to `team_id`, and the pair is rejected with `role` or `plan_task`. Membership is checked before tab creation, so a refusal leaves nothing behind. Socket `team.adopt_agent` applies the same check to an already-running tab; see API-TRANSPORT. A **roled** Team member (including a replacement Orchestrator) uses `POST /api/teams/{team_id}/members` / socket `team.add_member` (API-TEAMS).

An unknown id on an authoritative catalogue or a published free-form live list is refused before spawn or task dispatch creates a tab. REST spawn returns HTTP 422 with `error.code` set to `invalid_model`, including `provider`, `requested_model`, `valid_model_ids`, and `refresh_command`. A bare id owned by multiple providers returns `ambiguous_model` with `ambiguous_providers`; choose the provider explicitly. Neither refusal has a transient retry hint. CLI `--json` preserves these fields under the error envelope's `data` for spawn, dispatch, and team pair.

**Response** (after the launch verdict, bounded at 20 s):
```json
{"tab_id": "a3f1b2c7", "pty_id": "c9d0e1f2", "agent": "claude", "title": "Worker-1", "role": "Worker", "launch_state": "ready"}
```

`launch_state` is `ready` once the agent is ready, or `starting` with a `warning` when it waits on an owner prompt, holds a provisional boot stage, or is still launching at the bound; `starting` never claims readiness. A provider that exits before readiness fails the spawn: REST answers 500 with `error.code` `launch_failed`, socket `agent.spawn` returns the same fields as error `data`, and the CLI exits non-zero. Both carry `tab_id`, `reason`, `output_tail` (the provider's last printed lines), and `retry_hint`; the failed Tab stays in `spawn_failed` for inspection. `plan task dispatch` reports the same failure, output tail included, through its startup-failed event.

### Remote mesh workers

A worker spawned on another peer with `codeterm agent spawn <agent> --mesh PEER --cwd <peer-workspace> [--report-to <tab|main|general>]` gets a **durable cross-mesh report target**: the CLI relays the spawn to `PEER`, whose relay ingress stamps an authenticated `origin_peer` onto the body, and the spawned Tab persists `report_target_mesh_peer = <caller's peer>` + `report_target_tab_id = <report_to>` (defaults to the caller's Tab). `--cwd` is required; `--task-file` is read locally and only its body is relayed.

The worker runs `codeterm agent report "..."` (no `--mesh`/`--tab`); its daemon relays it over the signed `/send` path (`kind: "report"`), so it surfaces on the origin peer as `[Report from <worker>@<peer>]`, like a local report. If no target resolves, `agent report` fails with an actionable hint, never a silent drop.

Report routing precedence: **remote mesh target → `parent_tab_id` → `report_target_tab_id` → `spawned_by_tab_id`**; never self.

### Plan-authorized task-less evidence reports
The live authenticated Plan owner grants a review or canonical-QA tab with `plan.review.authorize-reporter` / `codeterm plan review authorize-reporter`. The durable grant records the target's exact tab id, session id, and a one-way binding-nonce digest; it never exposes the bearer nonce through Plan history. The target reports with `plan_id` only: `task_id`, `status`, and task provenance remain absent. Success appends an immutable `plan_evidence_report` Plan stride, publishes the ordinary `agent-report` event to the live owner, and returns `plan_report` with the stride id and `task_transition:false`. Authorization is capability-based; role labels grant nothing. An absent caller session is resolved from the exact registered tab only with a matching stored and hook-reported nonce; an explicitly wrong session still refuses. Foreign, unbound, stale/rebound, task-bound, or ungranted callers change no Plan history or report store. A session change requires a fresh owner grant. Registered Planner and task-bound Worker paths are unchanged.

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

<!-- revision: 41 -->
