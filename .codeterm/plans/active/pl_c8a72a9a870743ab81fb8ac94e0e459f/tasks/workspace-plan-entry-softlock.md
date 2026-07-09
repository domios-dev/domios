---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: task
id: task_b380603769604410bf4f6b3271ff020b
handle: workspace-plan-entry-softlock
title: Workspace-tab Plan button → board without an agent (fix soft-lock)
status: done
---

# Plan button on the workspace tab — third pane mode (peer to Chat/Terminal)
Andrey: every orchestrator/workspace tab must show a PLAN button to the RIGHT of Chat and Terminal. Clicking it opens the PLAN as a third pane MODE (plan-as-mode) — the canvas board for the plan that belongs to this workspace/orchestrator. So from any orchestrator I can click Plan and see/drive its board. Wire the button to the workspace's plan (the plan owned by / linked to this workspace). Fix the soft-lock (board opens even without an agent attached). This is the §30 "Plan-as-mode + composer + active" entry point. FE (tab bar + pane mode) + the workspace→plan resolution.
