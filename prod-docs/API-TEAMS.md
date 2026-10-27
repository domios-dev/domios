# Team and plan-ownership API

REST contracts for task execution attempts, Team-bound plans, Team staffing, and Team close.

## Plan task attempts

Task execution attempts are append-only history on `Task.execution_attempts`. The CLI/API uses the stable `TaskExecutionAttempt.id` as `attempt_id`; `executor_tab_id` is the executor identity, not the primary key. `requested_model` and `provider_resolved_model` preserve the selected and provider-resolved ids; `session_model` records the model detected from that attempt's bound session and stays null until observed. `model_identity_mismatch` records a difference and its cause. Respawn creates a new Worker tab and live attempt, never reactivating the selected historical attempt.

| Method | Path | Body / Query | Description |
|--------|------|--------------|-------------|
| GET | `/api/plans/{id}/tasks/{tid}/attempts` | — | List execution attempts recorded for one task |
| GET | `/api/plans/{id}/tasks/{tid}/attempts/{attempt_id}` | — | Show one attempt by stable attempt id, including derived report command, derived prompt, and last report summary when present |
| POST | `/api/plans/{id}/tasks/{tid}/respawn` | `{attempt_id, provider?, model?, parent_tab_id?, workspace_id?, title?}` | Build a Worker prompt from the task body, selected prior attempt, last report/status, lane branch/worktree hints, and exact `domios agent report --plan ... --task ...` command; spawn a new Worker and append a new `execution_attempts` entry |

Task dispatch requires explicit `execution_intent` (`read_only` or `mutating`). Mutating dispatch requires known provider-allowance telemetry at or above `provider_reserve_pct` (default 5); a stale snapshot emits a warning but uses its last known reserve value instead of deadlocking dispatch. This preflight runs before lane provisioning or task mutation. An interrupted mutating attempt projects `handoff_required` with a digest-verified Git checkpoint until a fresh owner adopts the next owner epoch.

`POST /api/plans/{id}/tasks/{tid}/adopt-checkpoint` adopts the sealed checkpoint for the nonce-authenticated Worker named by the Domios identity headers. The equivalent CLI command is `domios plan task adopt-checkpoint <plan> <task>`. Successful adoption increments `owner_epoch`, creates a fresh `awaiting_worker_ack` attempt, and fences prior owner sessions.

## Team-bound plan creation

An orchestrator's first plan creation is atomic: its response includes `plan.team_id`, top-level `team_id`, and `team_minted`. When `team_minted` is true, `_hints` includes `domios team rename <team_id> <name>` and `domios docs ORCHESTRATION`. The `mux-state-changed` snapshot includes the Team registry in `teams` and each tab's `team_id`; committed plan mutations also emit `plan-changed`. A Team with an active plan refuses a second plan with the unchanged `team_already_has_active_plan` error.

## Team-bound plan activation and transfer

Every active plan must reference exactly one live Team, and a live Team may have only one active plan. `POST /api/plans/{id}/unarchive` and `POST /api/plans/{id}/resume` reject a missing or stale Team binding; unarchive also rejects a Team that already owns another active plan. `team.adopt_plan` activates an archived/teamless plan atomically, replaces its retired owner with the target Team's sole live Orchestrator, records owner history, clears the stale planner binding, and relinks live task hierarchy to that Orchestrator. A Team with zero or multiple live Orchestrators cannot adopt implicitly; use an explicit transfer destination when applicable.

`POST /api/plans/{id}/transfer` accepts `{to: OrchestratorRef, reason?}` and requires `X-CodeTerm-Tab-Id` to identify the source Team's Orchestrator or planner-profile Manager. Missing callers, Workers, and foreign-Team tabs receive `403` without mutation. The destination must be a live Orchestrator belonging to a live Team. Owner, owner history, Team binding, and stale planner retirement are one storage mutation, followed by hierarchy relinking under the Team lock.

## Team staffing and Orchestrator recovery (POST /api/teams/{team_id}/members)

`{provider, role, title?, task?, brief?, model?}` adds one member to an existing Team; `role` is `worker`, `planner`, `orchestrator`, or `cartographer`. Supply either `task` or `brief`, never both. This endpoint, the UI add-member action, the socket `team.add_member` verb, and `domios agent spawn <provider> --team <id> --role Orchestrator` are one service path. `domios team attach-cartographer <team> --provider <provider> [--model <model>] [--brief-file <path>]` uses that same path; `-` reads stdin verbatim.

