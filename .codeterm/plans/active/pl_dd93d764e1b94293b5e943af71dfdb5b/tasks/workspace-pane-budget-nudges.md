---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_4d649dfdbc844cee88baadb78ba8092b
handle: workspace-pane-budget-nudges
title: Workspace pane budget nudges
status: done
tags:
- workspace
- hygiene
- ux
---

User-visible problem: workspaces accumulate too many stale panes, especially after orchestration waves. Opening another pane still returns a plain OK, so the user gets no local pressure to clean up.

Implement a workspace pane-budget guard that is advisory, not blocking:
- Soft budget: 15 panes per workspace.
- Strong threshold: over 20 panes in the workspace.
- On any pane creation path inside a workspace (plain pane, agent spawn, orchestration/pair spawn where applicable), compute the post-create workspace pane count.
- If the count is above the soft budget, include a visible cleanup nudge in the command/API/UI response.
- If the count is above 20, the response must not be a plain OK; it must explicitly say something like: "Workspace now has N panes. Are you sure you did not forget to close finished panes?"
- Do not block creation; do not close panes automatically.
- Keep prod/dev safety intact and do not count panes from other workspaces.
- Prefer one shared pane-count helper so CLI/socket/UI paths cannot drift.
- Add enough metadata for future UI to show the same nudge consistently.
