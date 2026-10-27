# Plan Surface Contracts

Contracts for plan-owned task structure, goals, lanes, and executors, as the CLI and socket surface enforce them. To repair a moved or retired project root, use `domios plan set-project <plan> <path>` (socket `plan.set_project`, REST `POST /api/plans/{id}/project` with `{project_path}`). The target must be an existing absolute directory on that host; the old root need not exist. One serialized write updates `project_path` and resolves `canonical_project_path` using the target checkout, preserving all Plan, task, Team and attempt ids. The result has `plan_id`, both paths and `changed`; repeated identical requests return `changed: false` without a write or event. Changed requests emit `plan-changed`.

### Same-attempt Worker authority restoration

`domios plan rebind <plan> --task <task> --attempt <attempt>` restores the current authenticated binding on the exact existing Worker attempt. The CLI reads that task once for its current `interruption.owner_epoch`; the daemon then requires the caller's own live local PTY session, issued and provider-reported binding, original provider/session/tab assignment, current epoch, a unique acknowledged `in_progress` or `in_review` attempt, and an ordered host-owned lineage suffix beginning with the old nonce entry actually bound to that same session. The caller session may be omitted: the daemon derives it from the caller's own tab only with matching issued and provider-reported nonce; a supplied session must match. A missing, foreign, ambiguous or unqualified retained suffix refuses with `data.type = "attempt_binding_restore_refused"` and typed `data.kind` and `data.reason`; task-targeted requests never fall back to the planning-session rebind path.

The successful response is `{ok, plan_id, task_id, attempt_id, status}`, where `status` is `restored` or `already_current`. It contains no nonce. One serialized PlanStore write changes the attempt's active nonce and appends a typed digest receipt. Assignment, ACK, report, review, status, lane, model and loaded-skill history remain unchanged; the receipt does not claim a new ACK, skill read or report. Existing skill-load and report guards keep comparing against the single active nonce. The retained suffix proves only its entries from the matched old nonce onward; restoration makes no claim about history before that entry. A daemon that does not return the typed response has not confirmed the targeted operation.

After a committed restoration, rollback must preserve the current nonce and receipt. Do not downgrade to a daemon that drops unknown receipt fields when rewriting the plan.

### Durable task keys and references

Every socket, REST, and CLI argument that names a task accepts its `task_xxxxxx` id, its handle, or its human key (`PREFIX-N`, any case). The id stays primary. A new key is issued once from the plan's owning local workspace: a 2-4 character uppercase prefix derived from the workspace name (collisions take the next deterministic candidate), fixed by the first key, plus a counter that never decreases or reuses a number. A legacy task stores no `key`; its display key is its id without `task_`, uppercased. An exact id wins; a handle and key naming different tasks are refused as `ref_ambiguous`. Task list/detail JSON carries `display_key` beside `task.id` (`task.key` only when stored), sibling/parent labels carry `display_key`, and `plan.task.add`, `POST /api/plans/{id}/tasks`, and `POST /api/tasks` return `key`.

`plan task add|update --group <key>` declares an unknown group with its key as label (`plan groups set` relabels), so `plan groups close` accepts every group a heartbeat names.

Task text is capped at 256 KiB per task (all fields together), every other plan text field at 256 KiB, and stored plan text at 16 MiB. Control bytes such as ESC and BEL are refused (bodies and narratives keep line breaks). Plan, section, and task names are required, one line, and at most 256 characters.

### Planner authority over task structure

A tab bound as a plan's Planner (`role = Manager`, `role_profile = planner`, same Team as the plan) holds **durable** authority over task *structure* for the plan's whole life — name, body, sections, acceptance criteria, test plan, QA plan, correctness, tags, dependencies and routing. That authority does not lapse when a task is dispatched: it applies to a `pending` task and an `in_progress` one alike. Fresh pair Planners are bound to their first Plan from live Team membership. Each write still requires that Planner tab's current session and binding nonce; when startup binding is pending, the write waits for that event instead of requiring `plan rebind`.

A Planner registered before its session exists records that session when its own binding is authenticated. A plan-only report can complete a missing registered session from the exact Planner tab's live binding; a stored different session or a stale caller still refuses. It stops at execution, except that the Plan owner or Planner may deliberately block a task by passing `status=blocked` with a fresh nonblank `status_label` containing its reason and owner trigger. A Planner is refused for other `status` or `status_label` changes, `lane`, `tab`/`assign_tab`/`assigned_tab_id`, `peer`, attempt fields, `competition`, `review_stage` and `qa_state`, and it may not assign, dispatch or approve. A Worker's `agent report --status blocked` is a proposal: it persists the evidence and leaves task state unchanged. One field list bounds Planner authority on every write path.

