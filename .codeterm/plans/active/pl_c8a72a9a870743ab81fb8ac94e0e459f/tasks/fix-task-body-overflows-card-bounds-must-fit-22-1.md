---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: task
id: task_34f8713ec73c4008b03a4923be32499a
handle: fix-task-body-overflows-card-bounds-must-fit-22-1
title: 'FIX: task body overflows card bounds (must fit, §22.1)'
status: cancelled
---

Per §22.1: a node's bounds are fixed; content inside MUST fit (clamp/ellipsis/internal-scroll per view mode), never bleed past the card. Fix task-card body overflow; add a no-overflow assertion.
