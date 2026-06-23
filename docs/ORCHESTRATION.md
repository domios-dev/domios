# Orchestration Toolset — Plans & Canvas

<!-- revision: 1 -->

Full mechanical reference for the `codeterm plan` CLI. Mechanics only — the *why* (when to spawn, model routing, discipline) lives in the `codeterm-orchestrator` mem skill.

## Model

```
Plan
 └─ Sections[]          structured spec tree — auto-split from headings
     └─ Tasks[]          each task ⊆ one or more sections (many-to-many)
         └─ Milestones[] report chain — one node per `agent report --plan --task`
             └─ links[]  chain edges; loop-back edges form self-improving cycles
```

- **Sections** form a tree (`--parent` for nesting). Each section is a canvas node.
- **Tasks** attach to sections via `--sections`. Coverage is derived (no tasks → uncovered; done tasks → covered).
- **Milestones** are the persisted projection of `codeterm agent report --plan/--task`. Authorship is auto-resolved from the caller's pane env (`CODETERM_PANE_ID`).
- **`milestone_kinds`** declared at plan creation define the column vocabulary (e.g. `status_update,testing,review`). A milestone must use a declared kind — the daemon returns a teaching error on mismatch.
- **Loops** (`LoopSpec`) express declared self-improving cycles: a target + ordered stages + budget. Each iteration appends milestones tagged with `loop_id` + `iteration`. `plan task add --loop-from` is the lightweight single-continuation shortcut.
- **Task env:** when a task is assigned to a pane, the daemon exports `CODETERM_PLAN_ID` and `CODETERM_TASK_ID` into it. Workers report without being told the ids: `codeterm agent report --plan $CODETERM_PLAN_ID --task $CODETERM_TASK_ID`.

## Lifecycle

```
plan create                → orchestrator pane gets CODETERM_PLAN_ID
  plan kinds set           → declare milestone columns (or use --kinds on create)
  plan section add         → build the spec tree
  plan task add --model --why  → add tasks with routing rationale
  plan task assign --pane  → bind a pane (exports CODETERM_TASK_ID into it)
  ...workers run, report via agent report --plan --task...
  plan milestone add       → manual milestone (orchestrator checkpoints)
  plan task close          → mark task done, attach final result
  plan loop create         → declare a self-improving cycle on a task chain
  plan archive             → done; hidden from canvas
```

Edit while work runs — `codeterm plan ...` handles mutate a running plan safely. Re-orient with `plan get PLAN_ID` (markdown) at any time.

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
  [--status active|archived]

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

codeterm plan section rm PLAN_ID SECTION_ID
```

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
  [--loop-from TASK_ID]

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
```

`--type` is an open string — the listed values are hints for icon/color only. `--model` and `--why` record routing rationale on the task (not enforced, used for canvas display).

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
