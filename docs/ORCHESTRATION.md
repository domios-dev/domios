# Orchestration control loop

This document is the compact operating contract for a CodeTerm Orchestrator.
Use command `--help` for syntax instead of loading a command catalogue.

## Read depth

Start narrow:

```bash
codeterm orchestrator status --plan <plan> --tab <orchestrator-tab>
codeterm plan reconcile <plan>
codeterm task show <task>
```

Use `codeterm orchestrator inspect --plan <plan> --json` only to investigate a
specific discrepancy that compact status cannot explain.

Load specialized procedures only when needed:

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

## The loop

Run this loop after every report, reminder, task close, tab death, or gate:

1. **Handle pushed reports.** Worker reports arrive automatically as incoming
   messages. Accept or bounce them without polling an inbox.
2. **Reconcile.** Run `codeterm plan reconcile <plan>`; fix only owned drift.
3. **Re-derive the frontier.** Inspect all open tasks, not only the previous
   focus.
4. **Give every actionable task an exit:** dispatch, pause with a reason, judge,
   replan through the Planner, or escalate.
5. **Keep writers parallel.** Give disjoint implementation tasks separate
   Worker tabs, branches, worktrees, and file ownership.
6. **Serialize heavy gates.** Run broad QA once in the warm canonical checkout,
   not independently in cold lanes.
7. **Update heartbeat.** `current_focus` names the next action owned by the
   Orchestrator, never “wait for Worker.”
8. **Clean accepted work.** Close the Worker tab, remove the ancestry-merged
   worktree and branch, stop its dev process, and record cleanup.

Discovery is an intermediate wave. It must produce decisions and follow-up
implementation/review/verification tasks; research alone does not complete the
goal.

## Planning ownership

When a Planner is bound:

- send new requirements, evidence, and structural corrections to the Planner;
- do not silently add GOAL, Spec, Plan sections, or execution tasks yourself;
- run `codeterm plan doctor <plan> --kind quality` before approval and dispatch;
- execution tasks require an approved plan;
- the Orchestrator remains the sole dispatch and acceptance authority.

If the Planner is intentionally waived, record that explicitly on the plan.

## Task lifecycle

```text
pending → in_progress → in_review → done
              ↘ blocked
pending/in_progress/blocked/in_review → paused
any open state → cancelled
```

- `in_progress` requires a live bound Worker tab.
- Dispatch or `task take` atomically binds the tab and starts a pending task.
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
5. Treat success as real only after the command returns verified Worker
   readiness, session binding, task id, branch, and worktree provenance.
6. If dispatch fails, alert the user immediately and verify rollback. Never
   blind-retry a write.

Every Worker must be visible under the Orchestrator and carry:

- a clear title and description;
- `CODETERM_PLAN_ID` and `CODETERM_TASK_ID`;
- an isolated `lane/*` branch and sibling worktree;
- a self-contained brief with exact acceptance and test commands;
- instructions to report by running `codeterm agent report`.

Use `codeterm docs LANES` for the complete lane lifecycle.

## Review capacity

Review backpressure blocks ordinary implementation dispatch. A task that exists
only to clear stranded review debt must be classified durably: use task kind
`review`, tag `review-capacity`, or tag a canonical QA task `canonical-gate`.
Names and executor roles never grant capacity.

Generic task updates, pauses, decision gates, and removals cannot move a task
out of `in_review`; use `plan review accept` or `plan review bounce`. Historical
credit leaked by older builds is repaired explicitly and auditably with:

```bash
codeterm plan review reconcile-credit <plan> --reason "<incident evidence>"
```

## Event-driven coordination

Never sleep or poll for Worker readiness, reports, gates, or process startup.
Wait on explicit readiness, session-binding, process-exit, stream-completion, or
report events, with a bounded failure deadline.

Long gates run in a visible tab or delegated QA lane and send an explicit
completion marker back to the Orchestrator. Continue other frontier work while
the gate runs.

One immediate sanity read is allowed after a concrete event. Repeated manual
checks are polling.

## Verification and acceptance

Workers run the smallest meaningful focused oracle. They do not run broad
builds or full suites in every worktree.

The Orchestrator verifies at the layer where the user experienced the defect:

- inspect the actual diff;
- reproduce the failing scenario;
- run focused tests and type/build checks through repository wrappers;
- check real UI, persisted state, transport, or recipient output where relevant;
- review provider, platform, lifecycle, and concurrency blast radius;
- use an independent adversarial reviewer for broad, hot-path, security,
  session-binding, provider-boundary, or final integration changes.

Unit-green alone is not acceptance. Deferred critical evidence remains explicit
open risk.

Testing tiers:

- **Per task:** focused lane oracle.
- **Per group:** one serialized regression gate in the warm canonical checkout.
- **Per release:** platform build/test matrix through the release workflow.

Repository-specific restrictions override generic gate suggestions.

## Integration and cleanup

Integrate accepted work with a real merge commit. Never cherry-pick, squash,
replay, or patch-copy lane work.

Before deleting a lane:

```bash
git merge-base --is-ancestor <lane-tip> <integration-tip>
```

At every group close:

- close completed Worker tabs;
- remove merged worktrees and branches;
- stop lane-specific dev processes;
- record cleanup status;
- prune stale or superseded milestones;
- re-run reconciliation and re-derive the frontier.

More than five worktrees or direct Workers is cleanup pressure. More than ten is
critical: clean inactive or merged lanes before expanding the wave.

## Heartbeat

Heartbeat preserves the control loop across idle time and compaction.

- `current_focus` is the next Orchestrator-owned action.
- `active_lanes` lists only live work.
- `rules` stores temporary execution constraints.
- `learnings` stores durable orchestration lessons.

A reminder is handled only when plan state changes: dispatch, pause, verdict,
bounce, replan, escalation, or recorded decision.

Do not create self-send polling reminders. The plan already wakes the
Orchestrator for open work and Worker reports.

## Handoffs and plan skills

Before compaction, transfer, or a long interruption, create a plan handoff with:

- goal and current position;
- branch/worktree map;
- integrated changes and exact gates;
- open risks;
- ordered next actions.

Read it back and verify completeness, accuracy, and executability before reseed.

Store reusable effort-local procedures as plan skills:

```bash
codeterm plan skill list
codeterm plan skill show <slug>
codeterm plan skill new <slug>
codeterm plan skill edit <slug>
```

Keep the oracle immutable and record keep/discard/supersede honestly. Promote a
generally reusable recipe only during retrospective.

## Closeout

An effort is complete only when:

- every task is done, cancelled, or paused with an explicit reason;
- required group/release gates are recorded;
- every spawned Worker and Planner tab is closed;
- every accepted lane is ancestry-merged and removed;
- lane processes are stopped;
- stale milestones are pruned;
- the final handoff or retrospective is recorded;
- the plan is archived.
<!-- revision: 39 -->
