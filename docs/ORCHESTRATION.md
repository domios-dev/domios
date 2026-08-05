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

Use `codeterm orchestrator inspect --plan <plan> --json` only to investigate a
specific discrepancy that compact status cannot explain.

Use `codeterm docs <NAME>`; it returns the complete document. Use `codeterm docs`
or `codeterm docs --help` to discover available names. Read a related playbook
when its responsibility becomes active:

| Need | Read |
|---|---|
| Planner ownership and task cutting | `codeterm docs PLANNER` |
| Worktrees, integration, cleanup | `codeterm docs LANES` |
| Narrow plan edits | `codeterm docs PLAN-EDITING` |
| Bounce evidence and retry limits | `codeterm docs BOUNCE` |
| Test authoring | `codeterm docs TEST-AUTHORING` |
| CLI syntax | `codeterm <command> --help` |

## Durable model

CodeTerm planner is the source of truth:

```text
GOAL → Spec (WHAT/WHY) ⊥ Plan (HOW) → Tasks → Strides
```

- Set the GOAL first.
- Requirements, constraints, acceptance, and vocabulary belong in Spec.
- Phases, ownership, sequencing, and verification belong in Plan.
- Tasks carry bounded scope, ownership, acceptance criteria, correctness, and
  an executable test plan.
- Strides record evidence, decisions, reports, and working points.
- Do not replace planner state with a standalone markdown implementation plan.

The Planner drafts structure. The Orchestrator approves or revises it, dispatches
execution, judges reports, integrates accepted lanes, and owns cleanup.
The Orchestrator may implement a genuinely small local change when delegation
would cost more than the work, but Worker dispatch is the default for plan-owned source work. Never self-assign a normal implementation task merely because canonical is writable.
When Team-bound, attach utility shells and cron jobs to the same Team with
`--team <team-id>` so Team close and cleanup own their lifecycle.

Plan-owned work is dispatched atomically with `codeterm plan task dispatch`,
which spawns the Worker, assigns the task, and delivers the brief in one call.
When a dispatch is blocked, clear the blocker or record the task as blocked.
Never substitute a loose roleless agent: an agent outside the Team and outside
the plan owns no task, appears in no plan state, and reports to nobody, so its
work is invisible to every gate that would otherwise catch it.

### Lane safety: two hard stops, everything else advisory

A **mutating** Worker dispatch is refused before any tab is created when its
resolved worktree is the canonical checkout, or is the lane another task owns.
Both would destroy work in progress, so both are hard stops carrying a stable
violation id (`lane.mutating_write_in_canonical_checkout`,
`lane.mutating_write_in_foreign_lane`) and the exact `plan task update` command
that supplies an isolated lane. A dispatch with no recorded lane is *not*
refused — the dispatcher provisions an isolated lane for it, committing Team
workspace identity and lane isolation as one value.

Every other lane finding is a **non-blocking nudge**: the operation succeeds and
the response carries `safety_nudges`, each with a stable id, why it matters, and
the exact fix. Read-only dispatch, General-Agent creation, and plain shell tabs
are never obstructed, including in canonical.

`codeterm plan task compete` follows the same rule for parallel writers: each
**mutating** competitor is given its own provisioned worktree, branch, and base
before it spawns, so N competitors never share a checkout and never fall back to
canonical. If provisioning or spawning fails part-way, the lanes already created
in that call are removed. Read-only competition is unchanged.

### Recovering a tab-gone task

When a Worker's tab is gone, its owner epoch is stale by definition — so the
epoch is not allowed to block recovery. The **live authenticated Plan owner**
may `cancel`, `pend`, reassign, or respawn such a task. That recovery retires
the stale attempt in the *same* transition that clears the dead owner identity,
so no half-applied state is observable and a `dispatch_ready` arriving later for
the retired attempt is rejected instead of resurrecting ownership. The widening
is for the Plan owner only: a non-owner, non-epoch caller is still refused.

Archiving a plan with no retrospective **succeeds** and emits the advisory
`plan.retrospective_unrecorded`, recording durable incident evidence on the plan.
Write the retrospective before normal closure
(`codeterm plan retrospective <plan> --body-file <pad-relative-path>`) — but the
mechanism is a nudge, never a gate, so it can never trap recovery or closure.

## The loop

Run this loop after every report, reminder, task close, tab death, or gate:

1. **Handle pushed reports.** Worker reports arrive automatically as incoming
   messages. Accept or bounce them without polling an inbox.
2. **Reconcile.** Run `codeterm plan reconcile <plan>`; fix only owned drift.
3. **Re-derive the frontier.** Inspect all open tasks, not only the previous
   focus.
4. **Give every actionable task an exit:** dispatch, pause with a reason, judge,
   replan through the Planner, or escalate.
5. **Keep the frontier staffed.** Do not leave a capable Worker idle while
   independent actionable work exists. Clear dispatch, dependency, review, and
   cleanup blockers first, then reuse a matching Worker or dispatch the next
   bounded task without duplicating ownership.
