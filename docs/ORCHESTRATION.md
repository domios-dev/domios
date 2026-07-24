# Orchestration Toolset — Plans & Canvas

<!-- revision: 34 -->

Full reference for the `codeterm plan` CLI and orchestration discipline. The *why* (when to spawn, model routing, role contracts) lives here and in your role prompt — this doc is the authority; machine-specific deltas live in mem preferences (kind=preference, area=orchestration).

> **Doc entry:** `codeterm doc orchestration` (full doc). `codeterm docs ORCHESTRATION` is the same content via the list command.

**Planning source of truth:** in this project, durable planning content lives in CodeTerm GOAL/Spec/Plan/Tasks/Strides. Do not create `docs/superpowers/plans/*` or standalone markdown implementation plans as the primary plan unless the user explicitly asks for an export. Markdown may guide style, but the authoritative state is the planner.

## Model — the EFFORT: GOAL ⟶ Spec ⊥ Plan ⟶ Tasks ⟶ Strides

An effort is authored **top-down**. Define the GOAL first; the **Spec** says WHAT; the **Plan** says HOW and is built FROM the spec; Tasks execute; Strides are the working-points.

```
GOAL              north-star vector (+ vision, constraints, postulates). MUTABLE —
                  corrections are recorded (working-points), never silently overwritten.
Spec  (WHAT)      the requirements doc — its OWN section tree. Stable under tactic change.
 └─ Spec Sections[]
Plan  (HOW)       built FROM the spec — its OWN section tree (execution phases).
 └─ Plan Sections[]      each links to one or MORE spec sections (M:N, via spec_links)
     └─ Tasks[]          each task ⊆ one or more plan sections; carries
         │               acceptance_criteria + test_plan + correctness
         └─ Strides / Milestones[]   report chain / working-points;
                                     loop-back edges form self-improving cycles
```

- **GOAL is a first-class entity** (`Plan.goal` = `{vector, vision, constraints[], postulates[]}`). Set it **first** — it is the vector everything hangs off. It evolves via working-points and its changes are recorded, not overwritten.
- **Spec ⊥ Plan are TWO DISTINCT entities**, each with its own section tree. The Spec holds WHAT/WHY; the Plan holds HOW. **Never cram WHAT into plan sections** — that conflation is the classic mistake.
- **A spec section may belong to MANY plan sections** (M:N) via `spec_links`. Coverage of a spec section = union of tasks across its linked plan sections.
- **Tasks** attach to plan sections via `--sections`. Coverage is derived.
- **Strides / Milestones** are the persisted projection of `codeterm agent report --plan/--task` (authorship auto-resolved from `CODETERM_PANE_ID`). `milestone_kinds` declared at plan creation define the column vocabulary; **Loops** (`LoopSpec`) declare self-improving cycles.
- **Task env:** assigning a task to a tab exports `CODETERM_PLAN_ID` + `CODETERM_TASK_ID`; workers report without being told the ids.

## Lifecycle (author top-down, GOAL-first)

```
plan goal set <plan> --vector ...            # 1. GOAL first — the north-star vector
spec create --title ...                      # 2. Spec (WHAT)
  spec section add <spec> --title --body     #    requirements tree (--parent to nest)
plan create --from-spec <spec> --title ...   # 3. Plan (HOW), built FROM the spec  [guided]
  plan section add                           #    execution phases
  spec link add <plan> --spec <s> \          #    M:N: a spec section ⟶ plan section(s)
        --spec-section <ss> --plan-section <ps>
  plan task add --model --why \              #    tasks carry routing rationale +
        --acceptance-criteria ... --test-plan ... --correctness ...
  plan task assign --tab                      #    bind a tab (exports CODETERM_TASK_ID)
  ...workers run, report → strides/milestones...
  # §38 acceptance: workers report in_review; the ORCHESTRATOR judges `done`.
  plan loop create                            #    declare a self-improving cycle
  # CLOSEOUT (mandatory before archive): close every worker + Planner tab you
  # spawned, remove lane worktrees + delete merged branches, stop lane dev
  # daemons, prune stale/superseded milestones. Your spawned tabs/worktrees/
  # branches are THIS effort's mess to clear — never the next orchestrator's.
  plan archive                                #    done; hidden from canvas
```

## Closeout is part of the effort — leaving no mess

An effort is not complete when the code merges; it is complete when the workspace
is clean. Before `plan archive`, the orchestrator MUST:

1. **Close every worker tab and the Planner tab** it spawned (`codeterm tab close <id>`).
   Accepted lanes must not linger as zombie tabs — a screen full of dead worker
   tabs is unfinished cleanup, not finished work.
2. **Sweep lanes** (LANES rule 8): remove merged worktrees, delete merged branches,
   stop lane-specific dev daemons, record `--cleanup-status done` per task.
3. **Prune milestones**: drop stale/superseded strides so the completed plan reads clean.

The self-termination guard prevents an orchestrator/planner from closing its OWN
tab — so close children, not yourself. Sweep at every group close too, not only
at the end, so tabs and worktrees never accumulate.

Heartbeat digests surface cleanup pressure for the current canonical repository
and orchestrator hierarchy. More than five worktrees or direct workers produces
a soft reminder; more than ten produces a critical warning to clean merged lanes
or inactive worker tabs before expanding the wave.

Edit while work runs — `codeterm plan ...`/`spec ...` mutate a running effort safely. Re-orient with `plan get PLAN_ID` / `spec show SPEC_ID` (markdown) at any time.

## Planning Method — state in CodeTerm, quality from Superpowers

Use CodeTerm for durable state, and use the old Superpowers planning habits as
a quality checklist. Do **not** export the plan to markdown just because a
planning template says so; in CodeTerm the authoritative objects are GOAL,
Spec, Plan, Tasks, and Strides.

Correct authoring order:

1. Set or refine the GOAL.
2. Write Spec sections for requirements, evidence, gaps, constraints,
   acceptance, non-goals, and decision vocabulary.
3. Write Plan sections for execution phases, file ownership, decision flow,
   verification, and approval.
4. If the plan needs a dedicated Planner, create a `planning` task. Planning
   tasks are allowed before approval and exist only to improve GOAL/Spec/Plan
   structure. They are not execution work.
5. Run a plan self-review / doctor pass.
6. Approve the plan, then generate execution tasks.

Good Plan sections include:

- a file ownership map with exact paths and responsibilities;
- an approval/self-review gate;
- exact verification commands and expected results;
- wave sequencing: discovery/research first, synthesis of the remaining task
  graph, then implementation/review/verification tasks.

