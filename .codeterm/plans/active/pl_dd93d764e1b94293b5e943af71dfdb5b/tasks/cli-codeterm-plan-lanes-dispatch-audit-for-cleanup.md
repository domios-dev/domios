---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_d1450940bf2e439496a1f320d5167279
handle: cli-codeterm-plan-lanes-dispatch-audit-for-cleanup
title: 'CLI: codeterm plan lanes — dispatch audit for cleanup'
status: pending
---

Orchestrator-requested visibility command for lane hygiene (currently assembled by hand from plan state + pane list + git worktree list). COMMAND: codeterm plan lanes <plan> [--json] [--stale]. OUTPUT TABLE, one row per task with lane metadata: task short-id + name (truncated) | status | assigned pane (+ LIVENESS: pane exists on this daemon / mesh peer / GONE) | branch (+ exists in git? merged into integration head?) | worktree path (+ exists on disk?) | last stride age. --stale filters to cleanup candidates: pane GONE, or branch merged, or worktree orphaned. Data sources all exist: plan task records (pane/branch/executor), pane list, git worktree list + branch --merged run in the plan's project cwd. Read-only, no mutations — cleanup stays a human/orchestrator decision; the command just makes drift visible in one call. Include the same info the drift reconciler uses so 'plan lanes --stale' and 'plan reconcile' agree on what is gone (shared liveness helper, not a second implementation).
