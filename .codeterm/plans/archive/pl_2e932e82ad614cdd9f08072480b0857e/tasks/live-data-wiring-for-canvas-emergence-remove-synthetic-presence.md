---
plan_id: pl_2e932e82ad614cdd9f08072480b0857e
kind: task
id: task_9b360c0502814692b2c7b37d77a4c70f
handle: live-data-wiring-for-canvas-emergence-remove-synthetic-presence
title: Live data wiring for canvas emergence (remove synthetic presence)
status: pending
---

The canvas emergence layer (presence pins, swarm cluster, gliding, strips, tab indicators) is built + ship-ready but runs on SYNTHETIC presence. To make it reflect the REAL swarm, the daemon owner needs to wire: (1) live pane/agent registry -> buildTaskPresence (which agent is on which task, their session/role/branch) — kills the pane-id fallback, makes pins/cluster/strips show real agents; (2) orchestrators emit heartbeat.current_focus + active_lanes -> lights up the orchestrator progress strip + board heartbeat (today degrades gracefully to progress+faces); (3) enrich the lightweight plan index (/api/plans PlanIndexEntry) with done_count (+optional lead handle) -> upgrades the tab + workspace-plan-tab hint from 'N tasks' to a true done/total pip cheaply; (4) daemon indexing of .codeterm/plans/*.json (was index=0 vs 4 files) so real project-local plans appear in the canvas at all. All four are backend/daemon, out of the frontend canvas lane.
