---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_ad78e4efd3e54f7cad2d795d31879fd3
handle: decision-gate-productize-remote-qa-builders
title: 'DECISION GATE: productize remote QA builders?'
status: pending
---

Carrier task so the question survives the plan (the internal gate-runner shipped via the closed builders task; its body holds the full staged product design — registry federation, plan qa --builder routing, detached-fetch semantics, serial queues, structured results). QUESTION FOR ANDREY, post-release: should the remote-builder flow become shipped product surface, and in what scope (registry+routing only vs full auto-pick+artifacts)? INPUTS to weigh when opening: lessons from running the internal scripts through the release endgame (friction log), mesh security audit implications of a gate-execution channel, and whether plan qa local UX proved itself. DO NOT BUILD from this task — on decision, spin a properly scoped design/build effort (likely its own plan).
