---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_fe24b9f2696e4ad89b8031166b136ab9
handle: planning-is-plan-state-not-a-planner-task-state-driven-planner-flow
title: Planning is plan state, not a planner task (state-driven planner flow)
status: done
---

## What

Retire the "planning task" pattern: today a Planner gets a task like "Planner: design the task graph" (this very plan has one in_progress). Planning is not a task — it is the PLAN'S OWN STATE progressing. The planner works, and when the structure is ready it moves the plan state forward; that state gate is what unlocks execution-task creation. Both agents and users must see who does what and what state allows what.

## Design

1. **State machine as the planner's progress.** Plan mode already has planning|approved|execution|complete. Add planner-visible readiness signals on the plan (not on a task): e.g. `planning_state: drafting | structure_ready | approved` (naming per implementation taste, but ON the plan). Planner advances it (`codeterm plan planning-state set structure_ready --note ...` or equivalent via plan update); every advance writes a plan-level working point. Doctor gates approval on structure_ready + quality pass.
2. **No more planning-kind bootstrap tasks.** The planning-task kind stays for genuinely delegated planning subwork, but the PRIMARY planner engagement is via planner_ref binding + plan state — remove the auto-created planner bootstrap task from promotion/onboarding flows; planner reminder nudges key off plan state (planning + planner_ref bound -> nudge the planner pane about structure gaps) instead of a fake task assignment.
3. **Orchestrator references the planner.** Orchestrator prompts/next-actions while plan is in planning: "plan is <state> — planning belongs to your Planner peer <pane>; hand feedback to them, do not author structure" (shared constant). Next-actions suggest messaging the planner, not editing sections.
4. **UI.** Plan header (board/list/canvas + document view) shows the plan state prominently (planning:drafting / structure ready / approved / execution / complete) + who the planner and orchestrator are (peer pair chips, click -> pane). The companion bubble (Wave 20) uses planner_ref directly once this lands.
5. **Docs.** ORCHESTRATION: rewrite the Planner Profile section around state-driven planning; kill instructions that create planner bootstrap tasks.

## Migration

Existing planning tasks stay valid history; new flows stop creating them. This very plan dogfoods: when this task ships, the standing planner task here gets closed with a migration note.
