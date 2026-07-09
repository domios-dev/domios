---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_2214c60d4d2c4231942efcbc414fa34e
handle: group-containment-one-visual-block-per-orchestration-p0-split-ms_2214c60d4d2c4231942efcbc414fa34e
title: decision
---

ADDITION (round-35b, user, verbatim intent): DELETE the tiny green activity dots on panes entirely — the quiet 5px TabActivityIndicator dots (src/components/Sidebar/TabActivityIndicator.tsx, ct-tab-activity-quiet, orchestrator-accent + presence-color variants) that signal 'this pane took a task/plan'. User verdict: meaningless noise, looks broken. Remove the component usage from tab rows (and the component + its test if nothing else consumes it — grep first); the task/plan attribution story is already carried by the composer strips (Orchestrating/Planning/Working on), the group containment, and the task line under working rows. No replacement indicator. Criterion addition: zero activity dots render on any tab row (component assertion + grep proves TabActivityIndicator unused or deleted).
