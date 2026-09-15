# Orchestration control loop

This document is the compact operating contract for a CodeTerm Orchestrator.
Use command `--help` for syntax instead of loading a command catalogue.

## Related documents

Start narrow:

```bash
codeterm orchestrator status --plan <plan> --tab <orchestrator-tab>
codeterm plan reconcile <plan>
codeterm task show <task>
```

Use `codeterm orchestrator inspect --plan <plan> --json` only to investigate a specific discrepancy that compact status cannot explain.

Use `codeterm docs <NAME>`; it returns the complete document. Use `codeterm docs` or `codeterm docs --help` to discover available names. Read a related playbook when its responsibility becomes active:

| Need | Read |
|---|---|
| Planner ownership and task cutting | `codeterm docs PLANNER` |
| Worktrees, integration, cleanup | `codeterm docs LANES` |
| Narrow plan edits | `codeterm docs PLAN-EDITING` |
| Bounce evidence and retry limits | `codeterm docs BOUNCE` |
| Test authoring | `codeterm docs TEST-AUTHORING` |
| Verification, integration, heartbeat, closeout | `codeterm docs ORCHESTRATION-OPERATIONS` |
| CLI syntax | `codeterm <command> --help` |

## Durable model

CodeTerm planner is the source of truth:

```text
GOAL → Spec (WHAT/WHY) ⊥ Plan (HOW) → Tasks → Strides
```

- The Orchestrator sets the GOAL first, then delegates the rest of the structure.
- Requirements, constraints, acceptance, and vocabulary belong in Spec.
- Phases, ownership, sequencing, and verification belong in Plan.
- Tasks carry bounded scope, ownership, acceptance criteria, correctness, and an executable test plan.
- Strides record evidence, decisions, reports, and working points.
- Do not replace planner state with a standalone markdown implementation plan.

The Orchestrator states the GOAL and delegates planning by default; the Planner drafts structure, and the Orchestrator approves or revises it, dispatches execution, judges reports, integrates accepted lanes, and owns cleanup.
A bound Planner's authority over sections and tasks is durable, not phase-scoped: it survives approval. Approval and dispatch stay the Orchestrator's alone.
The Orchestrator may implement a genuinely small local change when delegation
would cost more than the work, but Worker dispatch is the default for plan-owned source work. Never self-assign a normal implementation task merely because canonical is writable.
When Team-bound, utility shells and cron jobs inherit the same Team for cleanup. Use `--no-team` only when a resource must deliberately outlive it.

Lane safety has two hard stops and otherwise only nudges; see the lanes playbook.

### Recovering a tab-gone task

The live Plan owner may cancel, pend, reassign, respawn, or dispatch a task whose
Worker tab is gone; the recovery retires the stale attempt in the same transition.
Ownership proof, verb allowlists, and refusals: `codeterm docs API-PLAN`.

Archiving a plan with no retrospective succeeds with the advisory
`plan.retrospective_unrecorded`; write it first with
`codeterm plan retrospective <plan> --body-file <pad-relative-path>`.

## The loop

Run this loop after every report, reminder, task close, tab death, or gate:

1. **Handle pushed reports.** Worker reports arrive automatically as incoming
   messages. Accept or bounce them without polling an inbox.
2. **Reconcile.** Run `codeterm plan reconcile <plan>`; fix only owned drift.
3. **Re-derive the frontier.** Inspect all open tasks, not only the previous focus.
4. **Give every actionable task an exit** under the task sequencing contract;
   plan reminders name only the actionable task ids.
5. **Keep the frontier staffed.** Do not leave a capable Worker idle while
   independent actionable work exists. Clear dispatch, dependency, review, and
   cleanup blockers first, then reuse a matching Worker or dispatch the next
   bounded task without duplicating ownership.
6. **Keep source writers parallel.** Implementation Workers use isolated,
   source-only lanes based on concurrency and file ownership.
7. **Own canonical verification.** Real-merge accepted ancestry first (never
   cherry-pick, squash, replay, or patch-copy), then run every build, test, dev
   check, and gate in the canonical checkout.
8. **Let heartbeat follow state.** Heartbeat is derived automatically from
   plan and task state; answer reminders by changing task or plan state, never
   heartbeat, focus, or lane fields.
9. **Clean accepted work.** Close the Worker tab, remove the ancestry-merged
   worktree and branch, stop its dev process, and record cleanup.

Discovery is an intermediate wave. It must produce decisions and follow-up
implementation/review/verification tasks; research alone does not complete the
goal.

## Planning ownership

When a Planner is present:

- the active Plan belongs to the Team and is visible through Team membership;
- run `codeterm plan doctor <plan> --kind quality`, then use one `codeterm plan
  approve <plan>` action to approve the structure and activate its tasks;
- duplicate worker guidance across tasks is a doctor warning to judge, never an
  approval block by itself;
