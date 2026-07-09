---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_a8e20e06e3474ad7975f13709e52daa8
handle: t0c-ownership
title: 'Planning ownership: docs, prompts, CLI nudge'
status: done
---

Encode the Orchestrator/Planner planning-ownership boundary in docs, prompts, and a CLI nudge (observed failure: Orchestrator started mutating the task graph while a Planner lane was active).

Steps:
1. docs/ORCHESTRATION.md: add a "Planning ownership" subsection under Planner Profile with the contract: while a Planner owns an open planning task, requirements/blockers/evidence/corrections are routed to the Planner as planning input; Orchestrator = dispatch/gate/integrate + fact recording (strides, heartbeat, lane metadata); graph synthesis by the Orchestrator only when no Planner is assigned or as recorded emergency repair; Planner reconciles foreign mutations (keep/rewrite/remove) with a correction stride. Bump revision tag.
2. Prompt templates in src-tauri/prompts/ (orchestration_toolset.md + orchestrator/planner/worker seeds that state role remits): add the same boundary language.
3. CLI nudge: plan/spec mutation commands (plan section add/update, plan task add/update/rm, plan goal set, spec section add/update) print a one-line stderr hint when the target plan has an open planning-kind task assigned to a pane other than the caller's: "hint: plan <id> has an active Planner lane (<task handle>); route planning input there or record a stride." Informational, never blocking, suppressed for the planner pane itself.
4. Tests: unit test for the nudge trigger (planning-kind task open + different pane => hint; same pane or no planning task => silent); docs revision-tag bump verified.

Commands: ./build.sh check; single narrowed cargo test; manual: from a non-planner pane run plan section add on a plan with an active planning lane, observe hint.
Report: doc/prompt diff summary, nudge trigger semantics, test output.
