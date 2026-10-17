# Orchestration control loop

Team utilities: `codeterm tab new --team <team-id> --purpose build|dev|qa|shell` (default `shell`); use dev for servers. Purpose survives restart/restore. Command marks show running, idle with last exit, or unknown without marks.

Use command `--help` for syntax.

## Related documents

Start narrow:

```bash
codeterm orchestrator status --plan <plan> --tab <orchestrator-tab>
codeterm plan reconcile <plan>
codeterm task show <task>
```

Use `codeterm orchestrator inspect --plan <plan> --json` only when compact status cannot explain drift.

Read `codeterm docs <NAME>`; list with `codeterm docs`:

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

The Orchestrator sets GOAL, judges structure, dispatches, reviews, integrates and cleans up. A bound Planner retains section/task authority after approval; approval and dispatch remain the Orchestrator's.
Split oversized work with `codeterm plan task split <plan> <task> --child <name>`;
dispatch each approved child in its own lane. The parent stays open until every child is
done or cancelled. Workers propose splits but never spawn sub-Workers.
Implement authorized small, local, low-risk corrections in canonical when
cheaper than delegation; default plan-owned source work to Worker dispatch.
Canonical write access never justifies self-assigning normal implementation.
Team utilities inherit cleanup; use `--no-team` only to outlive it deliberately.
Plan adoption binds the Team's unique live Planner.

Transfer acknowledged Workers between Teams only through their current parent; see `LANES` for identity preservation and refusals.

Lane safety: two hard stops, otherwise nudges; see `LANES`.

### Recovering a tab-gone task

The live Plan owner may cancel, pend, reassign, respawn or dispatch a tab-gone task, atomically retiring its stale attempt. Proof, allowlists, refusals: `codeterm docs API-PLAN`.

## The loop

After every report, reminder, task close, tab death or gate:

Worker death during `in_progress` (tab closed/crashed or agent exited) pushes `[worker lost: <task>]` with task, lane, dirt and recovery command. Tell the user about the incident, then run the named `codeterm plan task respawn` command.

1. **Judge pushed reports** without polling. Worker `--status blocked` proposes
   a block without changing task state; see `ORCHESTRATION-TASKS`.
2. **Reconcile.** Run `codeterm plan reconcile <plan>`; fix only owned drift.
3. **Re-derive the frontier.** Inspect all open tasks, not only the previous focus.
4. **Give every actionable task an exit** under the task sequencing contract;
   plan reminders name actionable task ids and show later-group work once as a
   compact parked-task count.
5. **Keep the frontier staffed.** Do not leave a capable Worker idle while
   independent actionable work exists. Clear dispatch, dependency, review, and
   cleanup blockers first, then reuse a matching Worker or dispatch the next
   bounded task without duplicating ownership.
6. **Keep source writers parallel** in isolated lanes with disjoint ownership.
7. **Own canonical verification.** Real-merge ancestry; never cherry-pick,
   squash, replay or patch-copy. Accept on review, then rerun each merged
   wave's focused selectors in canonical.
8. **Let heartbeat follow state.** It derives from plan/task state; answer
   reminders there, never through heartbeat, focus or lane fields.
9. **Clean up as you go:** when a task is accepted or cancelled, close its Worker tab, crons and idle utility shells (stop dev processes first),
   prune lanes with `codeterm plan lanes <plan> --clean --confirm`, and record cleanup.

`actionable: false` tab hints are observations, never instructions to answer, notify or mutate. Actionable events require two agreeing strong observations of one launch.

Discovery must lead to decisions and implementation, review or verification; research alone is incomplete.

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

Before dispatch, judge structure against your role brief's plan-shape contract. Record Planner waivers on the plan.
## Dispatch contract

Workers default to full machine access for both execution intents. `--execution-intent` governs task and report authority, independently of OS permissions. Explicitly request confinement with `--sandbox read-only` or `--sandbox workspace-write` on `agent spawn` or local `plan task dispatch`; unsupported mappings are refused. A requested sandbox rejects custom provider arguments and persists its immutable policy across restore. Explicit sandbox selection on mesh task dispatch is refused before dispatch.

Task-less verification reports omit `--status` and `--task`; execution intent still bounds authorized work even with full machine access. A launch explicitly confined to a read-only filesystem can report through UTF-8 stdin with `codeterm agent report --from-pad -`. Existing saved sandbox policies are not widened by the Worker default.

## Worker kinds

Worker kinds are prompt-only: they add role guidance, not machine access or execution ability. Both support assigned mutations with or without a task. Use `agent spawn --role Worker --kind tester|release_manager --parent-tab <tab>`; see `--help` for flags.

- `tester`: browser verification; may accept or bounce only an exact in-review task handed off by its parent, with an evidence note.
- `release_manager`: current-release coordination; publication requires owner authorization.

Bind `--plan-task <task>` with explicit `--execution-intent` for Worker ACK, lane and status lifecycle. Task-less kinds default to mutating intent, work for their parent and report without `--status`; Kinds grant no plan-state authority. Tester review delegation is explicit. Publication and tags require owner GO. Neither kind spawns children.

Before each wave: check provider headroom (reuse live Workers near95%), map disjoint task/file/branch/oracle/gate ownership, run plan doctor. Give Workers `CODETERM_PLAN_ID`/`CODETERM_TASK_ID`, checkout and exact acceptance/test commands; ACK first, report last. On a Full-access paired peer, `codeterm plan task dispatch <plan> <task> --mesh <peer-instance-id> --execution-intent mutating` creates an origin attempt and peer lane. Judge the persisted origin report; merge its exact accepted SHA to the clean dispatch-time branch.

Dispatch failure: send a standalone user alert immediately with impact, affected tasks, cleanup and fallback; verify rollback. After two same-class failures, stop, diagnose and await owner direction. Never bypass blocked dispatch by implementing plan-owned work yourself.

## Review capacity

Review by backlog and diff: large changes individually, compatible small ones
as one wave with canonical QA. After real merge, fix bounded mechanical
integration defects in canonical; bounce ownership, correctness, authority,
scope or design defects.

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

One automatic plan reminder exists: heartbeat, only after its configured interval. Events (reports, dispatches, task/mode changes, worker loss) never add reminders; completed plans get no wrap-up reminder. Reports, worker-lost notices, decisions and failure alerts keep their own channels.

Heartbeat deterministically suppresses unchanged observations and requires a concrete next step; no Supporter or model is required. Worker reminders also respect their interval.

A `[provider access lost]` notice means recover the login or respawn on another provider, never a completed task; routing is in `ORCHESTRATION-TASKS`.

Confined mutating Workers persist host-owned Cargo, lane-check, sccache and pnpm store roots. TMPDIR/TEMP/TMP use lane-local `.codeterm-tmp`. Task-bound spawns require explicit read_only or mutating intent. Older task tabs without saved confinement retain their behavior.

<!-- revision: 127 -->