A Team may have one optional Cartographer Manager. The first attachment spawns a tab with the built-in `cartographer` role kind and delivers the supplied brief as its launch task. Repeated attachments return the same `tab_id` with `reused: true` and deliver the brief to that tab without creating another; a send refusal fails the request. It is a non-anchoring Manager without Team-management, plan, or spawn authority. Team close removes the Cartographer tab and its Team binding with every other member; the default Orchestrator/Planner pair does not include one unless explicitly attached.

A Team holds **at most one live Orchestrator**, enforced as a headcount of live anchoring tabs, not a role ban: a removed tab is gone from the multiplexer and a dead agent's identity is cleared on lifecycle exit, so neither counts. Unlinking the final Orchestrator atomically retires its owner reference while keeping the discoverable Team and active Plan id/history, so the Team accepts the documented replacement command and the new anchor takes that Plan over through the tab-gone owner rebind. Descendant role bindings follow the ordinary unlink rules. The response carries `plan_takeover: {plan_id, owner_tab_id}` when ownership moved. A takeover failure fails staffing, removes the spawned replacement and lane, and leaves the retained Team and Plan owner/history unchanged. Because `plan.owner` moves, the plan heartbeat targets the new tab from the next scan on.

`X-CodeTerm-Tab-Id` is optional on this endpoint alone: a Team whose Orchestrator is gone has no membership left to spend. A caller-less add can place only an anchoring role; `worker`, `planner`, and `cartographer` still need a spawn-capable member. This bootstrap action only creates a member and cannot delete Team state; destructive Team and workspace closes without a caller tab require explicit target confirmation. Every structural refusal leads with a stable error code and is logged with Team, requested role, and cause: `team_orchestrator_already_live`, `team_member_role_unknown`, `team_role_requires_spawner`, `team_caller_unauthorized`, `team_caller_cannot_spawn`, `team_workspace_unresolved`. A structural refusal is never surfaced as a retry prompt.

## Move a running tab into a Team

`domios team adopt-agent <team> <tab>` attaches a running roleless agent or a plain shell to a Team you belong to. A shell joins the Team's utility shells and keeps its process (for example a dev server); describe it with `domios tab describe`.

`domios team adopt-agent <team> <tab> --peer <peer>` adopts an unbound remote Worker into the local Team and re-points reports to its live Orchestrator. `domios agent reparent <tab> --to <local-orchestrator> [--peer <peer>]` moves an unbound local or remote Worker without restarting its session. Both origin-side operations require the current parent's authenticated session and nonce, or the local owner. The destination cannot pull a Worker. Remote operations require Full-access pairing; `--mesh` cannot replace `--peer`. Task-bound Workers refuse these commands: use their Plan ownership path.

| Method | Path | Body | Description |
|--------|------|------|-------------|
| POST | `/api/teams/{team_id}/adopt-remote` | `{tab_id, peer}` | Adopt an exact remote Worker into this local Team; caller identity comes from Domios identity headers. |
| POST | `/api/agents/reparent` | `{tab_id, to, peer?}` | Re-parent an unbound Worker to a live local Orchestrator, preserving its session. |
| GET | `/api/teams/{team_id}/member-inventory` | — | Local and durable remote member rows, pending remote transfers and the remote close reservation. |

Success includes `team_id`, `tab_id` and, for remote Workers, `peer`, `operation_id` and the exact-session `member`. An uncertain remote response or a failed local commit retains the durable operation; repeating the same request as its authenticated caller recovers it without a second transfer. Conflicting requests refuse until that operation resolves. A peer cannot invoke these origin-side APIs to adopt a local tab; only the signed worker-side mesh operation described in API-AGENTS can change its own origin's Worker.

`team show` and member inventory include remote members with peer, remote tab, session and provider identity; report expiry never removes their membership. They also appear as ordinary remote Team tabs. Offline and unpaired members remain visible as unavailable. A replacement session cannot inherit the old member's control surface. Pending transfer intent blocks lifecycle changes of its source caller and destination; Team close refuses unresolved transfers.

## Close Team (POST /api/teams/{team_id}/close)

