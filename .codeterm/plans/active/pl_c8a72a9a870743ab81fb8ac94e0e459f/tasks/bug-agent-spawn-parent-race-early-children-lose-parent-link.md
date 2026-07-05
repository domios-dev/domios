---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: task
id: task_0e369343111841e7b2650a12f12e5bb3
handle: bug-agent-spawn-parent-race-early-children-lose-parent-link
title: 'BUG: agent spawn --parent race (early children lose parent link)'
status: done
---

INTERMITTENT: agent spawn --parent X does NOT persist parent_pane_id on the spawned pane when the child is spawned shortly after the parent is created. Evidence (live): orchestrator Euler fa0974b3 spawned 4 workers — early Orwell 931e7289 + Grieg cf5730cc got parent=None,role=None; later Hertz 09c82f36 + Vivaldi e03b5dbf got parent=fa0974b3,role=Worker. Even Euler itself (spawned --parent 073f64ae --role Orchestrator) got role=Orchestrator but parent=None. So role and parent are applied via different paths and the PARENT link is dropped for early/first spawns — likely a readiness/registration race (the parent is not yet marked can_spawn / not yet in the registry the link checks). Entry: src-tauri/src/api/agent_routes.rs:51 spawn_agent passes body.parent_pane_id into params (~:69) then calls svc.spawn_agent(params). Trace where parent_pane_id is APPLIED to the created pane + the guard/race, and fix so --parent ALWAYS links (or errors loudly, never silently drops).
