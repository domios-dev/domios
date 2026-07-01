# Orchestration Toolset — Plans & Canvas

<!-- revision: 10 -->

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
4. Run a plan self-review / doctor pass.
5. Approve the plan, then generate execution tasks.

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

Use `codeterm plan doctor PLAN_ID --kind quality` before approval and again
before dispatch. It is a structural approval gate: missing goal/spec link,
task-quality gaps, heartbeat-before-execution state, and tasks created while
still in planning mode are errors. It deliberately does **not** infer quality
from prose keywords; if a rule must be machine-enforced, model it as structured
planner state instead of text heuristics.

Heartbeat is runtime execution state, not a planning scratchpad. Plan-owned
nudges are automatic once an assigned orchestrator owns an execution-mode plan
with unfinished work. Manual heartbeat focus/lanes/rules are only overlays that
make the automatic nudge more precise; they are not a prerequisite. Workers get
task nudges only while their assigned task is still waiting for their report.

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
## Heartbeat — auto-reinjected orchestrator state

The orchestrator **heartbeat** is durable state attached to the plan (`current_focus`, `active_lanes`, `rules`, plus appended `learnings`). The daemon **auto-prepends** the state-reconciliation envelope to every plan nudge for an execution-mode plan with unfinished work, even if you never manually set focus/lanes. If focus and lane map exist, they are prepended too — so active orchestration survives compaction and idle drift without a manual self-ping.

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
codeterm plan archive PLAN_ID
codeterm plan delete PLAN_ID
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
codeterm plan loop stop LOOP_ID
```

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
