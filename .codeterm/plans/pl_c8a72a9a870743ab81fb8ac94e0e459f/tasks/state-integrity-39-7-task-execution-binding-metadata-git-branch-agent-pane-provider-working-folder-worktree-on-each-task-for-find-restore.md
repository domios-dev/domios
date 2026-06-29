---
tags:
- state-integrity
---

Task model only has model+parent_task_id — no record of WHERE/WHO works it. Add execution-binding fields: git branch, agent (pane id + provider), working folder (worktree path); set at assign/dispatch + on worker report; show in 'plan get'/§39.6 view + on the canvas card. So an orchestrator (or a recovering one) can instantly find the branch/agent/folder for any task = faster integration, restore, sweep. Ties to §39.3 (task<->pane lifecycle) — lifecycle events update these.