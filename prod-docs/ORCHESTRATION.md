# Orchestration control loop

Use command `--help` for syntax.

## Related documents

Start narrow:

```bash
domios orchestrator status --plan <plan> --tab <orchestrator-tab>
domios plan reconcile <plan>
domios task show <task>
```

Use `domios orchestrator inspect --plan <plan> --json` only when compact status cannot explain drift.

Read playbooks with `domios docs <NAME>`; list with `domios docs`:

| Need | Read |
|---|---|
| Planner/tasks | `PLANNER` |
| Worktrees/integration | `LANES` |
| Narrow edits | `PLAN-EDITING` |
| Bounce/retry | `BOUNCE` |
| Test authoring | `TEST-AUTHORING` |
| Task lifecycle/dispatch | `ORCHESTRATION-TASKS` |
| Verification/heartbeat/closeout | `ORCHESTRATION-OPERATIONS` |

## Durable model

The planner is the source of truth:

```text
GOAL → Spec (WHAT/WHY) ⊥ Plan (HOW) → Tasks → Strides
```

The Team owns and exposes its active Plan through Team membership; its bound Planner shapes it and keeps section/task authority after approval. The Orchestrator sets the GOAL first, judges structure, and owns approval, dispatch, review, integration, and cleanup.

- Spec owns requirements, constraints, acceptance, and vocabulary; Plan owns phases, owners, sequence, and verification.
- Tasks carry bounded scope/ownership, acceptance/correctness, and executable tests. Strides record evidence, decisions, reports, and working points.
- Never replace planner state with a standalone markdown implementation plan.

Split oversized tasks with `domios plan task split <plan> <task> --child <name>`; dispatch children in lanes and keep the parent open until all are done or cancelled. Workers may propose splits but never spawn; see `ORCHESTRATION-TASKS`.
Use canonical for authorized, small, local, low-risk fixes when cheaper than delegation; dispatch plan-owned source work by default. Canonical access never permits self-assignment.
Team utilities inherit cleanup; use `--no-team` only to outlive it deliberately. Plan adoption binds the Team's unique live Planner.

Only current parents transfer acknowledged Workers between Teams; `LANES` covers identity, refusals, two hard stops, and advisory nudges.

### Recovering a tab-gone task

Only the live Plan owner may cancel, pend, reassign, respawn, or dispatch a tab-gone task; `API-PLAN` covers proof, allowlists, stale-attempt retirement, and refusals.

## The loop

After every report, reminder, task close, tab death, or gate, reconcile with `domios plan reconcile <plan>`, fix only owned drift, and inspect all open tasks. Give each actionable task an exit; task sequencing governs staffing and parked work. See `ORCHESTRATION-TASKS` for reports and `ORCHESTRATION-OPERATIONS` for canonical verification and heartbeat.

Worker death during `in_progress` (tab closed/crashed or agent exited) pushes `[worker lost: <task>]` with task, lane, dirt, and recovery. Tell the user and run its named `domios plan task respawn` command.

Use disjoint isolated lanes for parallel writers. After review and real merge, rerun the wave's selectors in canonical; never cherry-pick, squash, replay, or patch-copy. Heartbeat derives from Plan state; answer reminders there, never by editing heartbeat, focus, or lane fields.

Close a reported Worker after its clean lane is integrated and no author correction remains, even during parent QA. Deferrals and hypothetical defects do not justify retaining it. Preserve review, evidence, and recoverable identity; restore through the supported lifecycle if needed. Keep active/unreported work and fixtures. Close idle utilities/watchers, stop dev first, and prune only clean ancestry-verified lanes with `domios plan lanes <plan> --clean --confirm`; record cleanup. Closing a tab never accepts its task.

Accepted clean merged lanes are removed completely when their Worker closes, including ignored build output; merged branches are deleted with `git branch -d`. Cancelled clean worktrees may be removed while unmerged branches remain recoverable. Dirty or unmerged accepted lanes remain cleanup debt with an exact command. Plan cleanup also previews unregistered artifact residue beneath the worktree root and removes it only with confirmation. Registered worktrees, source and link targets stay protected; deletion failures name the exact path and available holder evidence.

