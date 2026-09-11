# Plan Surface Contracts

Contracts for plan-owned task structure, goals, lanes, and executors, as the CLI and socket surface enforce them.


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

### Recording an executor that runs elsewhere

A task executed by an agent on another machine records its executor with the
owner-side verb that already exists:

```
codeterm plan task assign <plan> <task> --tab <tab-id> [--mesh <peer>]
```

This stores `assigned_tab_id`, `lane.tab_id` and `peer` as structured fields and
reads them back; there is no local-liveness requirement on the target tab, so a
mesh tab is a valid executor. `codeterm plan task show` renders them as an
`executor: tab <id> on peer <peer> (role <role>)` line. Do **not** record an
executor in a pause reason or any other free text — that is a record with no
reader.

`plan task update` deliberately has no assign flag: assignment is execution
authority and lives on its own verb, outside what a Planner may write.

<!-- revision: 2 -->
