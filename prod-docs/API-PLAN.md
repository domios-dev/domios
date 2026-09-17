# Plan Surface Contracts

Contracts for plan-owned task structure, goals, lanes, and executors, as the CLI and socket surface enforce them.

### Durable task keys and references

Every socket, REST, and CLI argument that names a task accepts its `task_xxxxxx`
id, its handle, or its human key (`PREFIX-N`, any case). The id stays primary.
A new key is issued once from the plan's owning local workspace: a 2-4 character
uppercase prefix derived from the workspace name (collisions take the next
deterministic candidate), fixed by the first key, plus a counter that never
decreases or reuses a number. A legacy task stores no `key`; its display key is
its id without `task_`, uppercased. An exact id wins; a handle and key naming
different tasks are refused as `ref_ambiguous`. Task list/detail JSON carries
`display_key` beside `task.id` (`task.key` only when stored), sibling/parent
labels carry `display_key`, and `plan.task.add`, `POST /api/plans/{id}/tasks`,
and `POST /api/tasks` return `key`.


### Planner authority over task structure

A tab bound as a plan's Planner (`role = Manager`, `role_profile = planner`, same
Team as the plan) holds **durable** authority over task *structure* for the plan's
whole life — name, body, sections, acceptance criteria, test plan, QA plan,
correctness, tags, dependencies and routing. That authority does not lapse when a
task is dispatched: it applies to a `pending` task and an `in_progress` one alike.

It stops at execution. A Planner is refused for `status`, `status_label`, `lane`,
`tab`/`assign_tab`/`assigned_tab_id`, `peer`, attempt fields, `competition`,
`review_stage` and `qa_state`, and it may not assign, dispatch or approve. The
list is `PLANNER_EXECUTION_FIELDS` in `service/plan_authority.rs`, and both the
owner-epoch seam and the `plan.task.update` handler bound the Planner by that one
list, so widening either cannot widen what a Planner may write.

Refusals carry a stable `data.type`: `caller_is_not_task_owner_or_plan_owner`
when the caller holds no authority over the task at all, and a `-32003`
`plan_role_authorization_required` when the binding is live but the role is not.

### `plan goal set` is a partial update

`codeterm plan goal set <plan> [--vector V] [--vision T] [--vision-file P]
[--constraint C ...] [--postulate P ...]`

**A field you do not pass is kept.** Setting `--vector` alone leaves the vision,
constraints and postulates exactly as they were; setting `--constraint` alone
leaves the vector. Passing a repeatable flag replaces that whole list; passing it
with an empty value clears the list. There is no wholesale-replacement mode:
naming every field is how you replace everything.

`--vector` is required only when the plan has no goal yet, and when it is missing
the refusal names the flag: `data.type = "goal_vector_required"`, with
`data.remedy.flag = "--vector"`.

### Dispatch stores the lane it reports

