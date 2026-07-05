---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_30b1397607c6454b90973b9db69ee7df
handle: t8-replan-loop
title: Replanning checkpoint (standing feedback loop)
status: cancelled
---

Standing replanning checkpoint — NOT a build task. This task stays open across the effort and is the Orchestrator's contract for feedback-driven iteration.

Trigger conditions:
1. Any worker reports blocked with a structural cause (API misfit, missing data, spec contradiction).
2. Wave 6/7 evidence (screenshots) fails the human eye test, or the user gives new feedback on any view.

On trigger the Orchestrator MUST NOT improvise large redesigns inline. Instead: collect current screenshots (Playwright artifacts or user-provided), failing task reports, and a one-paragraph statement of what reads wrong; spawn a Planner (codeterm agent spawn claude --model fable --role Planner --task-file <brief>, fallback --model opus) pointed at plan pl_dd93d764e1b94293b5e943af71dfdb5b to revise spec sections and adjust/regenerate tasks; then re-run codeterm plan state + codeterm plan doctor --kind quality and re-dispatch.
Small tactical fixes (spacing, copy, selector) stay with workers — no Planner round-trip.
Close this task only when the user accepts the visuals (gate 4) or explicitly ends the effort.