Closes every Team member through the Team cascade path, removes the Team registry entry, archives and detaches every bound plan, retires each plan owner into owner history, clears its owner/planner routing authority, finalizes open attempts and lanes, deletes Team crons, and emits one committed `mux-state-changed`. This is the only user operation that removes a Team. Ordinary tab closes keep the Team record; the one automatic exception is a pair-minted startup lease, whose forming, empty, fully owned Team is reaped once its two reserved launch attempts are gone. A present `X-CodeTerm-Tab-Id` header must identify the Team's Orchestrator or planner-profile Manager. With no caller tab header, a confirmed human sends `{force: true, confirm: "<team_id>"}` after the close dialog; the service logs this as `human-attributed (headerless, confirmed)`. Missing or mismatched confirmation is refused as Unattributed and logged. Unauthorized callers return `403`, and an unknown or already closed Team returns structured `404`. A busy Team (task `in_progress` or Worker working) returns `409` unless `force_busy: true`; plans with unfinished tasks stay active and flagged, never archived: see ORCHESTRATION-OPERATIONS.

Remote membership freezes during close. All nonfinalized remote members receive an authenticated Full-access preflight before any remote stop. An offline, unpaired, restricted or busy peer retains the Team and its members; a fresh failed preflight releases the provisional freeze. Closing stops only the exact recorded provider, session and parent. An absent tab or changed provider/session/report binding releases stale membership without closing its replacement. Each terminal receipt is persisted; a lost response is recovered by another authenticated close, and already finalized members are skipped. Late telemetry and cached rows for that finalized session cannot recreate a Team tab. A later failure retains partial close progress and the Team until cleanup completes. The response's `remote_members` lists `{peer_id, tab_id, provider, session_id, outcome}`, where outcome is `closed`, `absent` or `released_stale_binding`.

## Execution authority and recovery

### Recovering a tab-gone task

The live Plan owner may close pending, blocked or in-progress tasks directly with `plan task close --result-file`, or record `--status done` with a nonblank `--step` result note for that task. Both paths verify the owner's current session binding, persist the result and clear executor assignments. Worker completion goes through reports and review; a task body alone is not a result note.

When a Worker's tab is gone, its owner epoch is stale by definition, so the epoch never blocks recovery. The **live authenticated Plan owner** may `cancel`, `pend`, reassign, respawn, or **dispatch** such a task. The recovery retires the stale attempt in the *same* transition that clears the dead owner identity, so no half-applied state is observable and a later `dispatch_ready` for the retired attempt is rejected instead of resurrecting ownership. A non-owner, non-epoch caller is still refused.

Plan ownership is proven against the caller's **live tab binding**: tab present, agent session live, binding nonce both stored and reported back, and that tab recorded as the plan's owner with Team-Orchestrator authority. It is never proven from the owner record's `session_id`, so an Orchestrator whose session moved by restore, compaction, or provider relaunch stays its own plan's owner.

Both Plan-owner paths are gated on **closed verb allowlists**: recovery verbs are a subset of the verbs where the Plan owner has authority over a task it does not hold. A verb asserting the *caller's own* task binding, such as a Worker report, is on neither list and can never ride the recovery hatch. Refusals carry a stable `data.type` naming the blocking state, so a tab-gone task is never reported as a stale epoch.

A genuine respawn preserves predecessor reports and review evidence, retires the prior attempt, and enters pending startup. Runtime readiness precedes the awaiting-ACK label; only the fresh Worker's ACK starts execution.

Dispatch refuses `paused`, `done`, `cancelled`, and `in_review` tasks before spawning. Explicit dispatch of a blocked task clears it to pending in the same mutation that reserves its attempt and records the prior block reason in a plan stride. Respawn also clears a blocked task atomically and records that reason; respawn is additionally the supported way to replace an attempt held in review.

Plan writes require the caller's live bound session. When the caller presents its own launch nonce while binding is still in flight, the daemon waits for the binding event through the startup deadline. A timed-out write returns `data.type = "session_binding_pending"` with `data.retryable = true`; a settled launch without a registered session remains the distinct `plan_binding_session_unregistered` error.

### Bouncing a cleaned terminal review generation

The existing authenticated review-bounce transition can recover a cleaned terminal generation when its original closed-attempt selector finds zero candidates. A unique original match keeps its existing behavior, and multiple original matches remain an ambiguity refusal.

The recovery requires an `in_review` task with no assigned executor, peer, or remaining lane tab, workspace, worktree, or branch, and `cleanup_status: done`. Every attempt must be closed and noncompetitive. Persisted attempt order must agree with strictly increasing, present assignment times; duplicate identities, overlapping generations, and contradictory times refuse selection. Only the last appended generation is considered, and it must have a nonblank executor session and binding nonce, an ACK, a persisted own-session `in_review` report, and ordered assignment, ACK, report, closure, and retirement facts. Continuation round metadata must describe the current review round, and replay labels or continuation-state conflicts refuse the fallback. An incomplete terminal generation blocks the transition even when an earlier generation has a valid report.