Discovery/research tasks are **intermediate only**. Completing the first wave
does not complete the plan. A discovery wave must record evidence, accepted
gaps, decisions, and follow-up requirements; the Orchestrator then uses those
results to autonomously generate the remaining execution/review/verification
tasks and continues until the GOAL is satisfied. Stop only when the plan goal
and acceptance gates are complete, or when the user explicitly blocks/changes
the effort.

Good Tasks include body, section links, routing model + why, acceptance
criteria, test plan, correctness vector, exact commands, and report format.
Tasks should be small enough that a Worker can execute and report cleanly.
Execution tasks must not be created until planning_state is `approved`.
Planning-kind tasks are the exception and are **first-class**: their job is to
improve the GOAL / spec / plan / task-graph itself, so `codeterm plan task add
--type planning` is allowed before approval **without `--force`**. Every
other kind (implement/review/build/…) still hits the approval gate and requires
an `approved` plan or an explicit `--force` draft.

Use `codeterm plan doctor PLAN_ID --kind quality` before approval and again
before dispatch. It is a structural approval gate: missing goal/spec link,
task-quality gaps, heartbeat-before-execution state, and *non-planning* tasks
created before approval (`tasks_before_approval`) are errors.
Planning-kind tasks never trip `tasks_before_approval`; quality checks (missing
body, criteria, section link) still apply to them. It deliberately does **not**
infer quality from prose keywords; if a rule must be machine-enforced, model it
as structured planner state instead of text heuristics.

Heartbeat is runtime execution state, not a planning scratchpad. Plan-owned
nudges are automatic once an assigned orchestrator owns an execution-mode plan
with unfinished work. Manual heartbeat focus/lanes/rules are only overlays that
make the automatic nudge more precise; they are not a prerequisite. Workers get
task nudges only while their assigned task is still waiting for their report.

## Long-running shell gates — completion contract, never polling

Long-running shell commands (builds, soaks, remote regressions) are gates, not
things to watch from the Orchestrator's control loop. Delegate them or launch
them in a separate visible tab/tracked process, and give the command a
completion contract at launch:

```bash
codeterm tab new --cmd '<cmd>; rc=$?; codeterm send --tab <orchestrator-tab> "GATE:<task_id> rc=$rc"; exit $rc'
```

Record the gate on the task and work other frontier items until the marker
arrives. A process handle, process-exit event, channel close, or the command's
own completion message is the readiness signal. Never block the user
conversation on a long tool call, and never substitute sleep/poll loops for a
completion signal. If no signal exists, add one at the ownership boundary.

## Worker Lanes — Git worktrees + task metadata

The portable lane lifecycle contract is `codeterm docs LANES`. This section
adds the orchestration mechanics around that standard.

Each delegated lane should run in a real `git worktree` with its own branch.
The Orchestrator's integration checkout stays separate from worker edits:

```bash
git worktree add -b lane/<handle> ../codeterm-<handle> HEAD
codeterm agent spawn codex \
  --parent <orchestrator-tab> --role Worker \
  --cwd ../codeterm-<handle> \
  --title <handle> \
  --task-file /tmp/<task>.md
codeterm plan task update PLAN_ID TASK_ID \
  --tab <worker-tab> \
  --branch lane/<handle> \
  --worktree ../codeterm-<handle> \
  --workspace-name <workspace>
```

This is durable task state, not only a chat convention. The task lane metadata
must identify the worker tab, branch, worktree path, workspace, cleanup status,
and any cleanup hints. Use that metadata for canvas presence, reports,
reconciliation, handoffs, and cleanup.

Worker lanes normally end this way:

1. Worker reports `in_review`.
2. Orchestrator enqueues QA when the lane needs serialized verification:
   `codeterm plan qa queue PLAN_ID --task TASK_ID --note "<scope>"`.
3. A tester claims exactly one queued item:
   `codeterm plan qa next PLAN_ID --claim --note "<scope>"`. Claim output
   includes the task test plan.
4. The tester records the verdict with evidence:
   `codeterm plan qa pass PLAN_ID TASK_ID --note "<evidence>"` closes the task;
   `codeterm plan qa fail PLAN_ID TASK_ID --note "<failure>"` bounces it to
   `in_progress` and nudges the implementing worker.
5. Orchestrator integrates with a real merge commit from the lane into the
   integration target. Cherry-pick, squash, replay, and patch-copy are forbidden.
   If ancestry is wrong, rebase the same lane onto the target, re-run its bounded
   review, then merge that lane.
6. Orchestrator closes the worker tab, removes the clean
   worktree, and sets `--cleanup-status done`.

QA is still part of the `in_review` lifecycle. The persisted task keeps
`review_stage=qa` plus `qa_state=queued|running` only while its status is
`in_review`; leaving review clears both fields. Every enqueue, claim, pass, and
fail writes a `qa` milestone in the same atomic plan write as the state change,
so there is no stage-without-evidence state. `codeterm plan qa queue PLAN_ID`
shows the serialized queue.

Do not keep accepted branches around as visual memory. The task report, merge
commit, and cleanup stride are the durable evidence.

Never remove a worker worktree before its diff is integrated, bounced, or
explicitly abandoned. Never keep a finished worker visually assigned just to
remember who did the work; executor history and lane metadata are the memory,
live assignment is only current work.

Blocked worker lanes use `blocked` plus `--status-label`, not a separate schema.
Use `human-decision` for an escalated choice outside agent authority. Use
`human-verify` for an external leg the agent cannot physically drive, such as
OAuth, real email/payment, or a production side effect.

### Milestone gates — three-tier testing ladder

Regression cost scales with scope, so testing is tiered, not uniform.

Verification stays proportional to risk. During implementation, run one
smallest meaningful oracle after a coherent change, not a broad suite after
every edit. Batch compatible expensive checks at the group boundary. Deferring
critical evidence is allowed while work continues, but it remains explicit
open risk and cannot support a done or release verdict.

Testing ladder — per-task = focused filters only; per-group (milestone) = one full regression gate (cargo suite + ts-unit + playwright golden, serialized in the warm checkout); per-release = the farm matrix across every platform.

- **Per-task:** each lane runs the narrowest focused filter that proves its
  change — never the whole suite. Assert the true end state of the user journey,
  not only an API/status response: `POST 200` is not a pass unless the DOM,
  recipient thread, or persisted visible result is correct and stable.
- **Per-group (milestone):** task groups are milestones. When the last task of a
  group closes, next-actions and the heartbeat emit one regression-gate
  suggestion. Run the full gate ONCE for the whole milestone, then record it as a
  single group-scoped working point:
  `codeterm plan qa next PLAN_ID --group GROUP_KEY --claim --note "<scope>"`.
  The quality doctor (`--kind structure`) warns while a completed group has no
  regression run on record since its last close; reopening a task re-arms the
  gate. The suggestion is advisory pressure, not a hard lock.
