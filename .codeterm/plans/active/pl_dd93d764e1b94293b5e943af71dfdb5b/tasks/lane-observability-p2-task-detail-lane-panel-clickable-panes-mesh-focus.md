---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_c87cf9c0784b4095bd52164262cb5a5a
handle: lane-observability-p2-task-detail-lane-panel-clickable-panes-mesh-focus
title: 'Lane observability P2: task-detail lane panel, clickable panes, mesh focus/resume'
status: pending
---

Builds on the P1 lane (task_03bf8da6 — Working-on row + recording discipline must land first; this lane consumes the fields P1 populates). SCOPE: (1) LANE PANEL in the task detail surface (slide-over/overlay): one structured block showing assigned pane (title + short-id + liveness), agent provider + model, mesh peer/machine, workspace, branch + worktree path, dev port when known (from the workspace record), watchers attached (panes with 'watch:' title convention targeting this task), last report excerpt + age (latest status_update stride), execution attempts history (executor history already exists — render it). (2) CLICKABLE NAVIGATION: pane chip click -> focus the pane locally (existing focus action); for a mesh pane -> best-effort: if the peer exposes focus via relay use it, else show the peer+pane id with a copyable 'codeterm pane focus --mesh' command — do NOT block on full remote focus parity. (3) CLOSED/STALE AFFORDANCE: when the recorded pane no longer exists: 'closed' state with a resume path — link to codeterm sessions search/resume for that pane's session when the provider store still has it (sessions resume is atomic already); otherwise plain stale marker. (4) DATA-MODEL DELTAS (only what P1 proved missing — expected small): last_activity_at per lane if stride-derivation proves too slow for the panel; watcher list is derived from the title convention + target task id in the watcher's command line, NOT a new registry table unless derivation fails in practice. API: extend the task detail payload; no breaking changes to list payloads.
