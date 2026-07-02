---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: section
id: sec_0c038de776d041088825dee7e59d8bae
handle: w8-replan
title: Wave 8 — Feedback-driven replanning loop
---

Wave 8 (standing loop, not a one-shot): feedback-driven replanning.

Contract:
- Trigger conditions: (1) a worker reports blocked with a structural cause (e.g. shared overlay API does not fit a view, REST data missing), (2) Wave 6/7 evidence shows the design reads badly (screenshots fail the human eye test), (3) the user gives new feedback on any view.
- On trigger, the Orchestrator does NOT improvise large redesigns inline. It collects: current screenshots (Playwright artifacts or user-provided), the failing task reports, and a one-paragraph statement of what reads wrong, then spawns a Planner (claude --model fable, fallback opus, --role Planner, brief via --task-file) pointed at this plan id to revise spec sections and regenerate/adjust tasks.
- Small tactical fixes (spacing, copy, a broken selector) stay with workers; anything that changes a requirement or adds/removes a task graph node goes through the Planner.
- Each loop iteration ends with plan state + doctor re-run and a fresh Orchestrator dispatch decision. Budgeted via the plan loop (target: user accepts visuals; budget: 3 iterations before a mandatory user conversation about direction).