`plan.task.dispatch` onto a **pre-created worktree** (`--cwd <path>`) records
`lane.branch`, `lane.worktree_path` and `lane.base` in the same transition that
emits the `dispatch_ready` block, so the stored lane equals the reported one.
Every candidate checkout is verified against the plan's canonical project path,
never against itself; collapsing those two made a resolved lane look like the
canonical checkout and dropped it (codeterm#425). A readiness report is no longer
refused with "cannot locate your lane worktree" for a task dispatched this way.

### A mutating dispatch with no lane provisions one

A repository-mutating `plan.task.dispatch` whose task records no lane no longer
resolves the canonical checkout as the worker checkout. The dispatcher branches
one sibling lane off the plan's canonical project, commits its worktree, branch,
base and Team workspace identity as a single value, and only then spawns. A lane
the task already records is still used as-is, and a recorded lane that names the
canonical checkout or another task's lane is refused before any tab is created
(`data.type = "unsafe_mutating_lane"`). A recorded lane that fails verification
is refused with `checkout_required` rather than replaced by a fresh one.

Both payloads that report the lane — the synchronous dispatch response and the
`dispatch_ready` block — carry `lane.canonical_project_path` as the **plan's**
canonical checkout and `lane.worktree_path` as the **worker's** lane, for a
provisioned and a pre-created lane alike.

### Canonical writer placement is an explicit dispatch parameter

`plan.task.dispatch` accepts `placement: "isolated_lane" | "canonical_writer"`;
absent or null is `isolated_lane`, and nothing else ever selects the canonical
placement. `canonical_writer` requires `execution_intent: "mutating"`, a
repository task, and no recorded lane (otherwise `invalid_task_dispatch`). It
verifies canonical as an attached checkout, skips lane provisioning, and runs the
checkout occupancy preflight against canonical, so any live attempt there refuses
it with `checkout_occupied`. The attempt persists `placement`, and on readiness
records `worktree_path` as the canonical checkout with no `base`, which keeps the
report guard strict. `task.lane` stays empty, so lane cleanup never touches
canonical. The dispatch response echoes `placement`.

While such an attempt is live, `plan.review.accept` on another task that records
a lane is refused with code `-32015`, `data.type = "canonical_writer_holds_checkout"`,
`violation_id = "lane.integration_while_canonical_writer_holds_checkout"`, and the
holder task and attempt ids. Gate claims stay bound to the attempt's exact
`authorized_gates`.

### Recording an executor that runs elsewhere

A task executed by an agent on another machine records its executor with the
owner-side verb that already exists:

```
codeterm plan task assign <plan> <task> --tab <tab-id> [--mesh <peer>]
```

This stores `assigned_tab_id`, `lane.tab_id` and `peer` as structured fields and
reads them back. An explicit remote peer has no local-liveness requirement and
never inherits the identity of a same-named local tab. A local assignment requires
the target's verified live session binding and a pending task before creating its
attempt; missing binding or an active/review-held task refuses without mutation.
Ordinary assignment never clears a review hold; use review continuation or the
genuine respawn lifecycle. A fresh attempt
still requires its own task ACK. `codeterm plan task show` renders them as an
`executor: tab <id> on peer <peer> (role <role>)` line. Do **not** record an
executor in a pause reason or any other free text — that is a record with no
reader.

`plan task update` deliberately has no assign flag: assignment is execution
authority and lives on its own verb, outside what a Planner may write.

### Recovering a tab-gone task

When a Worker's tab is gone, its owner epoch is stale by definition, so the
epoch never blocks recovery. The **live authenticated Plan owner** may `cancel`,
`pend`, reassign, respawn, or **dispatch** such a task. The recovery retires the
stale attempt in the *same* transition that clears the dead owner identity, so no
half-applied state is observable and a later `dispatch_ready` for the retired
attempt is rejected instead of resurrecting ownership. A non-owner, non-epoch
caller is still refused.

Plan ownership is proven against the caller's **live tab binding**: tab present,
agent session live, binding nonce both stored and reported back, and that tab
recorded as the plan's owner with Team-Orchestrator authority. It is never proven
from the owner record's `session_id`, so an Orchestrator whose session moved by
restore, compaction, or provider relaunch stays its own plan's owner.

Both Plan-owner paths are gated on **closed verb allowlists**: recovery verbs are
a subset of the verbs where the Plan owner has authority over a task it does not
hold. A verb asserting the *caller's own* task binding, such as a Worker report,
is on neither list and can never ride the recovery hatch. Refusals carry a stable
`data.type` naming the blocking state, so a tab-gone task is never reported as a
stale epoch.

A genuine respawn preserves predecessor reports and review evidence, retires the
prior attempt, and enters pending startup. Runtime readiness precedes the
awaiting-ACK label; only the fresh Worker's ACK starts execution.

### Review credit repair and evidence reporters

Historical review credit leaked by older builds is repaired auditably with:

```bash
codeterm plan review reconcile-credit <plan> --reason "<incident evidence>"
```

Credit repair and capacity classification require the live bound Orchestrator
session and record an immutable plan stride. For task-less review or canonical-QA
evidence, the live Plan owner runs `codeterm plan review authorize-reporter <plan>
<tab> --class review|canonical_qa`; that exact session gains evidence-only
authority and reports with `codeterm agent report --plan <plan>` and no
task/status, producing a Plan stride and an ordinary owner event without a task
transition. Missing, foreign, rebound, task-bound, or stale-owner grants fail
before persistence or delivery. A fresh shell may omit `CODETERM_SESSION_ID`:
evidence reporting resolves the registered tab session only with a matching
stored and hook-reported nonce; never inject another tab's session.

Planner spawns through CLI, REST, and synchronous service callers resolve an
omitted `--parent-tab` from the Plan owner before provisioning; a missing owner
refuses with the Plan identity and creates no tab, and an explicit parent wins.

### Task-bound plan skill reads

A task-bound `plan skill show` is proven by the caller's live tab, not by an
environment secret: the tab supplies its id and launch binding nonce, and the
daemon derives the session id only when that nonce is both the tab's current
nonce and the one its running session echoed back. A Worker therefore reads its
mandatory skills as soon as it launches, before any session id exists, while
stale, mismatched, partial, or foreign identities are refused. Reading a plan
skill as evidence still requires a current task binding for that plan.

<!-- revision: 7 -->