A successful bounce records one review evidence item and milestone, advances the review round and credit through the existing transaction, marks the selected predecessor `executor_gone`, and returns the task to pending with no continuation. The transaction preserves historical reports, launch metadata, and closure and retirement times. The retired executor receives no delivery; fresh placement follows ordinary dispatch. Refusals leave task, review credit, and milestones unchanged. This contract adds no command or wire fields.

This verb requires the live owning Orchestrator's top-level `caller_tab_id` and `session_binding_nonce`; `session_id` is optional under existing current-tab resolution. `from` remains attribution only and never supplies authority. The v3 gate refuses v2 clients before dispatch with `-32002`, `data.type = "version_skew"`, and the recovery command.

### Review credit repair and evidence reporters

`domios plan review delegate <plan> <task> --qa-tab <tab>` hands an `in_review` task to a direct child QA: only that bound session may accept or bounce it, with a non-empty evidence note; the verdict consumes the grant, which never authorizes hold, dispatch or other transitions.

`domios plan review reconcile-credit <plan> --reason "<incident evidence>"` auditably repairs review credit leaked by older builds. Credit repair and capacity classification require the live bound Orchestrator session and record an immutable plan stride. For task-less review or canonical-QA evidence, the live Plan owner runs `domios plan review authorize-reporter <plan> <tab> --class review|canonical_qa`; that exact session gains evidence-only authority and reports with `domios agent report --plan <plan>` and no task/status, producing a Plan stride and an ordinary owner event without a task transition. Missing, foreign, rebound, task-bound, or stale-owner grants fail before persistence or delivery. A fresh shell may omit `CODETERM_SESSION_ID`: evidence reporting resolves the registered tab session only with a matching stored and hook-reported nonce; never inject another tab's session.

Planner spawns (CLI, REST, service) resolve an omitted `--parent-tab` from the Plan owner before provisioning; a missing owner refuses with no tab created.

### Task-bound plan skill reads

An omitted `--plan` on `domios plan skill` uses the authenticated caller's Team active plan. Inherited launch plan, task, and attempt values do not override that live binding. An explicit `--plan` reads the named plan without rebinding the Team; a read of another plan records no current-task skill-load receipt.

The caller supplies its tab and binding nonce. The daemon derives its session only when the issued and session-reported nonces match; a supplied session must match too. Same-plan task evidence requires exactly one current assignment and an exact announced content digest. Explicit task or attempt conflicts, ambiguous assignments, unannounced skills, and changed bytes refuse.

Dispatch announces the manifest in its brief. For a supported existing-tab assignment, an authenticated `domios agent ack` returns the current manifest and its brief, recording that announcement on the current attempt before a load can be recorded. Repeating ACK retains an existing announcement and ACK; it never copies predecessor loads or replaces an announced digest. A startup ACK can still precede runtime binding; the first ACK returns only the manifest already announced by dispatch. Repeated ACK can complete both missing attempt binding fields only with retained launch authority for the exact creator, Team, task/attempt, owner epoch, PTY and nonce, plus the current session's matching issued and reported nonce. A verified session occupying the tab cannot mint that authority. The guarded write preserves ACK/timestamps, history, manifest, loads, status and lane. Exact already-bound equality is idempotent; partial, foreign, ambiguous, closed or retired ownership, absent/finalized launch proof and unannounced or changed manifests refuse without repair. A refusal returns `plan_skill_attempt_binding_missing`, the retained ACK timestamp, and the plan, task and attempt identifiers. Report those identifiers to the Orchestrator for launch-proven binding repair; this operation never rearms finalized launches or assigns a replacement attempt.


## Additional REST routes

Team staffing and closing use the authority and confirmation rules above.

| Method | Path | Operation |
|--------|------|-----------|
| GET | `/api/teams/{team_id}/mission-control/spend` | Read Team spend projections. |
| GET | `/api/teams/{team_id}/mission-control/state` | Read Team mission-control state. |
| POST | `/api/orchestration/new` | Create an Orchestrator/Planner Team. |
| POST | `/api/tabs/{id}/make-orchestrator` | Promote the selected tab to Orchestrator. |
| POST | `/api/tabs/{id}/make-planner` | Promote the selected tab to Planner. |
| POST | `/api/teams/{team_id}/close` | Close a Team and its owned resources. |
| POST | `/api/teams/{team_id}/members` | Add a Team member. |
| POST | `/api/teams/{team_id}/rename` | Rename a Team. |

<!-- revision: 11 -->
