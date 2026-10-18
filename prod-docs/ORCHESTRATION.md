# Orchestration control loop

Create Team utilities with `codeterm tab new --team <team-id>`: `--purpose build` for builds, `--purpose dev` for servers, `--purpose qa` for verification; default `shell`. Purpose survives restart/restore. Rows and headers show command marks: running, idle with last exit, or unknown without marks.

Use command `--help` for syntax.

## Related documents

Start narrow:

```bash
codeterm orchestrator status --plan <plan> --tab <orchestrator-tab>
codeterm plan reconcile <plan>
codeterm task show <task>
```

Use `codeterm orchestrator inspect --plan <plan> --json` only when compact status cannot explain drift.

Read playbooks with `codeterm docs <NAME>`; list with `codeterm docs` or `codeterm docs --help`:

| Need | Read |
|---|---|
| Planner/tasks | `PLANNER` |
| Worktrees/integration | `LANES` |
| Narrow plan edits | `PLAN-EDITING` |
| Bounce/retry limits | `BOUNCE` |
| Test authoring | `TEST-AUTHORING` |
| Task lifecycle/dispatch | `ORCHESTRATION-TASKS` |
| Verification/heartbeat/closeout | `ORCHESTRATION-OPERATIONS` |

## Durable model

Domios planner is the source of truth:

```text
GOAL → Spec (WHAT/WHY) ⊥ Plan (HOW) → Tasks → Strides
```

The Team owns and exposes its active Plan; its bound Planner shapes it and keeps section/task authority after approval.

- Spec owns requirements, constraints, acceptance, and vocabulary; Plan owns phases, owners, sequence, and verification.
- Tasks carry bounded scope, ownership, acceptance/correctness criteria, and executable tests. Strides record evidence, decisions, reports, and working points.
- Never replace planner state with a standalone markdown implementation plan.

Workers may propose splits but never spawn children; `ORCHESTRATION-TASKS` covers splitting and parent closeout.
Use canonical for authorized, small, local, low-risk corrections when cheaper than delegation; dispatch plan-owned source work by default. Canonical access never permits self-assignment.
Team utilities inherit cleanup; use `--no-team` only to outlive it deliberately.
Plan adoption binds the Team's unique live Planner.

Transfer acknowledged Workers only through their current parent; see `LANES` for identity, refusals, and lane safety.

### Recovering a tab-gone task

The live Plan owner alone handles tab-gone recovery and stale-attempt retirement; see `API-PLAN`.

## The loop

Run this loop after every report, reminder, task close, tab death, or gate:

When an `in_progress` Worker dies (tab closes/crashes or agent exits), `[worker lost: <task>]` gives task, lane, dirt, and recovery. Tell the user and run its named `codeterm plan task respawn` command.

1. **Handle pushed reports.** Accept or bounce incoming Worker reports without
   polling. `--status blocked` proposes a block; see `ORCHESTRATION-TASKS`.
2. **Keep source writers parallel.** Prefer isolated lanes with disjoint
   ownership: diffs stay reviewable and canonical stays warm.
3. **Clean up as you go:** when a task is accepted or cancelled, close its Worker tab, crons and idle utility shells (stop dev processes first),
   prune lanes with `codeterm plan lanes <plan> --clean --confirm`, and record cleanup.

Run `codeterm plan reconcile <plan>` after each event; fix only owned drift and inspect all open tasks to rederive actionable exits. The task-sequencing contract governs staffing and ownership; see `ORCHESTRATION-OPERATIONS` for verification and heartbeat.

Never answer, notify, or mutate from `actionable: false` tab hints; require two agreeing strong observations of one launch before acting.

Discovery must lead to decisions and implementation, review or verification; research alone is incomplete.

## Planning ownership

When a Planner is present:

Run `codeterm plan doctor <plan> --kind quality`; duplicate Worker guidance is advisory. Summarize a green draft and ask the owner. After UI approval, verify `planning_state=approved`; after conversational approval, run `codeterm plan approve <plan>` and verify it. Never let the Planner self-approve or set `structure_ready`; drafts stay non-dispatchable. Judge the role brief's plan-shape contract and record Planner waivers. The Orchestrator alone dispatches and accepts.

