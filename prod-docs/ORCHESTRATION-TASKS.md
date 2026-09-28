# Orchestration task lifecycle

State transitions, group closeout, and dispatch mechanics for the Orchestrator.

## Task lifecycle

```text
pending → in_progress → in_review → done
              ↘ blocked
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
- The Orchestrator alone accepts, bounces, or holds an `in_review` task. `hold`
  records a reason while preserving its assignment and lane; `bounce` returns it
  to the assigned Worker when its continuation is available. If the Worker is
  gone, bounce reopens the task as pending and preserves the note, attempt history,
  and lane for redispatch.
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
4. Default to an authored task and dispatch it through `codeterm plan task dispatch`; use `codeterm agent spawn --role Worker --unplanned --reason "<why>"` only for small, urgent, or exploratory work. Lanes by default,
   `--canonical-writer` only by deliberate choice (operations playbook).
5. Wait for the pushed `worker_ack` described in Task lifecycle.
6. Dispatch failure: immediately send a standalone user alert (impact, affected tasks, cleanup, active fallback); verify rollback. After two same-class failures, stop dispatching; diagnose/escalate the coordination incident via `codeterm notify send ... --level action_needed` and a plan stride on the task. Await owner direction before retrying.
   Never self-implement plan-owned work to bypass failed dispatch; mark the task done only after an acknowledged Worker attempt or owner decision.

Every Worker has a clear Orchestrator title and description,
`CODETERM_PLAN_ID`/`CODETERM_TASK_ID`, its chosen checkout, and a self-contained
brief with exact acceptance and test commands; it ACKs first and reports last.

`plan task dispatch` is local-only. A Worker spawned with
`agent spawn --mesh PEER` has no origin-plan attempt or plan/task env; report with
`--plan <origin-plan> --task <task>`, or the Orchestrator attaches the arriving
message by hand. Cross-mesh dispatch: out of scope; see the spawn-then-bind
pattern in `codeterm docs MESH`.

## Owner decision holds

Pause only when an owner decision blocks all tasks. Ask once with
`codeterm notify send "<question>" --level action_needed` (repeat `--choice
"<label>"` for options), then wait for the owner's next message in this tab.
Paused plans have no heartbeat or reminders. Resume with
`codeterm plan resume <plan_id>` and continue the frontier.

<!-- revision: 1 -->
