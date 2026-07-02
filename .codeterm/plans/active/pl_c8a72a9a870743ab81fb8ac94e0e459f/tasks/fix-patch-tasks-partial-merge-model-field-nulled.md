---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: task
id: task_f635a7700ea94a12ba910a754da04c76
handle: fix-patch-tasks-partial-merge-model-field-nulled
title: Fix PATCH /tasks partial-merge (model field nulled)
status: done
---

PATCH /api/plans/{id}/tasks/{tid} and socket task.update must do a PARTIAL merge: only overwrite provided fields, preserve model/why/status_label/etc. Add a test: set model, then patch parent_task_id, assert model survives.
