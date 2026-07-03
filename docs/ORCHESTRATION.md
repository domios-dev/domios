# Orchestration Toolset — Plans & Canvas

<!-- revision: 19 -->

Full mechanical reference for the `codeterm plan` CLI. Mechanics only — the *why* (when to spawn, model routing, discipline) lives in the `codeterm-orchestrator` mem skill.

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
- **Task env:** assigning a task to a pane exports `CODETERM_PLAN_ID` + `CODETERM_TASK_ID`; workers report without being told the ids.

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
  plan task assign --pane                     #    bind a pane (exports CODETERM_TASK_ID)
  ...workers run, report → strides/milestones...
  # §38 acceptance: workers report in_review; the ORCHESTRATOR judges `done`.
  plan loop create                            #    declare a self-improving cycle
  plan archive                                #    done; hidden from canvas
```

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
Execution tasks must not be created while the plan is still in planning mode.
Planning-kind tasks are the exception and are **first-class**: their job is to
improve the GOAL / spec / plan / task-graph itself, so `codeterm plan task add
--type planning` is allowed on a planning-mode plan **without `--force`**. Every
other kind (implement/review/build/…) still hits the approval gate and requires
approval or an explicit `--force` draft.

Use `codeterm plan doctor PLAN_ID --kind quality` before approval and again
before dispatch. It is a structural approval gate: missing goal/spec link,
task-quality gaps, heartbeat-before-execution state, and *non-planning* tasks
created while still in planning mode (`tasks_before_approval`) are errors.
Planning-kind tasks never trip `tasks_before_approval`; quality checks (missing
body, criteria, section link) still apply to them. It deliberately does **not**
infer quality from prose keywords; if a rule must be machine-enforced, model it
as structured planner state instead of text heuristics.

Heartbeat is runtime execution state, not a planning scratchpad. Plan-owned
nudges are automatic once an assigned orchestrator owns an execution-mode plan
with unfinished work. Manual heartbeat focus/lanes/rules are only overlays that
make the automatic nudge more precise; they are not a prerequisite. Workers get
task nudges only while their assigned task is still waiting for their report.

## Worker Lanes — Git worktrees + task metadata

Each delegated lane should run in a real `git worktree` with its own branch.
The Orchestrator's integration checkout stays separate from worker edits:

```bash
git worktree add -b lane/<handle> ../codeterm-<handle> HEAD
codeterm agent spawn codex \
  --parent <orchestrator-pane> --role Worker \
  --cwd ../codeterm-<handle> \
  --title <handle> \
  --task-file /tmp/<task>.md
codeterm plan task update PLAN_ID TASK_ID \
  --pane <worker-pane> \
  --branch lane/<handle> \
  --worktree ../codeterm-<handle> \
  --workspace-name <workspace>
```

This is durable task state, not only a chat convention. The task lane metadata
must identify the worker pane, branch, worktree path, workspace, cleanup status,
and any cleanup hints. Use that metadata for canvas presence, reports,
reconciliation, handoffs, and cleanup.

Worker lanes normally end this way:

1. Worker reports `in_review`.
2. Orchestrator inspects the lane diff and gates it in the integration checkout.
3. Orchestrator integrates the lane (`cherry-pick`, merge, or carefully applied
   patch when the integration checkout already has related dirty work).
4. Orchestrator marks the task `done`, closes the worker pane, removes the clean
   worktree, and sets `--cleanup-status done`.

Never remove a worker worktree before its diff is integrated, bounced, or
explicitly abandoned. Never keep a finished worker visually assigned just to
remember who did the work; executor history and lane metadata are the memory,
live assignment is only current work.

## Planner Profile

Use a Planner when the Orchestrator needs high-reasoning help to create or
repair CodeTerm planner GOAL/Spec/Plan/Task structure, not to execute the work.
Planner is a profile on a structural Worker: it cannot spawn agents, assign
panes, accept tasks, or judge final completion. Execution ownership and final
acceptance stay with the Orchestrator.

Preferred spawn shape:

```bash
codeterm agent spawn claude --model fable \
  --parent <orch-pane> \
  --role Planner \
  --task-file <brief>