Refusals carry a stable `data.type`: `caller_is_not_task_owner_or_plan_owner` when the caller holds no authority over the task at all, and a `-32003` `plan_role_authorization_required` when the binding is live but the role is not.

### `plan goal set` is a partial update

`domios plan goal set <plan> [--vector V] [--vision T] [--vision-file P] [--constraint C ...] [--postulate P ...]`

**A field you do not pass is kept.** Setting `--vector` alone leaves the vision, constraints and postulates exactly as they were; setting `--constraint` alone leaves the vector. Passing a repeatable flag replaces that whole list; passing it with an empty value clears the list. There is no wholesale-replacement mode: naming every field is how you replace everything.

`--vector` is required only when the plan has no goal yet, and when it is missing the refusal names the flag: `data.type = "goal_vector_required"`, with `data.remedy.flag = "--vector"`.

### Dispatch stores the lane it reports

`plan.task.dispatch` onto a **pre-created worktree** (`--cwd <path>`) records `lane.branch`, `lane.worktree_path` and `lane.base` in the same transition that emits the `dispatch_ready` block, so the stored lane equals the reported one. Lane revisions and ancestry resolve in the repository that owns the worktree, discovered from its common Git directory; an extracted repository lane can belong to a plan rooted elsewhere. Every candidate checkout is checked for safety against the plan's canonical project path, never against itself; collapsing those two made a resolved lane look like the canonical checkout and dropped it (codeterm#425). A readiness report is no longer refused with "cannot locate your lane worktree" for a task dispatched this way.

### A mutating dispatch with no lane provisions one

A repository-mutating `plan.task.dispatch` whose task records no lane no longer resolves the canonical checkout as the worker checkout. The dispatcher branches one sibling lane off the plan's canonical project, commits its worktree, branch, base and Team workspace identity as a single value, and only then spawns. A lane the task already records is still used as-is, and a recorded lane that names the canonical checkout or another task's lane is refused before any tab is created (`data.type = "unsafe_mutating_lane"`). A recorded lane that fails verification is refused with `checkout_required` rather than replaced by a fresh one.

Both payloads that report the lane — the synchronous dispatch response and the `dispatch_ready` block — carry `lane.canonical_project_path` as the **plan's** canonical checkout and `lane.worktree_path` as the **worker's** lane, for a provisioned and a pre-created lane alike.

### Canonical writer placement is an explicit dispatch parameter

`plan.task.dispatch` accepts `placement: "isolated_lane" | "canonical_writer"`; absent or null is `isolated_lane`, and nothing else ever selects the canonical placement. `canonical_writer` requires `execution_intent: "mutating"`, a repository task, and no recorded lane (otherwise `invalid_task_dispatch`). It verifies canonical as an attached checkout, skips lane provisioning, and runs the checkout occupancy preflight against canonical, so any live attempt there refuses it with `checkout_occupied`. The attempt persists `placement`; readiness reports use that placement to validate `--sha` against the plan's canonical checkout, including when the attempt has no `worktree_path`. Canonical reports keep no base, so they require the reported commit to match canonical `HEAD` and refuse any dirty checkout, even with `--acknowledge-dirty`. `task.lane` stays empty, so lane cleanup never touches canonical. The dispatch response echoes `placement`.

For local dispatch, `worker_kind: "qa"` or `"release_manager"` explicitly selects the built-in Worker prompt kind for that attempt without changing `task.executor_role`. When omitted, an exact built-in leaf in `executor_role` selects the prompt kind. A kind affects prompt composition only; the live Plan owner still authorizes dispatch and the attempt reservation still controls task-bound canonical occupancy. Direct `agent spawn --plan-task` does not reserve that attempt and remains refused for task-bound canonical mutation. Task-less kinds with Plan context retain parent-directed placement behavior and do not reserve task attempts or participate in canonical writer occupancy, so the parent coordinates those writes. Read-only verification may still run there.

While such an attempt is live, `plan.review.accept` on another task that records a lane is refused with code `-32015`, `data.type = "canonical_writer_holds_checkout"`, `violation_id = "lane.integration_while_canonical_writer_holds_checkout"`, and the holder task and attempt ids. Gate claims stay bound to the attempt's exact `authorized_gates`.

### Recording an executor that runs elsewhere

A task executed by an agent on another machine records its executor with the owner-side verb that already exists:

```
domios plan task assign <plan> <task> --tab <tab-id> [--mesh <peer>]
```