Plan cleanup previews use local Git inventory and answer independently of a running deletion. Confirmed cleanup prints each lane as it is checked and completed; a second confirmed cleanup for that plan is refused while the first runs. JSON output remains one final result.

Only the live Plan Orchestrator may run confirmed lane cleanup (including `--fetch`) from its Team utility shell. The daemon verifies launch binding, creator, Team, and live Plan ownership; shells need no agent session or rebind. Other members cannot clean lanes.

Never answer, notify, or mutate from `actionable: false` tab hints; require two agreeing strong observations of the same launch before acting.

Discovery must lead to decisions and implementation, review or verification; research alone is incomplete.

## Planning ownership

Duplicate Worker guidance in `plan doctor` is advisory. Summarize a green draft and ask the owner; after UI approval verify `planning_state=approved`, after conversational approval run `domios plan approve <plan>` once and verify it. Record Planner waivers.
## Dispatch contract

Workers default to full machine access; `--execution-intent` governs task and report authority, not OS permissions. Confine one with `--sandbox read-only|workspace-write` on `agent spawn` or local `plan task dispatch`. A caller with a recorded sandbox, or the owner's `agent.sandbox_ceiling`, caps delegation: a request above that ceiling, including an omitted `--sandbox`, is refused. Task-less verification reports omit `--status` and `--task`.

## Worker kinds

Kinds add prompt guidance, not machine access or execution ability. `reviewer` is read-only code review by default; use it for independent review of lane diffs. `qa` and `release_manager` support assigned mutations or task-less use; spawn task-less kinds with `domios agent spawn <provider> --role Worker --kind <kind> --parent-tab <tab> [--authorized-gate <command>]`. Task-less instances own no task, default to mutating intent, report to their parent without `--task`/`--status`, and gain no Plan authority. Neither kind spawns children. QA provides browser verification and may accept or bounce only an exact parent-delegated in-review task with evidence; it cannot hold or dispatch. Release managers coordinate current releases; publication and tags require owner GO. See `AGENT-CONTROL`.

Task-bound Workers need `--plan-task <task>` and explicit `--execution-intent` for ACK, lane, and status. Gates authorize only recorded commands, never source edits; task-bound gates are declared on dispatch, where enforced. QA review delegation is explicit.

Task-bound canonical mutation goes only through dispatch with `--execution-intent mutating --canonical-writer` (add `--kind qa|release_manager` without changing `executor_role`). See `API-PLAN` and `LANES`.

Before each wave, follow `ORCHESTRATION-TASKS` for headroom, disjoint ownership, Plan doctor, binding, and ACK; give exact acceptance/test commands and require ACK first, report last. For Full-access paired-peer dispatch, judge the origin report and merge its accepted SHA into the clean dispatch-time branch.

On dispatch failure, send a standalone user alert immediately with impact, affected tasks, cleanup, and fallback; verify rollback. After two same-class failures, stop, diagnose, and await owner direction. Never self-implement plan-owned work to bypass it; see `ORCHESTRATION-TASKS` for escalation.

## Review capacity

Review by backlog and diff: large changes individually, compatible small ones in one wave and canonical QA. After real merge, directly fix bounded mechanical defects; bounce ownership, correctness, authority, scope, or design defects.

At five reviews, prioritize closure and avoid backlog growth. Backlog, credit, and pending terminal-group evidence are advisory; dispatch, competition, and respawn need no capacity grant. Authority, dependencies, review safety, and acceptance still apply. Never fake acceptance or adjust credit.

Bounce resumes the Worker or reopens its task; hold keeps review, assignment and lane. See `API-PLAN` for credit repair, task-less evidence and Planner parent resolution.

### Quiet team heartbeat

The default Curator control loop replaces the Orchestrator heartbeat. It mirrors the Orchestrator's model, starts from a bounded activity digest, and chooses its next round within 45 minutes to two hours. Quiet windows use a free heartbeat nudge only when work needs attention. Paused plans, completed plans and Teams waiting on a human do not launch rounds. Owners can select heartbeat mode and its interval; failures expose a visible heartbeat fallback. Worker reminders keep their own interval. See `ORCHESTRATION-OPERATIONS`.

A `[provider access lost]` notice means recover the login or respawn on another provider, never a completed task; routing is in `ORCHESTRATION-TASKS`.

<!-- revision: 142 -->