```

Use `fable` first for planning depth. If that model is unavailable in
`codeterm agent models claude`, fall back to `opus` and keep the rest of the
spawn shape unchanged. Put the brief in a task file so long planning context,
backticks, and command snippets arrive byte-for-byte.

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
3. Orchestrator approves the planning state, switches the plan to execution,
   dispatches workers, gates their output, and records what happened.
4. If implementation exposes a blocker, weak UX, missing requirement, or the
   user asks to iterate, Orchestrator creates a Planner replanning task with the
   evidence and asks for a revised task graph.
5. Repeat until the GOAL and acceptance gates are satisfied; discovery or the
   first planning wave is never considered completion by itself.

### Planning ownership

While a Planner holds an open planning-kind task, the **Orchestrator routes
planning input to the Planner** — new requirements, discovered blockers,
implementation evidence, user feedback. The Orchestrator dispatches workers,
gates their output, integrates results, and may record facts (strides,
heartbeat updates, lane metadata). It must **not** synthesize graph nodes
(GOAL edits, new spec/plan sections, new tasks) while a Planner lane is active.

Exceptions (Planner unreachable, emergency structural repair) must be recorded
as a stride. The **Planner reconciles foreign mutations** on its next run:
it reviews what changed, decides keep/rewrite/remove, and records a correction
stride so the plan state remains coherent.

CLI mutation commands (`plan section add/update`, `plan task add/update`,
`plan goal set`) emit a non-blocking stderr hint when another pane owns the
active Planner lane, naming the task handle so routing is actionable.

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
- `codeterm orchestrator handoff --mode new|compact [--reseed] [--from-file|--stdin] [--plan <id>]` — compose + (optionally) reseed this pane in one shot.

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

- **Owner orchestrator:** receives a timed reminder while the owned plan has any task that is not `done`, `cancelled`, or `parked`.
- **Assigned worker:** receives a timed reminder while its task is `pending`, `in_progress`, or `blocked` and it has not reported back.
- **Event nudges:** pane sends and agent reports still count as activity triggers, but they are not required for the timer to fire.
- **Cadence:** `[orchestration].nudge_interval_secs` defaults to `1800` (30 minutes). `[orchestration].nudge_after_turns` remains the message-count fallback. `[orchestration].plan_reminders = false` disables both.

### Task lifecycle — PARKED and the relentless nudge contract

The open-work set and the heartbeat-stop condition are the **same predicate**:
a task counts as open only while it is `pending`, `in_progress`, `in_review`,
or `blocked`. `done` and `cancelled` are terminal; **`parked`** is a
non-terminal deferral that is *also* excluded from the open-work set — parked
tasks do not keep the heartbeat alive and are folded/de-emphasized in read
models. So there is no nudge dead-zone: while any non-terminal, non-parked task
is open, the nudge **repeats until every open task is dispatched, parked with a
reason, or sent to replanning; silence is not an option.**

Every open task must take exactly one of five exits: (1) dispatch it to a
worker, (2) park with a reason, (3) judge an `in_review` task to done or bounce
it, (4) send it to replanning, (5) escalate to a smarter model.

- **Park:** `codeterm plan task park <plan> <task> --reason "<why>"` — requires a
  reason, sets `parked`, clears the lane assignment, and records a working-point
  stride. Parking with no reason is rejected; a task parked without a reason is a
  **doctor error** (`parked_without_reason`).
- **Unpark:** `codeterm plan task unpark <plan> <task>` — returns the task to
  `pending`, clears `park_reason`, and records a stride.
- **Age thresholds (drift):** a `pending` task that stays **unstaffed over 4h**,
  or an `in_review` task that stays **unjudged over 2h**, is recorded as drift
  and named explicitly in the escalated nudge so stalled work cannot hide. Doctor
  warns on aged unstaffed open tasks (`aged_unstaffed_task`).

### Event-driven orchestration: no manual wait loops

Do not create manual `sleep`, polling, or "wait and check again" loops while
orchestrating. The plan already owns wake-up delivery through timed heartbeats,
worker reports, and state-reconciliation nudges. Manual wait loops make the
orchestrator burn attention on one lane and can hide the fact that other tasks
are ready to dispatch or review.

Allowed checks:

- Run one immediate sanity check after a concrete event: a worker report, a plan
  reminder, a pane death, a user message, a gate finishing, or a command you just
  started returning.
- If no event happened, do not keep polling a worker. Keep the heartbeat lane map
  accurate, leave the worker assigned, and advance another independent task if
  one exists.
- If a worker appears overdue only because a plan nudge told you so, check
  `codeterm orchestrator status --plan PLAN_ID --pane ORCH_PANE --json` first.
  Inspect a worker pane at most once, then either bounce it with a concrete next
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
any task whose precondition just became true gets dispatched NOW, parked with
a recorded reason, or routed to replanning. The most common stall: a gating
task (review/verification) becomes dispatchable at the moment the last lane
integrates, while the stale focus still describes the finished wave.

A nudge is handled only if it ends in a state change for at least one open
task: dispatch / park (or a decision stride) / judge a report / bounce a
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

```bash
codeterm orchestrator heartbeat set \
  --focus "Integrate lane fixes, then cut release" \
  --lane "lane-a: CLI hardening (worker p1)" --lane "lane-b: canvas perf (worker p2)" \
  --rule "Never merge a lane before its gate is green"
codeterm orchestrator heartbeat set --clear-lanes        # wipe lanes when a phase ends
codeterm orchestrator heartbeat show                     # inspect current heartbeat
codeterm orchestrator learn "QuickJS lacks console — guard host calls"   # append a learning
codeterm --json orchestrator status --plan PLAN_ID       # reports + live_lanes + drift + next_actions
```

`set` writes only the fields you pass (omitted fields are left intact); `--clear-lanes` / `--clear-rules` empty those lists and cannot be combined with `--lane` / `--rule` in the same command. Plan is resolved from `--plan`/`CODETERM_PLAN_ID`, else the active plan owned by this pane/session. Keep the plan-owned heartbeat current after each state-reconciliation pass — the focus + lanes are what get reinjected.

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

```bash
codeterm plan task add PLAN_ID --name NAME \
  [--type implement|research|review|prototype|docs|fix] \
  [--sections id,id] \
  [--model "claude:opus"] \
  [--why "..." | --why-file PATH] \
  [--body "..." | --body-file PATH] \
  [--intent "..." | --intent-file PATH] \
  [--assign-pane PANE_ID] \
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

codeterm plan task assign PLAN_ID TASK_ID --pane PANE_ID [--mesh PEER]

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

## What lives where

| Content | Location |
|---------|----------|
| CLI mechanics (this doc) | `codeterm docs ORCHESTRATION` |
| Orchestration principle (when to spawn, model routing, discipline) | mem skill `codeterm-orchestrator` |
| Full mem reference | `codeterm docs MEMORY` |
| REST + socket API | `codeterm docs API` |
