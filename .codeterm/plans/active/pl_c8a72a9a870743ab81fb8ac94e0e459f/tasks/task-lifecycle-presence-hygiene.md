---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: task
id: task_988387cc141141cdb82a0a163ec2576a
handle: task-lifecycle-presence-hygiene
title: L3 task lifecycle and presence data hygiene
status: done
---

Fix done/cancelled tasks retaining assigned_pane_id or lane.pane_id. plan task close must clear live assignment; frontend presence/live-agents must ignore done/cancelled tasks everywhere. Reconcile must not treat empty-string pane ids as closed panes. Done tasks must never appear active or show stale ghost pins/bubbles.
