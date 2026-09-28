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

Use `codeterm docs <NAME>` for the full document; bare `codeterm docs` or
`codeterm docs --help` lists names. Read related playbooks when needed:

| Need | Read |
|---|---|
| Planner ownership and task cutting | `codeterm docs PLANNER` |
| Worktrees, integration, cleanup | `codeterm docs LANES` |
| Narrow plan edits | `codeterm docs PLAN-EDITING` |
| Bounce evidence and retry limits | `codeterm docs BOUNCE` |
| Test authoring | `codeterm docs TEST-AUTHORING` |
| Task lifecycle, groups, dispatch | `codeterm docs ORCHESTRATION-TASKS` |
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

The Orchestrator states the GOAL and delegates structure to the Planner, then
approves or revises it, dispatches, judges reports, integrates accepted lanes,
and owns cleanup. A bound Planner keeps section/task authority after approval;
approval and dispatch remain the Orchestrator's.
For oversized in-progress work, the Orchestrator can use `codeterm plan task
split <plan> <task> --child <name>` to add approved child tasks, then dispatch
each child into its own lane. Child status rolls up on the parent, which stays
open until every child is done or cancelled. Workers can propose a split in
their report but cannot spawn sub-Workers.
Directly implement a genuinely small, local, low-risk change already authorized
in canonical when delegation costs more. Worker dispatch remains default for
plan-owned source work; never self-assign normal implementation work merely
because canonical is writable.
Team utility shells and cron jobs inherit that Team for cleanup. Use `--no-team`
only when a resource must outlive it deliberately.
Adopting a plan into a Team binds that Team's unique live Planner to the plan.

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

Discovery must produce decisions and follow-up implementation, review, or
verification tasks; research alone does not complete the goal.

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

Judge structure against your role brief's plan-shape contract before dispatch.
If the Planner is waived, record that on the plan.
## Dispatch contract

Before each wave, check provider headroom (reuse live Workers near 95%), map disjoint task/file/branch/oracle/gate ownership, and run plan doctor. Each Worker needs `CODETERM_PLAN_ID`/`CODETERM_TASK_ID`, its chosen checkout, and a brief with exact acceptance and test commands; it ACKs before work and reports last. Dispatch is local only; do not use mesh dispatch.

If dispatch fails, send a standalone user alert immediately with impact, affected tasks, cleanup, and fallback; verify rollback. After two failures of the same class, stop, diagnose, and await owner direction. Never self-implement plan-owned work to bypass a blocked dispatch.

## Task lifecycle, milestones, and dispatch

The task guide covers lifecycle and dispatch; the operations guide covers review, verification, and closeout.

## Review capacity

Choose review granularity by backlog and diff: review a few large changes
individually; batch compatible small ones in one wave and canonical QA. After
real merge, directly fix bounded mechanical integration defects in canonical;
bounce ownership, correctness, authority, scope, or design defects rather than
grinding tiny edits.

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
"…"`: `bounce` returns work to a live Worker or reopens it as pending when the
Worker is gone, `hold` keeps the review open with the assignment and lane intact,
and every verdict requires `--note`, never `--reason`. Review credit repair,
task-less evidence reporters, and Planner parent resolution are plan surface
contracts (API-PLAN).

<!-- revision: 94 -->