This stores `assigned_tab_id`, `lane.tab_id` and `peer` as structured fields and reads them back. An explicit remote peer has no local-liveness requirement and never inherits the identity of a same-named local tab. A local assignment requires the target's verified live session binding and a pending task before creating its attempt; missing binding or an active/review-held task refuses without mutation. Ordinary assignment never clears a review hold; use review continuation or the genuine respawn lifecycle. A fresh attempt still requires its own task ACK. `domios plan task show` renders an `executor: tab <id> on peer <peer> (role <role>)` line. Never record an executor in a pause reason or other free text; nothing reads it. `plan task update` has no assign flag: assignment is execution authority, outside Planner writes.

Execution recovery, review credit and task-bound skill-read authority: API-TEAMS.

### A spawned Worker is not an accepted dispatch

The immediate response to `plan.task.dispatch` reports `state` `awaiting_worker_ack` with `spawned: true` and `accepted: false`. Durable acceptance is the later `dispatch_ready` block, emitted once the session binds. A session binding that is merely late — the tab is alive but has not bound before the deadline — is not a dispatch failure: the dispatch stays spawned-and-awaiting and the provider's own launch detection or the worker-ack deadline settles it. Only a confirmed-gone tab fails the dispatch.

### Reviewed remote lane integration

`plan.task.merge_remote_lane` requires the authenticated origin Orchestrator, an origin-accepted remote attempt, and a persisted Worker report. The origin requests a Git bundle from the recorded paired peer over signed mesh transport, fetches the recorded branch into a Domios-owned ref, then runs a real `git merge --no-ff`. It persists the lane tip and merge commit after `git merge-base --is-ancestor <lane-tip> <target>` succeeds. Repeating an already-recorded integration is idempotent.

### Splitting an in-progress task

The Plan Orchestrator can create child tasks beneath an in-progress task with `domios plan task split <plan> <task> --child <name>` or a JSON child file. Children inherit the parent's sections, tags (including its `work:<value>`), executor role, model routing, and quality fields unless overridden; a child tag replaces the inherited tag of the same facet key. They retain the same plan approval and may be dispatched while the parent remains in progress; each dispatch provisions the child's own lane. Task detail and plan state expose child statuses, and the parent presents as `blocked_on_subtasks` while any child is open. Marking the parent done or cancelled is refused until all children are done or cancelled. Worker briefs carry the parent task id; a Worker may propose children in its report but cannot dispatch or spawn them.



### Lane cleanup progress

`plan.lanes_clean` accepts optional `stream: true` and returns `{job_id}` immediately. The cleanup runs outside the async runtime. `plan.lanes_clean_next` with `{job_id, cursor: 0}` waits for events, then returns `{events, cursor, done, result}`. Each event contains `{lane, finished}`; pass the returned cursor to receive the next events. A final response contains the ordinary `{plan_id, dry_run, lanes}` result and retires the job. Without `stream`, the call returns that final result directly. Dry runs remain available while confirmed cleanup runs; overlapping mutations for the same plan are refused. The existing owner authorization applies to confirmation and fetch.


## Additional REST routes

Task writes require current caller, session and binding authority (API). Execution recovery and review authority: API-TEAMS.

