---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_f1b7971f23634d489a91ea8e59ee407a
handle: planner-lane
title: 'Planner: design Board/List/Canvas UX redesign task graph'
status: done
---

Planner-profile lane (re-added after plan approval; the original pre-approval task was removed to clear the tasks_before_approval gate — its recorded decision is preserved below; future planning lanes use --type planning once Wave 0 lands).

Preserved decision (originally ms_5f9f5a1f0cfd4038a6b2a26ed7b8218f): Bootstrap Planner work should be represented as task kind=planning. Planning tasks are allowed while plan mode is planning and exist only to improve GOAL/Spec/Plan/task graph; execution/review/build tasks remain blocked until plan approval/execution. Planner must include this product/CLI improvement in the generated task graph.

Scope: study current Board/List/Canvas implementation + user evidence screenshots, then author the full CodeTerm planner state: refined GOAL, spec sections (evidence, IA/view priority, per-view requirements, shared detail surface, done-heavy behavior, plan/spec context, spec entity presentation, acceptance gates, non-goals, planning-kind tasks), plan sections (Waves 0-8), M:N spec links, and this executable task graph.
Deliverable: durable planner state + in_review report with plan state/doctor results and next Orchestrator action.
