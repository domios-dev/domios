---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_5ce15b85d9864506bb03798680ab9010
handle: task-detail-ui-show-planned-executor-role-and-role-guidance
title: 'Task detail UI: show planned executor role and role guidance'
status: pending
---

USER REQUEST (round-96): task detail surfaces do not show WHO a task was planned for — the executor role and its description are invisible in the UI even though the Planner records them on every task. DISPLAY, in the task detail surface (slide-over panel / PlanDetailOverlay / DetailContent — wherever Description renders today): (1) EXECUTOR ROLE (task.executor_role — the label: backend-dev / frontend-dev / qa / release / prompt-engineer / planner...) as a labeled chip near the status/priority row; missing -> muted 'no role set' (honest, not blank); (2) ROLE GUIDANCE (task.role_guidance — the worker stance prose, e.g. 'You are a read-only Rust reviewer; do not fix, file tasks') as its own titled block 'Role guidance' between Description and Test plan, collapsible if long (same pattern as other long fields); (3) ROUTING line while there: model + model_rationale (task.model / task.model_rationale) as 'Routing: claude/sonnet — <why>' one-liner — the three fields together answer 'who was this planned for and why' in one glance; (4) Board/List cards do NOT get the guidance prose (cards are owned by the metadata design pass — only the detail surface shows it); the executor-role CHIP may also appear on cards if the metadata lane's chip row has room — coordinate, do not duplicate. DEPENDENCY NOTE: executor_role persistence was just fixed (task_67d426a8, done) and the 28-task backfill ran — real data exists to render; role_guidance has 100% coverage on open tasks (planner audit round-63).