| Method | Path | Operation |
|--------|------|-----------|
| DELETE | `/api/plans/{id}` | Delete a plan. |
| DELETE | `/api/plans/{id}/elements/{element_id}` | Remove a canvas element. |
| DELETE | `/api/plans/{id}/links/{to}` | Unlink plans. |
| DELETE | `/api/plans/{id}/notes/{element_id}` | Remove a canvas element. |
| DELETE | `/api/plans/{id}/sections/{sid}` | Remove a section. |
| DELETE | `/api/plans/{id}/spec-links/{spec_id}` | Unlink a specification. |
| DELETE | `/api/plans/{id}/tasks/{tid}` | Remove a task. |
| DELETE | `/api/plans/{id}/tasks/{tid}/milestones/{ms_id}` | Remove a milestone. |
| DELETE | `/api/specs/{id}/sections/{sid}` | Remove a section. |
| GET | `/api/efforts/{id}` | Read an effort. |
| GET | `/api/plans` | List plans in the selected scope. |
| GET | `/api/plans/{id}` | Read a plan. |
| GET | `/api/plans/{id}/attachments` | Read plan attachments. |
| GET | `/api/plans/{id}/control-loop` | Read the control-loop configuration. |
| GET | `/api/plans/{id}/coverage` | Read section coverage. |
| GET | `/api/plans/{id}/decision` | List decisions. |
| GET | `/api/plans/{id}/handoffs` | List plan handoffs. |
| GET | `/api/plans/{id}/handoffs/show` | Read a plan handoff. |
| GET | `/api/plans/{id}/loops/{lid}` | Read loop status. |
| GET | `/api/plans/{id}/next` | Read the next planned work. |
| GET | `/api/plans/{id}/pause/preview` | Preview pause effects. |
| GET | `/api/plans/{id}/reconcile` | Read reconciliation findings. |
| GET | `/api/plans/{id}/resume/preview` | Preview resume effects. |
| GET | `/api/plans/{id}/skills` | List announced plan skills. |
| GET | `/api/plans/{id}/skills/{slug}` | Read an announced skill. |
| GET | `/api/plans/{id}/state` | Read plan state. |
| GET | `/api/specs` | List specifications. |
| GET | `/api/specs/{id}` | Read a specification. |
| GET | `/api/specs/{id}/links` | List specification links. |
| GET | `/api/tasks` | List tasks in the selected scope. |
| GET | `/api/tasks/{id}` | Read a task. |
| PATCH | `/api/plans/{id}` | Update plan metadata and goal fields. |
| PATCH | `/api/plans/{id}/elements/{element_id}` | Update a canvas element. |
| PATCH | `/api/plans/{id}/layout` | Set canvas layout. |
| PATCH | `/api/plans/{id}/loops/{lid}` | Update loop settings. |
| PATCH | `/api/plans/{id}/notes/{element_id}` | Update a canvas note. |
| PATCH | `/api/plans/{id}/sections/{sid}` | Update a section. |
| PATCH | `/api/plans/{id}/tasks/{tid}` | Update task structure or authorized execution fields. |
| PATCH | `/api/plans/{id}/tasks/{tid}/milestones/{ms_id}` | Update a milestone. |
| PATCH | `/api/specs/{id}/sections/{sid}` | Update a section. |
| POST | `/api/canvas/focus` | Focus the task canvas. |
| POST | `/api/efforts/{id}/links` | Link an effort. |
| POST | `/api/plans` | Create a plan. |
| POST | `/api/plans/resolve-refs` | Resolve plan and task references. |
| POST | `/api/plans/{id}/archive` | Archive the plan. |
| POST | `/api/plans/{id}/batch` | Apply a batch of plan operations. |
| POST | `/api/plans/{id}/claim` | Claim plan ownership. |
| POST | `/api/plans/{id}/control-loop` | Set the control-loop configuration. |
| POST | `/api/plans/{id}/control-loop/run` | Run the control loop. |
| POST | `/api/plans/{id}/decision` | Ask a decision question. |
| POST | `/api/plans/{id}/decision/{gid}/answer` | Answer a decision. |
| POST | `/api/plans/{id}/decision/{gid}/escalate` | Escalate a decision. |
| POST | `/api/plans/{id}/disown` | Release plan ownership. |
| POST | `/api/plans/{id}/elements` | Add a canvas element. |
| POST | `/api/plans/{id}/groups` | Set task groups. |
| POST | `/api/plans/{id}/handoffs` | Create a plan handoff. |
| POST | `/api/plans/{id}/ingest` | Ingest structured plan content. |
| POST | `/api/plans/{id}/kinds` | Set task kinds. |
| POST | `/api/plans/{id}/links` | Link plans. |
| POST | `/api/plans/{id}/loops` | Create a plan loop. |
| POST | `/api/plans/{id}/loops/{lid}/record` | Record loop progress. |
| POST | `/api/plans/{id}/loops/{lid}/stop` | Stop a plan loop. |
| POST | `/api/plans/{id}/mode` | Set plan mode. |
| POST | `/api/plans/{id}/notes` | Add a canvas note. |
| POST | `/api/plans/{id}/pause` | Pause plan execution. |
| POST | `/api/plans/{id}/project` | Set the project root. |
| POST | `/api/plans/{id}/resume` | Resume plan execution. |
| POST | `/api/plans/{id}/retrospective` | Record a retrospective. |
| POST | `/api/plans/{id}/sections` | Add a section. |
| POST | `/api/plans/{id}/spec-links` | Link a specification. |
| POST | `/api/plans/{id}/tag-facets` | Set tag facets. |
| POST | `/api/plans/{id}/tasks` | Create a task. |
| POST | `/api/plans/{id}/tasks/{tid}/adopt-checkpoint` | Adopt an interrupted task checkpoint. |
| POST | `/api/plans/{id}/tasks/{tid}/assign` | Assign a task executor. |
| POST | `/api/plans/{id}/tasks/{tid}/close` | Close a task. |
| POST | `/api/plans/{id}/tasks/{tid}/milestones` | Add a task milestone. |
| POST | `/api/plans/{id}/transfer` | Transfer plan ownership. |
| POST | `/api/plans/{id}/unarchive` | Unarchive the plan. |
| POST | `/api/specs` | Create a specification. |
| POST | `/api/specs/{id}/sections` | Add a section. |
| POST | `/api/tasks` | Create a task. |
| POST | `/api/tasks/{id}/drop` | Drop task ownership. |
| POST | `/api/tasks/{id}/take` | Take task ownership. |

<!-- revision: 39 -->