- **Per-release:** the full farm matrix (`release-X.Y.Z` tag) builds and tests
  across every platform. That is the release pipeline, not per-milestone work.

## Planner Profile

Use a Planner when the Orchestrator needs high-reasoning help to create or
repair CodeTerm planner GOAL/Spec/Plan/Task structure, not to execute the work.
Planner is a profile on a structural Worker: it cannot spawn agents, assign
panes, accept tasks, or judge final completion. Execution ownership and final
acceptance stay with the Orchestrator.

### Planning is a plan state, not a bootstrap task

Every plan carries a **planning_state** the Planner advances:

- `drafting` — GOAL/spec/plan skeleton being built.
- `structure_ready` — structure complete, awaiting approval review.
- `approved` — approved for execution; **only this state unlocks execution-task
  creation** (the single gate — `codeterm plan task add` for a non-planning kind
  is rejected until then; planning-kind tasks and `--force` drafts are exempt).

Promotion and onboarding **do not create a planner bootstrap task** — planning
progress lives in the plan's own state, advanced by the Planner (or the user):

```bash
codeterm plan planning-state PLAN_ID --set structure_ready --tab <planner-tab> --note "skeleton done"
codeterm plan approve PLAN_ID          # user/orchestrator: advances to approved
```

Only the Planner tab or the user may advance the state; each advance records an
author and note as a working point in `planning_log`. `codeterm plan approve`
still runs the quality doctor first, which gates approval on a set GOAL, a linked
spec, plan sections, and a verification hint. Nudges and `next_actions` derive
the "delegate planning work to the attached Planner" prompt from planning_state
plus the plan's `planner` binding — named by binding, never guessed.

Preferred spawn shape:

```bash
codeterm agent providers
codeterm agent models <provider>
codeterm agent spawn <provider> [--model <model>] \
  --parent <orchestrator-tab> \
  --role Planner \
  --task-file <brief>
```

The Orchestrator owns the provider/model choice based on availability, planning
depth, and current usage headroom. Omit `--model` to use the provider default.
Put the brief in a task file so long planning context, backticks, and command
snippets arrive byte-for-byte.

The Planner must hand back:

- the GOAL it created or refined;
- Spec sections for WHAT/WHY, constraints, acceptance, and decision vocabulary;
- Plan sections for HOW, sequencing waves, ownership, approval, and verification;
- tasks with body, section links, model plus why, `executor_role` for execution
  tasks (slug that names the agent personality — e.g. `rust-backend-dev`,
  `frontend-dev`, `code-reviewer`), `role_guidance` where useful, acceptance
  criteria, test plan, correctness vector, and report expectations;
- `codeterm plan state <plan_id>` / quality-doctor status, known gaps, and the
  next concrete Orchestrator action.

`executor_role` is a free-form slug that lets the Orchestrator route agent
personality per task declaratively, without hard-coding provider IDs in the
dispatch loop. Execution tasks without `executor_role` are flagged as a quality
WARNING by `codeterm plan doctor --kind quality`. Planning-kind tasks are exempt.

Planner-authored `role_guidance` is worker onboarding text. When spawning a
worker from a task, prepend that guidance to the task brief so the worker knows
its stance and boundary, then add the task body, acceptance criteria, test
plan, correctness vector, and report command. Do not turn `role_guidance` into
a custom role registry or a third structural AgentRole.

### Planner iteration loop

Planner work is iterative when the result depends on taste, ambiguous UX, or
new evidence from implementation. The Orchestrator owns execution, but can
return to the Planner with the current board state, screenshots, failed gates,
user feedback, and worker reports. The Planner then revises the GOAL/Spec/Plan
and task graph instead of the Orchestrator improvising large changes inline.

Use this loop:

1. Orchestrator creates or updates the effort goal and assigns a Planner task.
2. Planner writes/repairs Spec, Plan, task briefs, role guidance, and gates.
3. Planner advances planning_state to `structure_ready`; the Orchestrator (or
   user) runs the quality doctor and `plan approve` to reach `approved`, which
   unlocks execution-task creation, then dispatches workers, gates their output,
   and records what happened.
4. If implementation exposes a blocker, weak UX, missing requirement, or the
   user asks to iterate, Orchestrator creates a Planner replanning task with the
   evidence and asks for a revised task graph.
5. Repeat until the GOAL and acceptance gates are satisfied; discovery or the
   first planning wave is never considered completion by itself.

### Planning ownership

The **Orchestrator is the single authority** for plan structure, execution, and
acceptance. While planning_state is not `approved` and a Planner is bound, the
Orchestrator delegates planning input to the Planner specialist — new
requirements, discovered blockers, implementation evidence, user feedback —
then accepts, revises, or rejects the draft. The Orchestrator dispatches
workers, gates their output, integrates results, and may record facts (strides,
heartbeat updates, lane metadata). It must **not** silently improvise graph
nodes (GOAL edits, new spec/plan sections, new tasks) while the delegated
planning lane is open. A waived planner (`plan update --planner none --note ...`)
makes the Orchestrator explicitly solo and silences these nudges.

Exceptions (Planner unreachable, emergency structural repair) must be recorded
as a stride. The Planner may draft a correction on its next run, but the
Orchestrator decides what stays.

CLI mutation commands (`plan section add/update`, `plan task add/update`,
`plan goal set`) emit a non-blocking stderr hint while planning_state is
unapproved and a Planner is bound, naming the Planner so delegation is
actionable.

## Handoffs

A handoff is a durable, goal-anchored resume point attached to the plan — far sharper than a provider's generic `/compact` recap. Whoever wakes (you post-compaction, a reseeded session, or another orchestrator) pulls it first and continues from its `next_actions`.

**WHEN to author one:**
- **Ready to compact / context getting heavy** — the primary case (see discipline below). Author your own handoff *before* compacting, never compact-then-hope.
- **End of a run / session** — so the next session resumes cleanly instead of cold-reading the transcript.
- **Handing ownership to another orchestrator** (pair with `plan transfer`).
- **Any durable checkpoint of a long effort** — after a phase lands, before a risky step.

**WHAT goes in one** (the resume contract):
- The **GOAL vector** + where the effort stands against it.
- Current branch/worktree map + integrated diff summary; gates run (exact commands + outcomes); UI/Playwright/manual-oracle status.
- Known gaps / residual risk; branches or worktrees intentionally left in place.
- An ordered **`next_actions`** list — the resume entry point.

**HOW (commands):**
- `codeterm plan handoff create <plan_id> --from-file body.md --goal "<vector>" --next "..." [--next ...] --rule "..." --decision "..."` — attach a structured handoff to the plan.
- `codeterm plan handoff show <plan_id> --latest [--markdown]` or `codeterm plan handoff show <plan_id> --seq <n> [--markdown]` / `plan handoff list <plan_id>` — read them back.
- `codeterm orchestrator handoff --mode new|compact [--reseed] [--from-file|--stdin] [--plan <id>]` — compose + (optionally) reseed this tab in one shot.