## Dispatch contract

`--execution-intent read_only` requires provider-declared filesystem enforcement; refusal names capability gaps. Scoped launches override global full access and reject custom launch arguments. Intent and allowed paths survive restart/restore and cannot be broadened at runtime. Mutating Workers use lanes plus explicit host-owned pad/Git roots.

Read-only Workers report via UTF-8 stdin: `codeterm agent report --from-pad -` (pads are read-only). Task-less reports omit `--status` and `--task`. Gates grant no writes; sandbox-external writes require an explicit owner-run canonical gate. Report refusals; never request broader access.

## Task-less Worker kinds

`tester` and `release_manager` are read-only, task-less Worker kinds, not Managers. Spawn with `codeterm agent spawn <provider> --role Worker --kind <kind> --parent-tab <tab> [--authorized-gate <command>]`.

- `tester`: browser verification; may accept or bounce only an exact in-review task handed off by its parent, with an evidence note.
- `release_manager`: current-release coordination; publication requires owner authorization.

Neither owns tasks or spawns children; both report to their parent without `--status`. Their role prompts define work and evidence rules. Gates authorize only exact recorded commands, never source edits.

Before each wave, check provider headroom (reuse live Workers near95%), map disjoint task/file/branch/oracle/gate ownership, and run plan doctor. Give Workers `CODETERM_PLAN_ID`/`CODETERM_TASK_ID`, checkout and exact acceptance/test commands; ACK first, report last. On a Full-access paired peer, `codeterm plan task dispatch <plan> <task> --mesh <peer-instance-id> --execution-intent mutating` creates an origin attempt and peer lane. Judge the persisted origin report; merge its exact accepted SHA to the clean dispatch-time branch.

Dispatch failure: send a standalone user alert immediately with impact, affected tasks, cleanup and fallback; verify rollback. After two same-class failures, stop, diagnose and await owner direction. Never bypass blocked dispatch by implementing plan-owned work yourself.

## Review capacity

Choose review granularity by backlog and diff: review a few large changes
individually; batch compatible small ones in one wave and canonical QA. After
real merge, directly fix bounded mechanical integration defects in canonical;
bounce ownership, correctness, authority, scope, or design defects rather than
grinding tiny edits.

At five reviews, prioritize closure and avoid backlog growth. Backlog, credit, and pending terminal-group
evidence are advisory; dispatch, competition, and respawn need no grant. Authority,
dependencies, review safety, and acceptance still govern. Never fake acceptance or
adjust credit.

Only the owner may run `codeterm plan review authorize-capacity <plan> <task>`
with `--class review` or `--class canonical_gate`; it records classification, not authority or a dispatch requirement.

Close `in_review` with `codeterm plan review accept`, `bounce`, or `hold`, passing
`<plan> <task>` and `--note-from-pad <file>` or `--note`; author notes in the pad.
Bounce resumes the Worker when available or reopens its task; hold keeps review,
assignment, and lane. See
`API-PLAN` for credit repair, task-less evidence, and Planner parent resolution.

### Quiet team heartbeat

Automatic reminders are deterministic for Owners and Workers: unchanged or healthy unchanged snapshots stay quiet. A changed snapshot wakes only for a concrete next step from actionable tasks, reports, changed tasks, incidents, owner messages or actionable hard events. Same-class soft wakes respect the reminder interval; hard events bypass it. No Supporter or model decision is needed. Each heartbeat names its next step; reports, decisions and failure alerts keep separate channels.

A `[provider access lost]` notice means recover the login or respawn on another provider, never a completed task; routing is in `ORCHESTRATION-TASKS`.

Confined mutating Workers persist host-owned Cargo, lane-check, sccache and pnpm store roots. TMPDIR/TEMP/TMP use lane-local `.codeterm-tmp`. Task-bound spawns require explicit read_only or mutating intent. Older task tabs without saved confinement retain their behavior.

<!-- revision: 126 -->
