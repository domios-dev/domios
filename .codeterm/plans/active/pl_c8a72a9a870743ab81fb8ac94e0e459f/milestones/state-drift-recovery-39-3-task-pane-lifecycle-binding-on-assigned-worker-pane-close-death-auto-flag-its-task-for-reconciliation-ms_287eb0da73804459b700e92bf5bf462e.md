---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_287eb0da73804459b700e92bf5bf462e
handle: state-drift-recovery-39-3-task-pane-lifecycle-binding-on-assigned-worker-pane-close-death-auto-flag-its-task-for-reconciliation-ms_287eb0da73804459b700e92bf5bf462e
title: status_update
---

Implemented in snapshot a5803c81. Evidence: ServiceContext::close_pane and PTY lifecycle call flag_bound_tasks_for_gone_pane; plan store flag_pane_gone clears assignment and surfaces drift. Covered by reconcile detector tests; lifecycle path reviewed in src-tauri/src/service/pane.rs and src-tauri/src/service/pty_lifecycle.rs.
