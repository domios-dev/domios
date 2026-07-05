---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_ddfe00d6902446399bb6c5d7bca1b44c
handle: planner-lane-ms_ddfe00d6902446399bb6c5d7bca1b44c
title: status_update
---

PARTIAL: Lane-cleanup hygiene planned (user request).
PLACEMENT DECISION (the 'where'): the right home is the EXISTING heartbeat/state-reconciliation envelope — heartbeat.rs already carries one generic 'clean finished lane worktrees' line; it becomes DATA-DRIVEN: when a plan has cleanup debt, the nudge lists each stale lane concretely (handle, pane, worktree, branch + exact --cleanup-status done command); no debt => current one-liner. Reinforced at three more moments: task close/done|cancelled prints the lane cleanup line while context is fresh; plan doctor warns per stale lane (warning, never error); plan archive prints outstanding debt (non-blocking).
DEBT DEFINITION: done/cancelled task with cleanup_status != done OR live assigned pane OR recorded worktree still on disk; explicit deferral silences the reminder but stays visible as info; removal advice only for integrated lanes (mirrors existing ORCHESTRATION rule).
GRAPH: spec section cleanup-hygiene + wave w0e-cleanup + task t0e-cleanup-reminders (task_97337ec04aca4512be2133fbf98541e1, sonnet, p2, wave0, spec-linked). Stride recorded.
NEXT: t0e is dispatchable to any free rust worker after current wave-0 lanes (e4f7a80c, 1663936d) land — it touches the same service/plan area, sequence to avoid conflicts.
