---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: task
id: task_403d983a71a44817a87449f41910823b
handle: persist-task-executor-history-and-respawn-workers-from-it
title: Persist task executor history and respawn workers from it
status: done
---

Implement first-class execution history for plan tasks.

Problem:
- Closing/cleaning a worker must clear live presence.
- The task must still retain who worked on it, so an orchestrator can respawn the same/similar worker with useful context later.
- Today assigned_pane_id/lane.pane_id are live fields, and durable report ledger has worker info, but task history does not have a first-class executor snapshot.

Scope:
- Separate live assignment from historical executor snapshots.
- On spawn/assign/report/in_review/close, persist executor metadata: worker pane id, title/name, provider/model/session, role, peer/mesh origin, branch/worktree, report id/status, timestamps, plan_id/task_id.
- Keep assigned_pane_id/lane.pane_id as live-only and clear them on terminal/cleanup.
- Add CLI/API support to inspect task executors and respawn/reopen from a prior executor snapshot when more work is needed.
- Canvas/task detail should show historical executor info without rendering it as active presence.

Acceptance:
- Live presence disappears when a worker pane is closed or task is terminal, while task history still shows the prior executor.
- A task can be reopened/continued by respawning from a saved executor snapshot.
- Reports/ledger/task history agree on plan_id/task_id/worker identity.
- No done/cancelled task appears active on Canvas.

Test plan:
- Rust tests for report -> executor snapshot persistence.
- Rust tests for task close cleanup preserving history.
- Rust tests for pane cleanup clearing live fields only.
- CLI/API tests for respawn-from-history command payload.
- Frontend tests for Canvas/detail rendering historical executor separately from active presence.

Correctness vector:
Live assignment and historical execution are distinct data models: cleanup only mutates live fields; history is append-only or snapshot-preserving.
