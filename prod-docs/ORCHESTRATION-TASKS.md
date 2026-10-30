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
  `domios agent ack`; either may arrive first.
- Explicit dispatch or respawn of a blocked task returns it to pending in the
  same mutation that reserves the new attempt, preserving the old block reason
  in a plan stride. Dispatch refuses review-held tasks; respawn can replace a
  review-held attempt through the documented review recovery path.
- `dispatch_ready` proves PTY, session, and lane readiness but does not start
  the task. A missing ACK emits `worker_ack_timeout` after the bounded deadline.
- `domios agent report --status in_review` atomically records the handoff,
  moves the bound task to `in_review`, and pushes the report into the
  Orchestrator tab. Workers never poll or separately update task status.
- `domios agent report --status blocked` persists the Worker's report and
  evidence as one block proposal notice; it leaves the task status, assignment,
  and lane unchanged. Only the Plan owner or Planner blocks explicitly with
  `domios plan task update <plan> <task> --status blocked --status-label
  "<reason; owner trigger>"`. A block label must be supplied with that update.
- `blocked` means only an outside prerequisite can advance the task (an owner
  decision, approval, access). Work sequenced after a release or milestone is
  not blocked: keep it `pending` in a later group (`plan task update --group`).
- The Orchestrator alone dispatches and holds `in_review` tasks. It may hand one
  in-review task to a direct child QA with `domios plan review delegate`;
  that QA may accept or bounce only the handed task and must attach its
  evidence note. A QA cannot hold, dispatch, or transition any other task.
  `plan task update --status cancelled` closes a review and returns its review credit. A Worker Partial report stays in progress; `plan review bounce` can return its note to that same bound Worker. `hold` preserves assignment and lane; `bounce` resumes the Worker when its
  continuation is available, or reopens a gone Worker task as pending while
  preserving the note, attempt history, and lane for redispatch.
- `paused` is a system hold, never an agent verb: a decision gate (`gate:<id>:`
  reason) or provider exhaustion with a resumable checkpoint. It is excluded
  from open work and resumes when the gate is answered or the provider recovers.
## Group milestones

Group tasks with `--group` and declare their sequence with `domios plan groups
set`. Pending tasks in later groups, including after-release work, stay parked
until their group reaches the frontier. They do not appear in actionable-task
reminders or stale-task drift; the reminder gives one count for parked work.

Regression records and group notes are optional recordkeeping. `domios plan
qa next`, `domios plan qa override`, and `domios plan groups close --note`
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
4. Default to an authored task and dispatch it through `domios plan task dispatch`; use `domios agent spawn --role Worker --unplanned --reason "<why>"` only for small, urgent, or exploratory work. Supply `--tag work:<value>` or let the missing work tag be inferred with a warning. Lanes by default,
   `--canonical-writer` only by deliberate choice (operations playbook). Add
   `--kind qa` or `--kind release_manager` to a canonical dispatch when the
   Worker needs that built-in prompt; it does not change `executor_role` or
   replace parent authorization. Direct task-bound spawn cannot reserve canonical.
5. Wait for the pushed `worker_ack` described in Task lifecycle.
6. Dispatch failure: immediately send a standalone user alert (impact, affected tasks, cleanup, active fallback); verify rollback. After two same-class failures, stop dispatching; diagnose/escalate the coordination incident via `domios notify send ... --level action_needed` and a plan stride on the task. Await owner direction before retrying.
   Never self-implement plan-owned work to bypass failed dispatch; mark the task done only after an acknowledged Worker attempt or owner decision.

Every Worker has a clear Orchestrator title and description. Task dispatch
derives a short description from the task name; pass `--description` to override
it. Direct `agent spawn` still requires `--description`.
`CODETERM_PLAN_ID`/`CODETERM_TASK_ID`, its chosen checkout, and a self-contained
brief with exact acceptance and test commands; it ACKs first and reports last.

Respawn uses the prior attempt's recorded execution intent, provider, and
resolved model and keeps its lane. The recovery command does not require callers
to restate those values; explicit provider/model options remain available for an
intentional change.

For work on a paired peer, dispatch with `plan task dispatch <plan> <task>
--mesh <peer-instance-id> --execution-intent mutating`. The origin owns the attempt and
review trail; the peer provisions the lane and binds the Worker to the origin
plan/task/attempt identifiers. Accept the report on the origin before running
`plan task merge-remote-lane <plan> <task> <attempt>` to fetch the signed peer
bundle at the accepted report SHA and merge into the clean dispatch-time branch.

