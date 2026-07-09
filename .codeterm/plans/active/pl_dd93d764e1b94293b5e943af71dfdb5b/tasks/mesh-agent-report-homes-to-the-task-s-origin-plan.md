---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_2b92de1c3e244157a7dbae09771606ae
handle: mesh-agent-report-homes-to-the-task-s-origin-plan
title: 'Mesh: agent report homes to the task''s origin plan'
status: done
---

Field gap (Dali@eight, round-10): 'codeterm agent report --plan <id> --task <id>' on a mesh peer resolves the plan LOCALLY (matched a different local plan) instead of homing to the plan's origin machine; '--mesh' on report printed a pane list instead of routing. p1b landed mesh homing for plan MUTATIONS, but agent report was left out — remote workers on a shared plan cannot record milestones/reports without a human relay. Fix: agent report with explicit --plan/--task refs must resolve the plan's home peer (same homing table as p1b) and deliver the report + milestone there; local-plan fallback only when the plan is actually local. Error must be honest when the home peer is unreachable (queue or refuse with retry hint, never silently record onto a wrong local plan — that is data corruption). Add a mesh integration test with two daemons or a routing-table unit test with mocked transport.