- after a green doctor, summarize the proposed plan to the user and explicitly
  ask for approval; never stop silently at a completed draft;
- do not dispatch until approval is durably recorded. After UI approval, verify
  `planning_state=approved`; after explicit conversational approval, run the
  approve action and verify the same state;
- never ask the Planner to self-approve or manually set `structure_ready`;
- draft tasks remain non-dispatchable;
- the Orchestrator remains the sole dispatch and acceptance authority.

Judge structure before dispatch against the plan-shape contract in your role
brief. If the Planner is intentionally waived, record that explicitly on the plan.

## Task lifecycle

```text
pending → in_progress → in_review → done
              ↘ blocked
pending/in_progress/blocked → paused   (system holds only)
any open state → cancelled
```

- `in_progress` requires both a live runtime binding and the Worker's durable
  `codeterm agent ack`; either may arrive first.
- `dispatch_ready` proves PTY, session, and lane readiness but does not start
  the task. A missing ACK emits `worker_ack_timeout` after the bounded deadline.
- `codeterm agent report --status in_review` atomically records the handoff,
  moves the bound task to `in_review`, and pushes the report into the
  Orchestrator tab. Workers never poll or separately update task status.
- The Orchestrator alone accepts, bounces, or holds an `in_review` task. `hold`
  records a reason while preserving its assignment and lane; `bounce` returns it
  to the assigned Worker for another pass.
- `paused` is a system hold, never an agent verb: a decision gate (`gate:<id>:`
  reason) or provider exhaustion with a resumable checkpoint. It is excluded
  from open work and resumes when the gate is answered or the provider recovers.

## Group milestones

Group tasks with `--group` and declare the groups with `codeterm plan groups
set`. The group close the task sequencing contract requires has two records,
and dispatch waits for both once every task in the group is terminal and at
least one is done:

1. the regression run: `codeterm plan qa next <plan> --group <group> --note "<evidence>"`;
2. the retrospective: `codeterm plan groups close <plan> <group> --note "<what worked, what failed, what changes next>"`.

An authorized `codeterm plan qa override` waives only the regression run, never
the retrospective. Tasks tagged `regression-gate` or `release-blocking` still
dispatch. Plan pause and resume are part of the same contract.

## Dispatch contract

Before a wave:

1. Check provider headroom; near 95%, reuse live Workers instead of spawning
   cold duplicates.
2. Define a disjoint lane map: task, files, branch/worktree, expected output,
   focused oracle, and central gates requested.
3. Run plan doctor.
4. Dispatch through `codeterm plan task dispatch`; lanes by default,
   `--canonical-writer` only by deliberate choice (operations playbook).
5. Wait for the pushed `worker_ack` described in Task lifecycle.
6. If dispatch fails, alert the user immediately and verify rollback. Never
   blind-retry a write.

Every Worker is visible under the Orchestrator with a clear title and description,
`CODETERM_PLAN_ID` and `CODETERM_TASK_ID`, the checkout the Orchestrator chose, and a
self-contained brief with exact acceptance and test commands; it acks first and reports last.

`plan task dispatch` is local-only. A Worker spawned with `agent spawn --mesh PEER`
has no attempt on the origin plan store and no plan/task env, so it reports with
explicit `--plan <origin-plan> --task <task>` to home the report back, or the
Orchestrator attaches the arriving tab message by hand. Full cross-mesh dispatch is
out of scope; see the spawn-then-bind pattern in `codeterm docs MESH`.

## Review capacity

Choose review granularity from backlog and diff size. A few large changes merit
individual review; a growing queue of compatible small changes should share one
review wave and canonical QA group. After real merge, the Orchestrator may fix a
bounded mechanical integration defect directly in canonical. Bounce ownership,
correctness, authority, scope, or design defects instead of grinding tiny edits.

Review backlog and credit debt are advisory, not dispatch blockers. At the default five-review threshold, status and heartbeats warn the Orchestrator to prioritize review closures and avoid growing the backlog.
Ordinary dispatch, competition, and respawn remain allowed without a capacity grant. Authorization, dependency, regression, and review-acceptance checks remain enforced; never fake acceptance or adjust credit merely to silence a warning.

The following owner-only commands can still record review-work classification;
they are not required for dispatch and do not grant additional authority:

```bash
codeterm plan review authorize-capacity <plan> <task> --class review
codeterm plan review authorize-capacity <plan> <task> --class canonical_gate
```

Generic task updates, decision gates, and removals cannot move a task out
of `in_review`. Use `codeterm plan review accept|bounce|hold <plan> <task> --note
"…"`: `bounce` returns the work to the Worker, `hold` keeps the review open with
the assignment and lane intact, and every verdict requires `--note`, never
`--reason`. Review credit repair, task-less evidence reporters, and Planner parent
resolution are plan surface contracts (API-PLAN).

<!-- revision: 83 -->