6. **Keep source writers parallel.** Implementation Workers use isolated,
   source-only lanes based on concurrency and file ownership.
7. **Own canonical verification.** Real-merge accepted ancestry first, then run
   every build, test, dev check, and gate in the canonical checkout.
8. **Let heartbeat follow state.** Heartbeat is derived automatically from
   plan and task state; never maintain focus or lane fields manually.
9. **Clean accepted work.** Close the Worker tab, remove the ancestry-merged
   worktree and branch, stop its dev process, and record cleanup.

Discovery is an intermediate wave. It must produce decisions and follow-up
implementation/review/verification tasks; research alone does not complete the
goal.

## Planning ownership

When a Planner is present:

- the active Plan belongs to the Team and is visible through Team membership;
- the Orchestrator creates and owns the Plan and may edit its full structure;
- delegate structural refinement to the Planner when useful;
- never create a separate Planner-to-Plan binding or ask the General Agent to bootstrap one;
- run `codeterm plan doctor <plan> --kind quality`, then use one `codeterm plan
  approve <plan>` action to approve the structure and activate its tasks;
- after a green doctor, summarize the proposed plan to the user and explicitly
  ask for approval; never stop silently at a completed draft;
- do not dispatch until approval is durably recorded. After UI approval, verify
  `planning_state=approved`; after explicit conversational approval, run the
  approve action and verify the same state;
- never ask the Planner to self-approve or manually set `structure_ready`;
- draft tasks remain non-dispatchable;
- the Orchestrator remains the sole dispatch and acceptance authority.

Judge structure before dispatch against the task-shape contract: 4-6 cohesive
vertical tasks, each delivering a reviewable capability, with ordinary
discovery, tests, docs, and review inside the slice that implements them.
Standalone discovery, test-author, or review tasks need a concrete independent
boundary — a distinct security or authority owner, or independently owned
per-platform rollout — and every task beyond five needs a justification naming
that boundary. Full rule: `codeterm docs PLANNER`.

If the Planner is intentionally waived, record that explicitly on the plan.

## Task lifecycle

```text
pending → in_progress → in_review → done
              ↘ blocked
pending/in_progress/blocked/in_review → paused
any open state → cancelled
```

- `in_progress` requires both a live runtime binding and the Worker's durable
  `codeterm agent ack`; either may arrive first.
- `dispatch_ready` proves PTY, session, and lane readiness but does not start
  the task. A missing ACK emits `worker_ack_timeout` after the bounded deadline.
- `codeterm agent report --status in_review` atomically records the handoff,
  moves the bound task to `in_review`, and pushes the report into the
  Orchestrator tab. Workers never poll or separately update task status.
- The Orchestrator alone turns `in_review` into `done` or bounces it.
- `paused` requires a reason and is excluded from heartbeat open work.
- Dependencies express sequencing; pause is an operator hold, not a queue.

Never leave an actionable task unstaffed or an `in_review` task unjudged.

## Dispatch contract

Before a wave:

1. Check provider headroom.
2. Define a disjoint lane map: task, files, branch/worktree, expected output,
   focused oracle, and central gates requested.
3. Run plan doctor.
4. Dispatch through `codeterm plan task dispatch`.
5. Wait for pushed `dispatch_ready`, then pushed `worker_ack`; only the ACK
   moves the task to `in_progress`. Never poll.
6. If dispatch fails, alert the user immediately and verify rollback. Never
   blind-retry a write.

Every Worker must be visible under the Orchestrator and carry:

- a clear title and description;
- `CODETERM_PLAN_ID` and `CODETERM_TASK_ID`;
- checkout placement chosen by the Orchestrator;
- a self-contained brief with exact acceptance and test commands;
- `codeterm agent ack` as its first task action;
- instructions to report by running `codeterm agent report`.

## Review capacity

Choose review granularity from backlog and diff size. A few large changes merit
individual review; a growing queue of compatible small changes should share one
review wave and canonical QA group. After real merge, the Orchestrator may fix a
bounded mechanical integration defect directly in canonical. Bounce ownership,
correctness, authority, scope, or design defects instead of grinding tiny edits.

Review backpressure blocks ordinary implementation dispatch. A task that exists
only to clear stranded review debt needs daemon-owned provenance. Task names,
kinds, tags, and executor roles never grant capacity. The live bound
Orchestrator authorizes the task explicitly:

```bash
codeterm plan review authorize-capacity <plan> <task> --class review
codeterm plan review authorize-capacity <plan> <task> --class canonical_gate
```

Generic task updates, pauses, decision gates, and removals cannot move a task
out of `in_review`; use `plan review accept` or `plan review bounce`. Historical
credit leaked by older builds is repaired explicitly and auditably with:

```bash
codeterm plan review reconcile-credit <plan> --reason "<incident evidence>"
```

Both mutations require the live bound Orchestrator session and record an
immutable plan stride.

## Related documents

Verification, integration, heartbeat, and closeout: `codeterm docs ORCHESTRATION-OPERATIONS`.

<!-- revision: 56 -->
