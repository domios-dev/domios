# Orchestration control loop

Create Team utility shells with `codeterm tab new --team <team-id> --purpose build` for builds, `--purpose dev` for servers, and `--purpose qa` for verification. The default purpose is `shell`; restart and desk restore keep it. Shell rows and headers show command-mark evidence: running, idle with the last exit, or unknown without marks.

Use command `--help` for syntax; this is the Domios Orchestrator contract.

## Related documents

Start narrow:

```bash
codeterm orchestrator status --plan <plan> --tab <orchestrator-tab>
codeterm plan reconcile <plan>
codeterm task show <task>
```

Use `codeterm orchestrator inspect --plan <plan> --json` only for discrepancies compact status cannot explain.

`codeterm docs <NAME>` reads a full playbook; bare `codeterm docs` or
`codeterm docs --help` lists names:

| Need | Read |
|---|---|
| Planner ownership and task cutting | `codeterm docs PLANNER` |
| Worktrees, integration, cleanup | `codeterm docs LANES` |
| Narrow plan edits | `codeterm docs PLAN-EDITING` |
| Bounce evidence and retry limits | `codeterm docs BOUNCE` |
| Test authoring | `codeterm docs TEST-AUTHORING` |
| Task lifecycle, groups, dispatch | `codeterm docs ORCHESTRATION-TASKS` |
| Verification, integration, heartbeat, closeout | `codeterm docs ORCHESTRATION-OPERATIONS` |

## Durable model

Domios planner is the source of truth:

```text
GOAL → Spec (WHAT/WHY) ⊥ Plan (HOW) → Tasks → Strides
```

- The Orchestrator sets the GOAL first, then delegates the rest of the structure.
- Requirements, constraints, acceptance, and vocabulary belong in Spec.
- Phases, ownership, sequencing, and verification belong in Plan.
- Tasks carry bounded scope, ownership, acceptance criteria, correctness, and an executable test plan.
- Strides record evidence, decisions, reports, and working points.
- Do not replace planner state with a standalone markdown implementation plan.

The Orchestrator sets the GOAL, judges the Planner's structure, dispatches,
reviews, integrates and cleans up. A bound Planner keeps section/task authority
after approval; approval and dispatch remain the Orchestrator's.
Split oversized work with `codeterm plan task split <plan> <task> --child <name>`;
dispatch each approved child in its own lane. The parent stays open until every child is
done or cancelled. Workers propose splits but never spawn sub-Workers.
Implement authorized small, local, low-risk corrections in canonical when
cheaper than delegation; default plan-owned source work to Worker dispatch.
Canonical write access never justifies self-assigning normal implementation.
Team utilities inherit cleanup; use `--no-team` only to outlive it deliberately.
Plan adoption binds the Team's unique live Planner.

Transfer an acknowledged Worker between Teams only through its current parent;
The LANES document linked above covers preserved identity and transfer refusals.

Lane safety: two hard stops, otherwise nudges; see `LANES`.

### Recovering a tab-gone task

The live Plan owner may cancel, pend, reassign, respawn, or dispatch a task whose
Worker tab is gone; the recovery retires the stale attempt in the same transition.
Ownership proof, verb allowlists, and refusals: `codeterm docs API-PLAN`.

## The loop

Run this loop after every report, reminder, task close, tab death, or gate:

A Worker that dies while its task is `in_progress` (tab closed, crashed, or its
agent exited) is pushed to you at once as a `[worker lost: <task>]` notice naming
the task, lane, dirty state, and recovery command. Treat it as an incident: tell
the user, then recover with the named `codeterm plan task respawn` command.

1. **Handle pushed reports.** Worker reports arrive automatically as incoming
   messages. Accept or bounce them without polling an inbox. A Worker
   `--status blocked` report is a block proposal and leaves task state
   unchanged; deliberate blocks are in `ORCHESTRATION-TASKS`.
2. **Reconcile.** Run `codeterm plan reconcile <plan>`; fix only owned drift.
3. **Re-derive the frontier.** Inspect all open tasks, not only the previous focus.
4. **Give every actionable task an exit** under the task sequencing contract;
   plan reminders name actionable task ids and show later-group work once as a
   compact parked-task count.
5. **Keep the frontier staffed.** Do not leave a capable Worker idle while
   independent actionable work exists. Clear dispatch, dependency, review, and
   cleanup blockers first, then reuse a matching Worker or dispatch the next
   bounded task without duplicating ownership.
6. **Keep source writers parallel.** Prefer isolated lanes with disjoint
   ownership: diffs stay reviewable and canonical stays warm.
7. **Own canonical verification.** Real-merge ancestry; never cherry-pick,
   squash, replay or patch-copy. Accept on review, then rerun each merged
   wave's focused selectors in canonical.
8. **Let heartbeat follow state.** It derives from plan/task state; answer
   reminders there, never through heartbeat, focus or lane fields.
