# Orchestration operations

Event-driven coordination, acceptance, integration, cleanup, heartbeat, and closeout.

## Event-driven coordination

Never sleep or poll for Worker readiness, reports, gates, or process startup.
Wait on explicit readiness, session-binding, process-exit, stream-completion, or
report events, with a bounded failure deadline.

Long gates run in a visible canonical tab and send an explicit completion
marker back to the Orchestrator. Continue other frontier work while the gate
runs. Delegation is allowed only to a Worker assigned to canonical with an
explicit list of authorized commands.

One immediate sanity read is allowed after a concrete event. Repeated manual
checks are polling.

## Verification and acceptance

Choose the canonical checkout or an isolated lane by concurrency and file
ownership. Workers use isolated lanes for source changes; cold compiled builds or suites
run only after integration in canonical unless the Orchestrator gives
explicit command-scoped permission to a canonical Worker.

Workers in isolated lanes implement source, author coverage, and report exact
oracle commands without executing project verification. They do not build,
run tests, start dev services, or execute gates, even when a command is focused
or cheap. A Worker in canonical still requires explicit Orchestrator permission
for each authorized command.

A mutating canonical Worker exists only through the explicit dispatch opt-in:

```bash
codeterm plan task dispatch <plan> <task> --execution-intent mutating \
  --canonical-writer --authorized-gate "<exact focused command>"
```

Use it for sequential work that benefits from fast iteration; parallel source
work stays in isolated lanes. The writer holds canonical exclusively: occupancy
refuses a second live canonical attempt, and `plan review accept` refuses lane
tasks until the writer's own verdict. Authorize only focused tests, check and
fmt; broad, release and integration gates stay with you. It commits on the
integration branch and reports its commit ids with a clean tree; the CLI checks
the SHA against canonical for this placement, and any dirty report is refused,
even with `--acknowledge-dirty`; that is a bounce. Accept or bounce it before
merging any lane.

After integrating a lane candidate, run its focused oracle through a canonical
verification Worker. If it fails, bounce the task with the exact command and
failure evidence. Run broad suites only once for the compatible group.

A verification Worker is spawned read-only and owns no task:

```bash
codeterm agent spawn <provider> --role Worker \
  --parent-tab <your tab> --report-to <your tab> \
  --cwd <canonical integration checkout> \
  --plan <plan_id> --execution-intent read_only \
  --authorized-gate "<exact command>" \
  --task-file <brief>
```

`--plan-task` is deliberately absent. The plan's implementation task stays bound
to the Worker whose lane was merged, and one task keeps one owner. The verifier
binds to the plan it was spawned under, reports to its parent, and runs only the
commands declared with `--authorized-gate`, which arrive in its brief. Because it
holds no task it can report against none and transition none, so no verification
run can move implementation state. Repeat `--authorized-gate` per command.

Spawning it is not a way around task binding for source work: a Worker declaring
`--execution-intent mutating` without a task is refused, and a task-bound Worker
declares its gates on the dispatch (`plan task dispatch --authorized-gate`),
where they are also enforced. The Orchestrator running the gate itself in its
own canonical checkout remains the other sanctioned path.

Design the smallest deterministic user-layer oracle before implementation so
unit-green changes cannot silently break the real workflow. Keep that oracle
fast, reusable, and event-driven.

During lane iteration, use source inspection and non-executable static review.
After real merge, run focused checks in canonical, then run the actual user-layer
oracle once at final integration or release verification.

The two tiers are not interchangeable, and the cheap one is mandatory:

- A **focused check** proves one change. Every accepted lane and every repair
  gets one, scoped to what it actually touched — one test module, one type
  check, one file's lint. It is the first executable thing that runs after a
  merge, and it is never skipped on the grounds that a broader gate is coming.
- A **broad or canonical gate** proves a release. It runs on the release SHA,
  once, immediately before cutting. It is not a per-merge step, and it is never
  a repair's first executable check: re-running a fifteen-minute matrix to prove
  a duplicate import or a one-line type fix costs a full slot to learn what the
  focused check answers in seconds.

A peer protocol may serialize a shared machine resource — one Docker lease, one
database, one build slot. It may not forbid the focused checks that keep the
broad gate down to a single run; a protocol that makes the full gate the first
executable check on a merge has inverted the two tiers and turns every trivial
defect into a full run.

Reaching a second broad-gate run with no release between them is the signal that
this has happened. Stop and record why before starting a third.

The final verification covers the layer where the user experienced the defect:

- inspect the actual diff;
- reproduce the failing scenario;
- run focused tests and type/build checks through repository wrappers;
- check real UI, persisted state, transport, or recipient output where relevant;
- review provider, platform, lifecycle, and concurrency blast radius;
- use an independent adversarial reviewer for broad, hot-path, security,
  session-binding, provider-boundary, or final integration changes.

Unit-green alone does not close a user-visible defect. Deferred final
user-layer evidence remains explicit open risk.

Testing ladder — per-task = focused filters only; per-group (milestone) = one full regression gate (cargo suite + ts-unit, serialized in the warm checkout); per-release = the farm matrix across every platform.

Repository-specific restrictions override generic gate suggestions.

## Owner decision holds

Pause a plan only when every task is blocked on an owner decision. Ask once with
`codeterm notify send "<question>" --level action_needed` (repeat `--choice
"<label>"` for options), then wait for the owner's next message in this tab.
Paused plans have no heartbeat or reminders. Resume with
`codeterm plan resume <plan_id>` and continue the frontier.
## Integration and cleanup

Integrate accepted work with a real merge commit. Never cherry-pick, squash,
replay, or patch-copy lane work. Never merge a lane into canonical while a
canonical writer attempt is live; the merge is plain git, so the accept refusal
is the recorded stop.

Before deleting a lane:

```bash
git merge-base --is-ancestor <lane-tip> <integration-tip>
```

At every group close:

- record the group regression run and the group retrospective
  (`codeterm plan groups close <plan> <group> --note "<retrospective>"`); no
  further task dispatches until both exist;
- close completed Worker tabs;
- remove merged worktrees and branches;
- stop lane-specific dev processes;
- record cleanup status;
- prune stale or superseded milestones;
- re-run reconciliation and re-derive the frontier.

More than five worktrees or direct Workers is cleanup pressure. More than ten is
critical: clean inactive or merged lanes before expanding the wave.

## Heartbeat

Heartbeat preserves the control loop across idle time and compaction. The
daemon derives it from plan state, live assignments, gates, and durable
learnings. Orchestrators do not edit heartbeat fields.

A reminder names the actionable task ids and is handled only when plan state
changes: dispatch, verdict, bounce, unblock, replan, escalation, or recorded
decision. Dependency-waiting and machine-paused tasks need nothing.

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

- every task is done or cancelled;
- required group regression runs, group retrospectives, and release gates are recorded;
- every spawned Worker and Planner tab is closed;
- every accepted lane is ancestry-merged and removed;
- lane processes are stopped;
- stale milestones are pruned;
- the final handoff or retrospective is recorded;
- the plan is archived.

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

## Team-bound resources

Every created agent, utility shell, and schedule belongs to its Team. Bind
utility tabs and crons with `--team <team-id>`; helpers join with
`codeterm agent spawn <provider> --team <team-id>` or
`codeterm team adopt-agent <team-id> <tab-id>`. Create a standalone shell or
cron only at the user's request. Team close finalizes members, attempts, lanes,
and crons; workspace close applies this to every Team it contains. A Planner
without spawn authority routes requests, including role, reason, and owned work,
to its Orchestrator.

<!-- revision: 8 -->