### Compaction discipline — verify-loop before reseed

When YOU sense you are ready to compact, do **not** rely on the provider's generic `/compact` summary. Author your **own** handoff and continue from it. Gate reseed behind the verify-loop in `.agents/skills/codeterm-orchestrator/SKILL.md`:

1. **Compose** — `codeterm plan handoff create <plan_id> --goal "..." --next "..." [--rule "..."] --from-file body.md`.
2. **Read back** — `codeterm plan handoff show <plan_id> --latest --markdown`.
3. **Verify x3** — three independent passes: completeness, accuracy, executability. Check the handoff markdown against `plan get`, `spec show <SPEC_ID>`, `plan in-progress <plan_id>`, and `codeterm agent reports`. Any gap means amend, read back, and restart the 3-pass cycle. `plan handoff verify` is not implemented yet; this is manual.
4. **Compact / reseed** — `codeterm orchestrator handoff --plan <plan_id> --reseed --mode new` (preferred; fresh zero-context session) or `--mode compact` (handoff seed then provider `/compact`). Preview sends with `--dry_run` first.
5. Resume from the handoff's `next_actions` — never a generic recap.
## Heartbeat — automatic plan nudges

The orchestrator **heartbeat** is durable state attached to the plan (`current_focus`, `active_lanes`, `rules`, plus appended `learnings`). The daemon **auto-prepends** the state-reconciliation envelope to every plan nudge for an execution-mode plan with unfinished work, even if you never manually set focus/lanes. If focus and lane map exist, they are prepended too — so active orchestration survives compaction and idle drift without a manual self-ping.

Plan nudges are automatic while an execution-mode plan has unfinished work:

- **Owner orchestrator:** receives a timed reminder while the owned plan has any task that is not `done`, `cancelled`, or `paused`.
- **Assigned worker:** receives a timed reminder while its task is `pending`, `in_progress`, or `blocked` and it has not reported back.
- **Event nudges:** tab sends and agent reports still count as activity triggers, but they are not required for the timer to fire.
- **Cadence:** `[orchestration].nudge_interval_secs` defaults to `1800` (30 minutes). `[orchestration].nudge_after_turns` remains the message-count fallback. `[orchestration].plan_reminders = false` disables both.

### Task lifecycle — PAUSED and the relentless nudge contract

The open-work set and the heartbeat-stop condition are the **same predicate**:
a task counts as open only while it is `pending`, `in_progress`, `in_review`,
or `blocked`. `done` and `cancelled` are terminal; **`paused`** is an explicit
operator hold that is *also* excluded from the open-work set — paused
tasks do not keep the heartbeat alive and are folded/de-emphasized in read
models. So there is no nudge dead-zone: while any non-terminal, non-paused task
is open, the nudge **repeats until every open task is dispatched, paused with a
reason, or sent to replanning; silence is not an option.**

Every open task must take exactly one of five exits: (1) dispatch it to a
worker, (2) pause with a reason, (3) judge an `in_review` task to done or bounce
it, (4) send it to replanning, (5) escalate to a smarter model.

- **Pause:** `codeterm plan task pause <plan> <task> --reason "<why>"` — requires a
  reason, sets `paused`, clears the lane assignment, and records a working-point
  stride. Pausing with no reason is rejected; a paused task without a reason is a
  **doctor error** (`paused_without_reason`).
- **Resume:** `codeterm plan task resume <plan> <task>` — returns the task to
  dependency-waiting `pending`, clears `pause_reason`, and records a stride.
- **Sequencing is not parking:** express ordering with `--depends-on`. A
  `pending` task with unfinished dependencies is shown as **waiting on dependency**;
  an actionable `pending` task is shown separately. Use `paused` only for an
  explicit operator hold with a reason, never as a sequencing surrogate.
- **Age thresholds (drift):** a `pending` task that stays **unstaffed over 4h**,
  or an `in_review` task that stays **unjudged over 2h**, is recorded as drift
  and named explicitly in the escalated nudge so stalled work cannot hide. Doctor
  warns on aged unstaffed open tasks (`aged_unstaffed_task`).

### Task-status state machine — transitions, preconditions, ownership

Statuses: `pending → in_progress → in_review → done`, with `blocked`, `paused`
(non-terminal deferrals) and `cancelled` (terminal) off to the side. Two rules
keep the board honest about what is actually running:

- **`in_progress` requires a bound executor tab.** A task cannot be "in
  progress" with nobody on it — the transition is rejected (`in_progress
  requires an executor tab`) unless a tab is assigned.
- **Taking a task IS starting it.** `codeterm task take <task>` binds the
  caller's tab as executor **and atomically flips `pending → in_progress`** in
  one step — the tab binding is the precondition, so there is no separate
  "record --status in_progress". Dispatching a worker to a task (which binds the
  tab) advances it the same way. The execution layer (a worker really coding in
  a lane) and the tracking layer (the task row) can no longer diverge, so the
  board never shows `pending` while a worker is live. Only `pending` auto-advances
  on bind; other states are left untouched.

Who owns each flip:

- **Worker owns `pending → in_progress`** — it holds the tab, so `task take`
  (or being dispatched) is what starts the task. The worker does not need the
  orchestrator to flip its status.
- **Worker owns `in_progress → in_review`** — it moves the task to review when
  its work is ready and reports back.
- **Orchestrator owns the `in_review` gate** — it judges an `in_review` task to
  `done` or bounces it back; it owns `blocked`/`paused`/replanning decisions.

### Event-driven orchestration: no manual wait loops

Do not create manual `sleep`, polling, or "wait and check again" loops while
orchestrating. The plan already owns wake-up delivery through timed heartbeats,
worker reports, and state-reconciliation nudges. Manual wait loops make the
orchestrator burn attention on one lane and can hide the fact that other tasks
are ready to dispatch or review.

Allowed checks:

- Run one immediate sanity check after a concrete event: a worker report, a plan
  reminder, a tab death, a user message, a gate finishing, or a command you just
  started returning.
- If no event happened, do not keep polling a worker. Keep the heartbeat lane map
  accurate, leave the worker assigned, and advance another independent task if
  one exists.
- If a worker appears overdue only because a plan nudge told you so, check
  `codeterm orchestrator status --plan PLAN_ID --tab ORCH_TAB` first.
  Inspect a worker tab at most once, then either bounce it with a concrete next
  instruction or record why the lane remains in progress.

The correct idle posture is not "wait 30 seconds and poll again"; it is "the
plan will nudge me when attention is due, and until then I either process another
ready task or stop talking."

### Focus discipline — the heartbeat amplifies what you write

`current_focus` is prepended to every future nudge, so it must name an action
YOU own and will execute ("dispatch t7; judge t0b report"), never a wait on
another agent. A focus like "planner decides what to dispatch next" is an
ownership inversion — dispatch is never delegable to a Planner — and it turns
every subsequent nudge into a confirmation of idleness (a real 5-hour stall
was traced to exactly this). If you are blocked on another agent, phrase the
focus as the action you will take: "bounce/escalate <agent> about <thing> if
unchanged at the next nudge."

### Dispatch frontier — re-derive after every gate

Every acceptance/close/integration changes which pending tasks are unblocked.
Immediately after closing a task or accepting a lane, re-check the frontier:
any task whose precondition just became true gets dispatched NOW, paused with
a recorded reason, or routed to replanning. The most common stall: a gating
task (review/verification) becomes dispatchable at the moment the last lane
integrates, while the stale focus still describes the finished wave.

A nudge is handled only if it ends in a state change for at least one open
task: dispatch / pause (or a decision stride) / judge a report / bounce a
worker / hand to replanning. "Waiting" is a valid response only for tasks
with a live assigned lane.

### Parallel writers, serialized gates

Parallelize independent implementation lanes freely on the same machine: a UI
lane, a backend lane, a docs lane, and a review lane can all write at once when
their file ownership is disjoint. The bottleneck is verification, not writing.

Do **not** let every worker run broad verification in its own fresh worktree.
Fresh worktrees rebuild caches, contend on CPU/IO, and make the machine feel
stuck. Workers run the smallest meaningful focused checks for their lane and
report exact commands. The orchestrator, or an explicit test-runner worker in a
warm integration checkout, serializes broad gates: repo-wide compile, full test
suites, UI/e2e sweeps, packaging, release, and any other expensive verification.
If a lane needs a heavy gate, record it as pending central verification instead
of making several worktrees verify the whole system at once.

The QA queue is the enforcement mechanism for those central gates. Only one QA
task runs per plan by default, so the warm checkout stays hot and the machine
does not thrash across identical broad builds.

```bash
codeterm orchestrator heartbeat set \
  --focus "Integrate lane fixes, then cut release" \
  --lane "lane-a: CLI hardening (worker p1)" --lane "lane-b: canvas perf (worker p2)" \
  --rule "Never merge a lane before its gate is green"
codeterm orchestrator heartbeat set --clear-lanes        # wipe lanes when a phase ends
codeterm orchestrator heartbeat show                     # inspect current heartbeat
codeterm orchestrator learn "QuickJS lacks console — guard host calls"   # append a learning
codeterm orchestrator status --plan PLAN_ID              # compact counts + next actions
codeterm --json orchestrator inspect --plan PLAN_ID       # explicit full snapshot with report bodies
```

`set` writes only the fields you pass (omitted fields are left intact); `--clear-lanes` / `--clear-rules` empty those lists and cannot be combined with `--lane` / `--rule` in the same command. Plan is resolved from `--plan`/`CODETERM_PLAN_ID`, else the active plan owned by this tab/session. Keep the plan-owned heartbeat current after each state-reconciliation pass — the focus + lanes are what get reinjected.

## Test plans — executable, not descriptive

A task's `test_plan` is a contract with the executing worker, not a note to self.
It starts from the **oracle**: the exact command or observation that PROVES the
task done. Then it names the method by the work — TDD for code paths, fixtures
for parsers and formats, live dev walkthrough for UX, seeded eval cases for
prompt or behavior changes. If the oracle is expensive (full build, broad suite,
e2e), route it through the QA queue instead of a per-lane gate. A criterion
without an oracle is an opinion.

Executable means the plan names:

1. the exact command(s) to run,
2. the exact surfaces touched (testids, selectors, files, endpoints),
3. numbered steps, each with the observable pass condition.

If the executing worker would have to invent a selector, a step, or an expected
value, the plan is incomplete — write the scenario, not the intention.
`"Component tests + manual check"` is not a test plan.

The reference standard is the picker-v2 task
(`task_234c14f04d9f4dbfb5a743d50486617d`): numbered Playwright scenarios, exact
testids, exact pass conditions, cleanup rules, and the exact run command. That
is the bar.

This is enforced mechanically, not by exhortation:

- **Doctor** emits a `test_plan_vague` warning when a non-planning task's
  `test_plan` is under ~120 chars OR names no command-like token (backtick,
  `npx`, `cargo`, `codeterm`, `bash`, `curl`). The predicate is structural
  (length + command token) — it never judges the prose itself, consistent with
  doctor philosophy. Planning-kind tasks are exempt.
- **QA claim** (`codeterm plan qa next --claim`) prepends
  `TEST PLAN IS VAGUE — bounce to the orchestrator for a concrete plan before
  verifying.` when the same predicate fires. A vague test plan is itself a
  bounce reason: return it for a concrete, executable plan before verifying.

## Plan skills — the plan's own know-how

A plan accumulates reusable procedure as it runs: a test recipe, a seeding
script, a gotcha worked around once. Instead of leaving that in chat where the
next worker cannot find it, record it as a **plan skill** — org-code that lives
with the effort and every future lane inherits.

```bash
codeterm plan skill new <slug>     # scaffold a SKILL.md (+scripts/) in the plan sidecar
codeterm plan skill list           # browse what earlier lanes proved out
codeterm plan skill show <slug>    # read one; bodies are never inlined into prompts
codeterm plan skill edit <slug>    # improve an existing skill before writing a new one
```

Skills live in the plan sidecar at
`.codeterm/plans/<bucket>/<plan>/skills/<slug>/SKILL.md`. The registry surfaces
as a single pointer line in the orchestrator heartbeat and worker charter — a
count and slug list, never the bodies.

Principles (encoded in the orchestrator and planner suffixes so the behavior
emerges rather than being a checklist):

- **Org-code, not chat.** Learned something reusable this effort? Record it.
  Improve an existing skill before writing a new one.
- **Separate harness from target.** A skill names what is FIXED — the oracle,
  the command or observation that proves success — and what is MUTABLE. Never
  edit the oracle to make results look better.
- **Measurable over vibes.** Prefer a skill with a `--check` command; cite the
  check when keeping or discarding an approach.
- **Simplicity is a criterion.** A marginal gain that adds hacky steps is a
  loss; a deletion that preserves results is a win. Applies to skills and code.
- **Ledger honesty.** Every skill edit appends to the plan's
  `skills/ledger.jsonl` — `keep | discard | superseded` plus a one-line why.
  Discarded approaches are data, not embarrassment.

**Promotion path.** A plan skill that proves generally useful — not specific to
this effort — graduates to a machine-level mem skill via `codeterm mem skill`,
so it is available across future plans. Suggest promotion at retrospective time,
not mid-run.

## Command Reference

### Plan CRUD

```bash
codeterm plan create --title "My plan" \
  [--spec "..." | --spec-file PATH] \
  [--kinds status_update,testing,review] \
  [--project PATH]

