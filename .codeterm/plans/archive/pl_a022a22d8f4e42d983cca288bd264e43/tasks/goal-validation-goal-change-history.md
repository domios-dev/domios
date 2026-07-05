---
plan_id: pl_a022a22d8f4e42d983cca288bd264e43
kind: task
id: task_a7af60bc22764f9f937f1b391ef9a3e1
handle: goal-validation-goal-change-history
title: Goal validation + goal-change history
status: cancelled
---

Validate the goal endpoint/CLI (reject malformed/empty, error loudly). Add a recorded goal-change history (Vec<GoalChange{ts,vector_before/after,reason}>) + CLI plan goal amend --reason. Render history later on canvas.
