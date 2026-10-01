# Team and plan-ownership API

REST contracts for task execution attempts, Team-bound plans, Team staffing, and Team close.

## Plan task attempts

Task execution attempts are append-only history on `Task.execution_attempts`. The CLI/API uses the stable `TaskExecutionAttempt.id` as `attempt_id`; `executor_tab_id` is the executor identity, not the primary key. `requested_model` and `provider_resolved_model` preserve the selected and provider-resolved ids; `session_model` records the model detected from that attempt's bound session and stays null until observed. `model_identity_mismatch` records a difference and its cause. Respawn creates a new Worker tab and live attempt, never reactivating the selected historical attempt.

| Method | Path | Body / Query | Description |
|--------|------|--------------|-------------|
| GET | `/api/plans/{id}/tasks/{tid}/attempts` | — | List execution attempts recorded for one task |
| GET | `/api/plans/{id}/tasks/{tid}/attempts/{attempt_id}` | — | Show one attempt by stable attempt id, including derived report command, derived prompt, and last report summary when present |
| POST | `/api/plans/{id}/tasks/{tid}/respawn` | `{attempt_id, provider?, model?, parent_tab_id?, workspace_id?, title?}` | Build a Worker prompt from the task body, selected prior attempt, last report/status, lane branch/worktree hints, and exact `codeterm agent report --plan ... --task ...` command; spawn a new Worker and append a new `execution_attempts` entry |

Task dispatch requires explicit `execution_intent` (`read_only` or `mutating`). Mutating dispatch requires known provider-allowance telemetry at or above `provider_reserve_pct` (default 5); a stale snapshot emits a warning but uses its last known reserve value instead of deadlocking dispatch. This preflight runs before lane provisioning or task mutation. An interrupted mutating attempt projects `handoff_required` with a digest-verified Git checkpoint until a fresh owner adopts the next owner epoch.

`POST /api/plans/{id}/tasks/{tid}/adopt-checkpoint` adopts the sealed checkpoint for the nonce-authenticated Worker named by the CodeTerm identity headers. The equivalent CLI command is `codeterm plan task adopt-checkpoint <plan> <task>`. Successful adoption increments `owner_epoch`, creates a fresh `awaiting_worker_ack` attempt, and fences prior owner sessions.

## Team-bound plan creation

An orchestrator's first plan creation is atomic: its response includes `plan.team_id`, top-level `team_id`, and `team_minted`. When `team_minted` is true, `_hints` includes `codeterm team rename <team_id> <name>` and `codeterm docs ORCHESTRATION`. The `mux-state-changed` snapshot includes the Team registry in `teams` and each tab's `team_id`; committed plan mutations also emit `plan-changed`. A Team with an active plan refuses a second plan with the unchanged `team_already_has_active_plan` error.

## Team-bound plan activation and transfer

Every active plan must reference exactly one live Team, and a live Team may have only one active plan. `POST /api/plans/{id}/unarchive` and `POST /api/plans/{id}/resume` reject a missing or stale Team binding; unarchive also rejects a Team that already owns another active plan. `team.adopt_plan` activates an archived/teamless plan atomically, replaces its retired owner with the target Team's sole live Orchestrator, records owner history, clears the stale planner binding, and relinks live task hierarchy to that Orchestrator. A Team with zero or multiple live Orchestrators cannot adopt implicitly; use an explicit transfer destination when applicable.

`POST /api/plans/{id}/transfer` accepts `{to: OrchestratorRef, reason?}` and requires `X-CodeTerm-Tab-Id` to identify the source Team's Orchestrator or planner-profile Manager. Missing callers, Workers, and foreign-Team tabs receive `403` without mutation. The destination must be a live Orchestrator belonging to a live Team. Owner, owner history, Team binding, and stale planner retirement are one storage mutation, followed by hierarchy relinking under the Team lock.

## Team staffing and Orchestrator recovery (POST /api/teams/{team_id}/members)

`{provider, role, title?, task?, model?}` adds one member to an existing Team; `role` is `worker`, `planner`, `orchestrator`, or `cartographer`. This endpoint, the UI add-member action, the socket `team.add_member` verb, and `codeterm agent spawn <provider> --team <id> --role Orchestrator` are one service path. `codeterm team attach-cartographer <team> --provider <provider> [--model <model>]` uses that same path.

A Team may have one optional Cartographer Manager. The first attachment spawns a tab with the built-in `cartographer` role kind; repeated attachments return the same `tab_id` with `reused: true` and do not create another tab. It is a non-anchoring Manager without Team-management, plan, or spawn authority. Team close removes the Cartographer tab and its Team binding with every other member; the default Orchestrator/Planner pair does not include one unless explicitly attached.

A Team holds **at most one live Orchestrator**, enforced as a headcount of live anchoring tabs, not a role ban: a removed tab is gone from the multiplexer and a dead agent's identity is cleared on lifecycle exit, so neither counts. Unlinking the final Orchestrator atomically retires its owner reference while keeping the discoverable Team and active Plan id/history, so the Team accepts the documented replacement command and the new anchor takes that Plan over through the tab-gone owner rebind. Descendant role bindings follow the ordinary unlink rules. The response carries `plan_takeover: {plan_id, owner_tab_id}` when ownership moved. A takeover failure fails staffing, removes the spawned replacement and lane, and leaves the retained Team and Plan owner/history unchanged. Because `plan.owner` moves, the plan heartbeat targets the new tab from the next scan on.

`X-CodeTerm-Tab-Id` is optional on this endpoint alone: a Team whose Orchestrator is gone has no membership left to spend. A caller-less add can place only an anchoring role; `worker`, `planner`, and `cartographer` still need a spawn-capable member. This bootstrap action only creates a member and cannot delete Team state; destructive Team and workspace closes without a caller tab require explicit target confirmation. Every structural refusal leads with a stable error code and is logged with Team, requested role, and cause: `team_orchestrator_already_live`, `team_member_role_unknown`, `team_role_requires_spawner`, `team_caller_unauthorized`, `team_caller_cannot_spawn`, `team_workspace_unresolved`. A structural refusal is never surfaced as a retry prompt.

## Close Team (POST /api/teams/{team_id}/close)

Closes every Team member through the Team cascade path, removes the Team registry entry, archives and detaches every bound plan, retires each plan owner into owner history, clears its owner/planner routing authority, finalizes open attempts and lanes, deletes Team crons, and emits one committed `mux-state-changed`. This is the only user operation that removes a Team. Ordinary tab closes keep the Team record; the one automatic exception is a pair-minted startup lease, whose forming, empty, fully owned Team is reaped once its two reserved launch attempts are gone. A present `X-CodeTerm-Tab-Id` header must identify the Team's Orchestrator or planner-profile Manager. With no caller tab header, a confirmed human sends `{force: true, confirm: "<team_id>"}` after the close dialog; the service logs this as `human-attributed (headerless, confirmed)`. Missing or mismatched confirmation is refused as Unattributed and logged. Unauthorized callers return `403`, and an unknown or already closed Team returns structured `404`. A busy Team (task `in_progress` or Worker working) returns `409` unless `force_busy: true`; plans with unfinished tasks stay active and flagged, never archived: see ORCHESTRATION-OPERATIONS.

<!-- revision: 2 -->
