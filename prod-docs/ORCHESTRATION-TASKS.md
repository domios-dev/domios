# Orchestration task lifecycle

State transitions, group closeout, and dispatch mechanics for the Orchestrator.

## Task lifecycle

```text
pending → in_progress → in_review → done
              ↘ blocked (owner or Planner decision)
pending/in_progress/blocked → paused   (system holds only)
any open state → cancelled
```

- `in_progress` requires both a live runtime binding and the Worker's durable
  `codeterm agent ack`; either may arrive first.
- Explicit dispatch or respawn of a blocked task returns it to pending in the
  same mutation that reserves the new attempt, preserving the old block reason
  in a plan stride. Dispatch refuses review-held tasks; respawn can replace a
  review-held attempt through the documented review recovery path.
- `dispatch_ready` proves PTY, session, and lane readiness but does not start
  the task. A missing ACK emits `worker_ack_timeout` after the bounded deadline.
- `codeterm agent report --status in_review` atomically records the handoff,
  moves the bound task to `in_review`, and pushes the report into the
  Orchestrator tab. Workers never poll or separately update task status.
- `codeterm agent report --status blocked` persists the Worker's report and
  evidence as one block proposal notice; it leaves the task status, assignment,
  and lane unchanged. Only the Plan owner or Planner blocks explicitly with
  `codeterm plan task update <plan> <task> --status blocked --status-label
  "<reason; owner trigger>"`. A block label must be supplied with that update.
- `blocked` means only an outside prerequisite can advance the task (an owner
  decision, approval, access). Work sequenced after a release or milestone is
  not blocked: keep it `pending` in a later group (`plan task update --group`).
- The Orchestrator alone accepts, bounces, or holds an `in_review` task. `hold`
  records a reason while preserving its assignment and lane; `bounce` returns it
  to the assigned Worker when its continuation is available. If the Worker is
  gone, bounce reopens the task as pending and preserves the note, attempt history,
  and lane for redispatch.
- `paused` is a system hold, never an agent verb: a decision gate (`gate:<id>:`
  reason) or provider exhaustion with a resumable checkpoint. It is excluded
  from open work and resumes when the gate is answered or the provider recovers.
## Group milestones

Group tasks with `--group` and declare their sequence with `codeterm plan groups
set`. Pending tasks in later groups, including after-release work, stay parked
until their group reaches the frontier. They do not appear in actionable-task
reminders or stale-task drift; the reminder gives one count for parked work.

Regression records and group notes are optional recordkeeping. `codeterm plan
qa next`, `codeterm plan qa override`, and `codeterm plan groups close --note`
remain available, and pending records appear as dispatch advisories. They never
block an otherwise authorized dispatch. Run focused checks for each change and
merged wave; run the broad regression suite once on the release SHA.

A readiness report rejects `--sha` only when it resolves in that report's lane
and differs from the lane's HEAD. Keep a task-declared commit from another
repository as its own SHA.
## Dispatch contract

Before a wave:

1. Check provider headroom; near 95%, reuse live Workers instead of spawning
   cold duplicates.
2. Define a disjoint lane map: task, files, branch/worktree, expected output,
   focused oracle, and central gates requested.
3. Run plan doctor.
4. Default to an authored task and dispatch it through `codeterm plan task dispatch`; use `codeterm agent spawn --role Worker --unplanned --reason "<why>"` only for small, urgent, or exploratory work. Lanes by default,
   `--canonical-writer` only by deliberate choice (operations playbook).
5. Wait for the pushed `worker_ack` described in Task lifecycle.
6. Dispatch failure: immediately send a standalone user alert (impact, affected tasks, cleanup, active fallback); verify rollback. After two same-class failures, stop dispatching; diagnose/escalate the coordination incident via `codeterm notify send ... --level action_needed` and a plan stride on the task. Await owner direction before retrying.
   Never self-implement plan-owned work to bypass failed dispatch; mark the task done only after an acknowledged Worker attempt or owner decision.

Every Worker has a clear Orchestrator title and description,
`CODETERM_PLAN_ID`/`CODETERM_TASK_ID`, its chosen checkout, and a self-contained
brief with exact acceptance and test commands; it ACKs first and reports last.

For work on a paired peer, dispatch with `plan task dispatch <plan> <task>
--mesh <peer-instance-id> --execution-intent mutating`. The origin owns the attempt and
review trail; the peer provisions the lane and binds the Worker to the origin
plan/task/attempt identifiers. Accept the report on the origin before running
`plan task merge-remote-lane <plan> <task> <attempt>` to fetch the signed peer
bundle at the accepted report SHA and merge into the clean dispatch-time branch.

## Plan recipes

- Too big once started: `plan task split <plan> <task> --child "<name>"` (repeat); dispatch each child into its own lane. The parent closes when every child is done or cancelled.
- Order work with `--group` and `--depends-on`, never by pausing tasks; record what shipped with `plan milestone add`.
- Move reviews forward in the same turn: `plan review accept` when the proof holds, `bounce` with the failing check when it does not, `hold` only while a named gate is pending. A task does not sit in `in_review` waiting for a convenient moment.
- `blocked` is the last resort: use it only when the task cannot move until a named owner or outside event acts, and name that owner and trigger. Prefer a bounce, a hold, a dependency or deferring the task.

## Owner decision holds

Pause only when an owner decision blocks all tasks. Ask once with
`codeterm notify send "<question>" --level action_needed` (repeat `--choice
"<label>"` for options), then wait for the owner's next message in this tab.
Paused plans have no heartbeat or reminders. Resume with
`codeterm plan resume <plan_id>` and continue the frontier.

## Provider access loss and archive advisories

A provider access-loss incident sends a `[provider access lost]` system notice to the tab's parent (a roleless spawned child included); a role tab without a parent falls back to its plan's Orchestrator. A top-level Orchestrator escalates through the owner notification. The `provider-access-lost` event identifies the affected tab, provider, bound session, notification and target. The work owner can recover login or respawn the work on another provider; an access refusal never means a completed task.

Archive succeeds without a retrospective and reports the optional `plan.retrospective_unrecorded` advisory; record a durable note at wrap-up with `codeterm plan retrospective <plan> --body-file <pad-relative-path>`.

<!-- revision: 7 -->