9. **Clean up as you go:** when a task is accepted or cancelled, close its Worker tab, crons and idle utility shells (stop dev processes first),
   prune lanes with `codeterm plan lanes <plan> --clean --confirm`, and record cleanup.

Tab-state hints marked `actionable: false` are observations, never instructions
to answer, notify or mutate state. An actionable event needs two agreeing strong
observations of the same launch.

Discovery must lead to decisions and implementation, review or verification;
research alone does not complete the goal.

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

`--execution-intent read_only` requires provider-declared filesystem enforcement; an unsupported provider is refused with its capability gap. Scoped launches override the global full-access preference and reject custom launch arguments. The intent and allowed paths survive restart and restore; runtime controls cannot broaden them. Mutating workers use their lane plus explicit host-owned pad and Git metadata roots.

A read-only Worker reports through UTF-8 stdin with `codeterm agent report --from-pad -`, since its pad is read-only too; task-less verification reports omit `--status` and `--task`. Authorized gate commands do not grant filesystem writes. A gate that needs writes outside the provider sandbox is an explicit owner-run canonical gate; the Worker reports the refusal instead of requesting broader access.

## Task-less Worker kinds

`tester` and `release_manager` are task-less Worker kinds, not Managers. Choose by the work:

| Kind | Use | Spawn |
|---|---|---|
| `tester` | Task-less, read-only browser verification; may accept or bounce only an exact in-review task handed off by its parent, with an evidence note. | `codeterm agent spawn <provider> --role Worker --kind tester --parent-tab <tab> [--authorized-gate <command>]` |
| `release_manager` | Task-less, read-only coordination for the current release; publication remains owner-authorized. | `codeterm agent spawn <provider> --role Worker --kind release_manager --parent-tab <tab> [--authorized-gate <command>]` |

Both kinds own no task, spawn no children, and report to the parent without `--status`. Their role prompts define their work and evidence rules. A gate grant authorizes only the exact recorded commands and does not grant source-edit access.

Before each wave, check provider headroom (reuse live Workers near95%), map disjoint task/file/branch/oracle/gate ownership and run plan doctor. Give each Worker `CODETERM_PLAN_ID`/`CODETERM_TASK_ID`, its checkout and exact acceptance/test commands; it ACKs first and reports last. On a Full-access paired peer, `codeterm plan task dispatch <plan> <task> --mesh <peer-instance-id> --execution-intent mutating` creates an origin attempt and peer lane. Judge the persisted origin report, then merge its exact accepted SHA to the clean dispatch-time branch.

If dispatch fails, send a standalone user alert immediately with impact, affected tasks, cleanup, and fallback; verify rollback. After two failures of the same class, stop, diagnose, and await owner direction. Never self-implement plan-owned work to bypass a blocked dispatch.

## Review capacity

Choose review granularity by backlog and diff: review a few large changes
individually; batch compatible small ones in one wave and canonical QA. After
real merge, directly fix bounded mechanical integration defects in canonical;
bounce ownership, correctness, authority, scope, or design defects rather than
grinding tiny edits.

At five reviews, prioritize closures and avoid backlog growth. Backlog and credit
debt are advisory: dispatch, competition and respawn need no capacity grant.
Authorization, dependencies, review safety, and task acceptance still apply;
pending terminal-group evidence remains advisory. Never fake acceptance or
adjust credit to silence warnings.

Owner-only `codeterm plan review authorize-capacity <plan> <task>` with
`--class review` or `--class canonical_gate` records review-work classification;
dispatch never needs it and it grants no authority.

Exit `in_review` only via:
`codeterm plan review accept <plan> <task> --note-from-pad review.md` (also `bounce`/`hold`).
Bounce resumes Workers or returns pending; hold keeps review, assignment and lane.
Pad notes or scalar `--note`; no authored inline.
`API-PLAN` covers credit repair, task-less evidence and Planner parent resolution.

### Quiet team heartbeat

The Orchestrator has one automatic plan reminder: the periodic heartbeat, eligible only after its configured interval. Plan events, reports, dispatches, task changes, mode changes and worker loss never send an extra reminder. Completed plans produce no automatic wrap-up reminder. Reports, worker-lost notices, decisions and failure alerts retain their own delivery channels.

At a scheduled heartbeat, a deterministic rule suppresses unchanged observations and requires a concrete next step. Worker reminders also respect their configured interval. No Supporter connection or model decision is required.

A `[provider access lost]` notice means recover the login or respawn on another provider, never a completed task; routing is in `ORCHESTRATION-TASKS`.

Confined mutating Workers receive persisted host-owned Cargo, lane-check, sccache and pnpm store roots. TMPDIR, TEMP and TMP select the lane-local `.codeterm-tmp` directory. Task-bound Worker spawns require an explicit read_only or mutating intent. Restoring an older task tab without a saved confinement policy retains its existing behavior.

<!-- revision: 125 -->