codeterm plan list \
  [--project PATH] [--workspace] \
  [--active] [--archived] [--all] [--unowned] \
  [--orchestrator PANE_ID]

codeterm plan get PLAN_ID

codeterm plan update PLAN_ID \
  [--title T] [--spec "..." | --spec-file PATH] \
  [--status active|archived] [--mode planning|approved|execution|complete]

codeterm plan approve PLAN_ID
codeterm plan unapprove PLAN_ID
codeterm plan mode PLAN_ID --set planning|approved|execution|complete
codeterm plan planning-state PLAN_ID --set drafting|structure_ready|approved \
  [--tab PLANNER_TAB] [--note "..."]

codeterm plan state PLAN_ID
codeterm plan doctor [PLAN_ID] [--kind storage|structure|quality|all]
codeterm plan search "query" [--kind plan|spec|plan_section|spec_section|task|stride] \
  [--active | --archived | --all] [--project PATH] [--limit N]
codeterm plan archive PLAN_ID
codeterm plan delete PLAN_ID
```

### Recall Search

`codeterm plan search` searches the synced planner corpus through the same FTS5
+ sqlite-vec hybrid engine used by `codeterm mem search`, but in a project-local
derived index under the CodeTerm data dir (`~/.codeterm*/plan-indexes/...`), not
inside the repository. The source of truth remains committed plan/spec files;
deleting the sqlite index is safe because `codeterm plan reindex [--project
PATH]` rebuilds it from those files.

Plan files are bucketed by lifecycle:

```
.codeterm/plans/active/<plan_id>/plan.json
.codeterm/plans/archive/<plan_id>/plan.json
```

All human-readable sidecars live beside that `plan.json`: `goal.md`,
`INDEX.md`, `sections/*.md`, `tasks/*.md`, `tasks/<handle>/strides.jsonl`,
`milestones/*.md`, `elements/*.md`, and `handoffs/*.md`. Archiving moves the
whole plan folder from `active` to `archive`; unarchiving moves it back. There
is no flat `.codeterm/plans/<plan_id>.json` layout.

Markdown sidecars include YAML frontmatter with `plan_id`, `kind`, `id`,
`handle`, and relevant `title`/`status` fields. The loader strips that
frontmatter before hydrating the plan body, so the UI and JSON model see clean
markdown. The metadata exists so a `mem search` hit or an `rg` hit inside a
markdown file can be reverse-resolved to the exact plan, task, section,
milestone, or element without opening every JSON file first.

Indexed units are `plan`, `spec`, `plan_section`, `spec_section`, `task`, and
`stride` (task report / working-point). Plan/spec writes update FTS and vectors;
archive/unarchive updates the indexed status.

Search defaults to active work only. Use `--archived` for completed historical
plans, or `--all` when mining both current and historical work:

```bash
codeterm plan search "windows conpty flash" --active
codeterm plan search "usage parser reset" --archived --kind task
codeterm plan search "why did we remove crisp zoom wrapper" --all --kind stride
```

### Ownership

```bash
codeterm plan transfer PLAN_ID --to ORCHESTRATOR [--reason "..."]
codeterm plan disown PLAN_ID
codeterm plan claim PLAN_ID
```

### Milestone kinds

```bash
codeterm plan kinds set PLAN_ID --kinds status_update,testing,review
```

### Sections

```bash
codeterm plan section add PLAN_ID --title "..." \
  [--body "..." | --body-file PATH] [--parent SECTION_ID]

codeterm plan section update PLAN_ID SECTION_ID \
  [--title "..."] [--body "..." | --body-file PATH]

codeterm plan section show PLAN_ID SECTION_ID    # title + body + tags + subsections
codeterm plan section rm PLAN_ID SECTION_ID
```

(`codeterm spec section show SPEC_ID SECTION_ID` is the spec-side equivalent.)

### Tasks

`--name` is the human-readable title shown on board cards and in lists — write
it as a short sentence ("QA pipeline: In-QA stage + serialized test queue"),
never a kebab-case slug ("task-review-stage-qa"). Slugs belong to handles and
branch names, which are derived separately.

```bash
codeterm plan task add PLAN_ID --name NAME \
  [--type implement|research|review|prototype|docs|fix] \
  [--sections id,id] \
  [--model "claude:opus"] \
  [--why "..." | --why-file PATH] \
  [--body "..." | --body-file PATH] \
  [--intent "..." | --intent-file PATH] \
  [--assign-tab TAB_ID] \
  [--loop-from TASK_ID] [--force]

codeterm plan task update PLAN_ID TASK_ID \
  [--name N] [--type T] [--model M] \
  [--why "..." | --why-file PATH] \
  [--status pending|in_progress|blocked|done|cancelled] \
  [--status-label "90% done"] \
  [--body "..." | --body-file PATH] \
  [--intent "..." | --intent-file PATH]

codeterm plan task close PLAN_ID TASK_ID \
  [--result "..." | --result-file PATH] [--mesh PEER]

codeterm plan task assign PLAN_ID TASK_ID --tab TAB_ID [--mesh PEER]

codeterm plan task rm PLAN_ID TASK_ID
codeterm plan task rm PLAN_ID --all --yes
```

`--type` is an open string — the listed values are hints for icon/color only. `--model` and `--why` record routing rationale on the task (not enforced, used for canvas display).
`plan task add` refuses planning-mode plans. Run `plan doctor --kind quality` and
`plan approve` first; use `--force` only for a deliberate draft task.

### Milestones

```bash
codeterm plan milestone add PLAN_ID TASK_ID \
  --kind status_update \
  [--add-kind] \
  [--text "..." | --text-file PATH] \
  [--from AGENT_REF]

codeterm plan milestone update PLAN_ID TASK_ID MILESTONE_ID \
  [--kind K] [--text "..." | --text-file PATH]

codeterm plan milestone rm PLAN_ID TASK_ID MILESTONE_ID
```

`--add-kind` declares the kind on the plan and uses it in one call. `--from` is a rare manual override for backfilling; normally authorship is auto-resolved.

### Loops

```bash
codeterm plan loop create PLAN_ID \
  --target "description of what the cycle operates on" \
  --budget 25 \
  [--stop-when "scout finds no further improvements"]

codeterm plan loop set LOOP_ID [--budget N] [--stop-when "..."]
codeterm plan loop status LOOP_ID
codeterm plan loop record LOOP_ID --note "what changed this iteration" [--stop]
codeterm plan loop stop LOOP_ID
```

Loop iterations are recorded explicitly. `plan loop record` increments the loop
count, stamps the iteration time, attaches a stride to the current task when one
is available (otherwise the freshest open planning task, then the freshest open
task), and flips the loop to `exhausted` at budget or `done` when `--stop` is
set. This is the durable visibility path used by state/heartbeat/doctor.

`LoopSpec.stages` was removed. It had no CLI surface, no runtime consumer, and
was dead planner state. If stage-specific execution is needed later, add it back
with a real command surface and task-routing behavior rather than carrying an
unused field in persisted plans.

### Canvas layout

```bash
# element ids: task:TASK_ID, section:SECTION_ID, ms:MILESTONE_ID, el:ELEMENT_ID
codeterm plan layout set PLAN_ID --element task:TASK_ID [--x 0] [--y 0]
```

### Canvas notes & free-form elements

```bash
codeterm plan note add PLAN_ID \
  [--text "..." | --text-file PATH] \
  [--link-task TASK_ID] [--link-section SECTION_ID]

codeterm plan element add PLAN_ID \
  --kind decision|note|mockup \
  [--content "..." | --content-file PATH] \
  [--link ELEMENT_ID]
```

## Provenance nodes + edges (§3.8)

All canvas elements are typed nodes; their `links` arrays are edges drawn as connectors. Provenance emerges from this same primitive — there is no separate history engine. "This section was re-scoped" = a `decision` element node whose `links` points at the section. `Plan.links` holds plan-to-plan edges (derived-from / discovered-during).

## Live mutability (§3.9)

The plan is a live object edited while work runs. Every `codeterm plan ...` call mutates it in place; the daemon serializes writes (single-writer, atomic temp-rename), refreshes the in-memory index, and fans out `plan-changed` so the canvas live-updates. Re-orient at any time: `plan get PLAN_ID` → markdown snapshot of current state. Teaching errors (§4.3) and plan-adherence nudges (§4.2) keep agents on track even after context compaction.

## Orchestrator Role Discipline

**PREREQUISITE — read the `codeterm` skill first.** It's auto-installed in your provider's skills dir. Baked in on purpose: you don't know the CLI by default until asked.

### Operating Model

**DELEGATE BY DEFAULT — even when told "do it yourself".** Every turn, ask "which tasks can I delegate RIGHT NOW, in parallel?" Dispatch disjoint lanes. Do work inline only for trivial fixes (a couple of lines). NEVER grind mechanical work inline — that goes to Workers. Route by difficulty: weaker/cheaper models for mechanical shuffling; stronger for reasoning/review/architecture. Your job = decomposition + dispatch + gating + acceptance, NOT execution.

Hierarchy is flat: Orchestrator → Workers. Workers do not spawn Workers.

**ALWAYS ATTACH workers to yourself: `--parent <your-tab-id> --role Worker`.** Without `--parent`, the spawn is standalone and untracked (report routing, hierarchy, lifecycle/cleanup all break). Standalone root spawning is reserved only for an independent sub-orchestrator running its own workstream.

**Give each Worker an identity: `--title` + `--description`.** A worker with a clear title is legible in the agent tree, ledger, and presence layer.

### Before Spawning Workers (Dispatch Checklist)

1. Write the lane map: problem, area, worktree/branch, expected output, acceptance gates.
2. Run a plan-review pass for broad/risky/multi-lane plans before dispatch.
3. Put repo-required gates in the task: fmt, typecheck, unit tests, build wrappers, UI/e2e.
4. Keep task prompts self-contained and in English for agent-to-agent traffic.
5. Avoid backticks inside shell-quoted `--task` strings.
6. **Think before you send** — does the recipient have everything to act on its own? A gap blocks the whole cycle.
7. **Tell the worker to EXECUTE the report, not write it.** Spell out: "report by RUNNING `codeterm agent report '...'` as a shell tool call". Some models compose the report as chat text and it never reaches you via `[Report from ...]`.

### Worker Report Shape

```
STATUS: done | blocked | partial | in_review
CHANGES: files touched
BUILD/TYPECHECK/TESTS: result
FOLLOWUP: anything the Orchestrator should know
```

Workers report `in_review` — not `done`. The Orchestrator decides `done` after gating.

**Keep the pipeline FULL.** Workers WRITE diffs (they don't build), so many run concurrently on non-conflicting files; only the orchestrator's build/verify gate is serial. Every cycle, ask: "what can I dispatch in parallel right now?"

### Heartbeat Discipline

**HEARTBEAT = ACTION, never IDLE.** Idling means sinking into over-verifying one change or sitting on a self-imposed gate while the backlog sits untouched.

Each heartbeat:
1. **Reports first** — process any worker reports (accept/bounce). Don't inspect worker panes if reports arrived.
2. **Re-scan the whole backlog** — list ALL open tasks; ask "what can I dispatch RIGHT NOW in parallel?"
3. **Dispatch — if your focus is blocked, switch lanes, never sit.** A blocked/waiting lane is a signal to advance a DIFFERENT open task.
4. **Timebox verification** — if a gate or test is blocked, record the friction, pause that lane WITH A NOTE, and advance other work.
5. Keep plan heartbeat current (owner, active assignments, lane metadata, latest handoff).
6. **Focus names an action YOU own** — never a wait. If blocked on someone: focus = "bounce <agent> re <thing> if unchanged at next nudge."
7. **Re-derive the frontier after every gate.** Each accept/close unblocks tasks — dispatch, pause with a reason, or replan NOW.

Do not create delayed self-send heartbeats as normal orchestration. If a worker is silent and overdue, check `tab status`/`tab chat` ONCE, record the lane state, and advance a different open task.

### Verification Gates

Unit-green is necessary and never sufficient. The Orchestrator's readiness claim needs evidence at the layer where the user experienced the bug.

Track a ledger per fix:
- Unit or focused test passed.
- Build/typecheck passed through the repo's required wrapper.
- Integration or UI pass run by the Orchestrator.
- UI / end-to-end pass where practical.
- Manual oracle checked for real session data or visuals.
- Test path exercises the real failing scenario, not a helper-only shortcut.
- Blast radius reviewed across providers/platforms.
- Change is in version-controlled source, not only in an installed/dev copy.
- **Gate tests run under a TIMEOUT.** A deadlock hangs — it reads as "still running" and slips a green gate. Wrap every gate in a hard timeout; timeout == failure == bounce back.

### Acceptance — YOU decide `done`

You are the acceptance authority. A Worker reports `in_review` (work written + self-checked). You then decide `done` after verification. Never let `in_review` rot — gate it NOW (→ `done` or bounce).

How to convince yourself (pick by what needs verifying):
- **Code/logic** → run tests/typecheck yourself in the warm checkout.
- **Visual/UX** → look on the board after redeploy, or spin a fast verifier that screenshots.
- **Against spec/GOAL** → re-read the plan's goal + relevant spec section; reject work that satisfies the symptom but misses the vector.

### State Reconciliation — a Reflex

Plan state silently drifts from reality. RECONCILE on two triggers, every time:
- **After every task you close or receive** (report, tab death, accept/bounce).
- **On every heartbeat** — before dispatching the next lane, sweep for drift first.

What to check:
- `in_progress` but assigned tab is dead/absent → clear binding, re-dispatch or flag.
- `in_review` sitting unaccepted → gate it NOW.
- Accepted/merged branch but task still open → mark `done`.
- Task bound to a closed tab → clear the dead binding.
- **NOT YOURS → flag, never touch.**

Don't do this by eye — run: `codeterm plan reconcile <plan>` (lists mismatches); `--fix` applies safe transitions.

### Review Discipline

Two layers — do not conflate them:

- **Self-review (every report):** read the actual `git diff`, re-run the meaningful gate in the integration checkout, grep risk patterns. The report is a signal, not proof.
- **Independent review (separate adversarial reviewer AGENT):** spawn a reviewer prompted to REFUTE / find consequences. Self-review is NOT a substitute — per-tab fetch fan-outs and hot-path regressions routinely sail past tests and self-review; only an independent reviewer catches them.

Run independent review on:
- Broad plans before dispatch.
- Risky lane diffs before integration.
- Final integration diff before declaring a phase done.
- Hot-path / perf-sensitive changes (render loops, fan-out, caching, per-tab mounts).
- Security, provider-boundary, session-binding, or cross-platform changes.

**Tailor review prompt to the model — prefer running TWO in parallel:**
- **Opus** does well with open-ended adversarial framing ("refute / find consequences") — it self-initiates empirical probing without being told each step.
- **Cursor/Composer** is reliable but NOT self-initiating — give it a checklist of review angles to actively probe. Plus a sharp persona ("skeptical QA who VERIFIES by executing, never by reasoning alone").
- **Dual-review:** for critical/final/hot-path diffs, run BOTH (Opus open-ended + cursor prescriptive) and triage the UNION.

### Model Routing

`codeterm agent spawn <provider> --model <id>`. Match task to tier. Verify with `codeterm agent models <provider>` before trusting any id.

| Model | provider / id | tier | best at | avoid |
|-------|---------------|------|---------|-------|
| Opus 4.8 | `claude` / `opus` | frontier | reasoning, planning, decomposition, adversarial/security review, architecture, final integration verification | bulk cheap work, simple edits |
| Sonnet 4.6 | `claude` / `sonnet` | balanced (default) | general coding, research synthesis, docs, moderate reasoning | hardest reasoning (Opus); cheapest fan-out (Haiku) |
| Haiku 4.5 | `claude` / `haiku` | cheap-fast | verification gates, mechanical edits, high-volume parallel fan-out | deep reasoning, ambiguous open-ended tasks |
| Composer 2.5 | `cursor` / `composer-2.5` | cheap-fast coder | bounded multi-file feature coding with explicit acceptance gates | terminal-heavy/non-IDE work, deep reasoning |
| GPT-5.5 | `codex` / `gpt-5.5` | frontier | terminal-heavy agentic work — build pipelines, multi-command orchestration | pure IDE multi-file edits (Composer cheaper) |
| GPT-5.3-Codex-Spark | `codex` / `gpt-5.3-codex-spark` | ultra-fast coder | latency-critical tight-loop coding | multi-step / deep-reasoning tasks |

Routing matrix:

| Sub-task | Primary | Fallback |
|----------|---------|----------|
| reasoning / architecture / planning / decomposition | Opus | Sonnet |
| adversarial code / security review | Opus | Sonnet |
| bounded multi-file feature coding (clear gates) | Composer 2.5 | Sonnet |
| mechanical edits / refactors / rename sweeps | Haiku | Composer 2.5 |
| verification: run tests / e2e / check output / lint | Haiku | Sonnet |
| terminal-heavy agentic (build pipelines, multi-cmd) | GPT-5.5 | Sonnet |
| research / synthesis / docs writing | Sonnet | Haiku |
| cheap high-volume parallel fan-out | Haiku | Composer 2.5 |
| latency-sensitive interactive coding loop | GPT-5.3-Codex-Spark | Composer 2.5 |

Heuristic: judgment → Opus. General coding/research → Sonnet. Mechanical/verify/fan-out → Haiku. Bounded IDE-coding lane → Composer. Terminal/agentic build → GPT-5.5.

### Agent / Tab Hygiene

Long runs accumulate finished Worker panes. Sweep them — it's expected, not optional.

- After a lane integrates/verifies AND its Worker has reported: `codeterm tab close <pane_id>`.
- Sweep periodically: `codeterm tab list` → close panes whose lane is done.
- Closing is recoverable: `codeterm sessions search "<what it was doing>"` → `codeterm sessions resume <id>`.
- Never close mid-task, a tab you don't own, or the General/main tab.
- Close dedicated workspaces when lanes land: `codeterm workspace close <ws_id>`.
- **Worktree pairing:** closing a Worker's tab + removing its git worktree go together: `git worktree remove <path> --force`.

### Sharp Edges

- Never blind-retry a write that returned an error — it may have SUCCEEDED, so retrying DUPLICATES the row. Verify by reading back, not by retrying.
- An inbox / SendMessage-style message does NOT reach a running agent tab — it's silently dropped. Brief at spawn with `--task-file`; steer a running agent with `codeterm send`.

### Usage Check Before Any Big Task Batch

Check both providers before heavy spawning: `codeterm usage latest --provider claude` and `codeterm usage latest --provider codex` (session% + weekly%). Pick the provider mix by remaining headroom. If BOTH are at/near 95%, alert via `codeterm bridge send` and pause heavy spawning until reset. Machine-specific parallelism limits and build constraints live in mem preferences (area=orchestration).

## What lives where

| Content | Location |
|---------|----------|
| CLI mechanics (this doc) | `codeterm docs ORCHESTRATION` |
| Lane lifecycle standard | `codeterm docs LANES` |
| Token-lean reads + in-place body edits | `codeterm docs PLAN-EDITING` |
| Orchestration principle (when to spawn, model routing, discipline) | `codeterm docs ORCHESTRATION` + role prompts; machine deltas are mem preferences |
| Full mem reference | `codeterm docs MEMORY` |
| REST + socket API | `codeterm docs API` |

<!-- revision: 4 -->
