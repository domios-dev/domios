---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_3d875c919c5b4f2ea76388c9249e5175
handle: mission-control-v2-flow-canvas-milestones-as-stations-agents-as-carriers
title: 'Mission control v2: flow canvas — milestones as stations, agents as carriers'
status: done
---

Elevate the mission-control canvas from an assembly grid to a FLOW canvas per the attached design doc + working HTML mockup (animated, open in browser). Core: each plan renders as a river — Frontier -> current MILESTONE station (glowing, named, N-of-M, powered by the task-groups/milestone-gates feature) -> Verify (QA+gate merged) -> Done pool — soft flow curves, agent avatars docked to carried chips, transition animations along the edges (reduced-motion static). Multi-plan: rivers stack (active full, others compact); NEW all-plans mode (toggle near settings) lifting the workspace filter — verified absent today (modes are workspace|focus only; needs surface-mode plumbing). TV-pleasant aesthetics per the mockup: big tabular numerals, soft glows, generous space. The mockup is the visual acceptance reference — final screenshots are compared against it. Reuses buildMissionControl + LOD/zoom + detail click-through unchanged.