## Agent checklists

Every agent tab, Orchestrator, Planner, Worker or roleless, keeps its own
checklist in Domios; no parent and no provider todo tool is needed. Write the
whole list as a JSON array to your pad and declare it:

```bash
domios todo write --from-pad todos.json
```

Each entry is `{ "content", "status": "pending|in_progress|completed|blocked|skipped", "activeForm?", "id?", "link?", "reason?" }`.
New items may omit `activeForm`; it defaults to `content`. Invalid JSON fields are reported together with the accepted schema, including on `todo amend`. The first write creates the checklist. Later writes reconcile it: an entry
matches by `id` from `domios todo list`, falling back to exact `content` title matching.
`id` names an existing item only; a new item omits `id`, and an unknown `id` is refused with `todo item not found`. An id entry may omit `content` and `activeForm`; it changes only status/reason,
preserving the title, links, attempts and history. A new
`content` appends an item; an omitted item you added is removed. Items a
dispatch seeded from acceptance criteria, or a QA Worker appended, are locked:
omitting one leaves it unchanged and the reply counts it. At most one item is
`in_progress`, counting omitted locked items, and a refusal names the
offending items. `blocked` and `skipped` need a `reason`. A checklist holds at
most 12 items; a larger list is refused, and the remedy is a task split. A
new item's `link` is an http(s) URL or a task reference; an existing item's
`link` must match its current label, URL or task id.

An `in_review` report warns about remaining open locked criteria without refusing
the report. Review acceptance completes them with reason "accepted by review";
a bounce reopens locked criteria.

The sidebar shows a checklist disclosure on every agent row whose tab holds a
checklist, including rows of paired peers that report one.
`domios todo list` reads your own list; an Orchestrator reads a child's with
`domios tab todo list --tab <child-id>`.

Workers share the checklist with `domios todo submit` and continue work.
The parent agent receives its criteria and plan items together for agent-to-agent
review; its chat shows no human review card or Approve/Edit controls.
`Not reviewed yet` in the sidebar and Underway is informational.
Only the Worker's parent or Plan owner may review through the CLI:

```bash
domios todo approve --tab <child-id> --note-from-pad note.md
domios todo amend --tab <child-id> --from-pad todos.json --note-from-pad note.md
```

The note is optional. Amendment uses the same array as `todo write`; `id` keeps
an existing item and `content` may reword an unlocked item. Omitted own items
are removed; omitted locked criteria stay. Approval clears the unreviewed
marker; amendment updates the checklist and sends the Worker its changes and
note. Sharing, review and checklist reminders never gate work or reporting.

## Plan recipes

- Too big once started: `plan task split <plan> <task> --child "<name>"` (repeat); dispatch each child into its own lane. The parent closes when every child is done or cancelled.
- Order work with `--group` and `--depends-on`, never by pausing tasks; record what shipped with `plan milestone add`.
- Move reviews forward in the same turn: `plan review accept` when the proof holds, `bounce` with the failing check when it does not, `hold` only while a named gate is pending. A task does not sit in `in_review` waiting for a convenient moment.
- `blocked` is the last resort: use it only when the task cannot move until a named owner or outside event acts, and name that owner and trigger. Prefer a bounce, a hold, a dependency or deferring the task.

## Owner decision holds

Pause only when an owner decision blocks all tasks. Ask once with
`domios notify send "<question>" --level action_needed` (repeat `--choice
"<label>"` for options), then wait for the owner's next message in this tab.
Paused plans have no heartbeat or reminders. Resume with
`domios plan resume <plan_id>` and continue the frontier.

## Provider access loss and archive advisories

A provider access-loss incident sends a `[provider access lost]` system notice to the tab's parent (a roleless spawned child included); a role tab without a parent falls back to its plan's Orchestrator. A top-level Orchestrator escalates through the owner notification. The `provider-access-lost` event identifies the affected tab, provider, bound session, notification and target. The work owner can recover login or respawn the work on another provider; an access refusal never means a completed task.

Archive succeeds without a retrospective and reports the optional `plan.retrospective_unrecorded` advisory; record a durable note at wrap-up with `domios plan retrospective <plan> --body-file <pad-relative-path>`.

<!-- revision: 19 -->
