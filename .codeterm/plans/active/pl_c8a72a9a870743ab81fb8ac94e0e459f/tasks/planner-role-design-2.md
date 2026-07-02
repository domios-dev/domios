---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: task
id: task_d18e450dc68547ee8fd539dd7b824b57
handle: planner-role-design-2
title: Design first-class Planner role and task role abstraction
status: done
tags:
- 1.6.2
- followup
---

Design how Planner differs from Orchestrator: Orchestrator owns execution and acceptance; Planner authors/updates GOAL, Spec, Plan, tasks, role prompts, sequencing, and reports the completed plan back. Reuse the same role DTO abstraction as Orchestrator roles where possible. Avoid overbuilding persistent custom role templates unless the design proves it necessary. Include how tasks carry free-form execution role guidance and how Orchestrator spawns workers using that task role.