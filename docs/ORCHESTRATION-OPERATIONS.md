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

Heartbeat preserves the control loop across idle time and compaction. The
daemon derives it from plan state, live assignments, gates, and durable
learnings. Orchestrators do not edit heartbeat fields.

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

<!-- revision: 3 -->
