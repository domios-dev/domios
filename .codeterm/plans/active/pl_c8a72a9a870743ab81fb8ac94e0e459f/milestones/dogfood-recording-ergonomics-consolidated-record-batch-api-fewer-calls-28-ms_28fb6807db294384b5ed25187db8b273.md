---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_28fb6807db294384b5ed25187db8b273
handle: dogfood-recording-ergonomics-consolidated-record-batch-api-fewer-calls-28-ms_28fb6807db294384b5ed25187db8b273
title: decision
---

ERGONOMICS synth (codex): friction = 4-7 calls per provenance update. SOLUTION: one consolidated 'codeterm plan record' backed by POST /api/plans/:id/batch — ordered typed ops with client ids + $ref resolution ([upsert_task, set_task_body, add_milestone, add_element...]), ONE daemon transaction, ONE plan write, ONE plan-changed, returns all ids. Idempotency via --key or {pane,command_hash,minute_bucket}; upsert task by handle; milestone de-dupe (task,kind,key). 'plan handoff create' internally calls the same batch service. Implement batch service first/alongside handoff; expose 'plan record' as the ergonomic path used by CLI AND in-app.
