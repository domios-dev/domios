---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_7ca832dbdbb2480fb61fcae3a382fd0f
handle: task-detail-description-before-activity-absolute-timestamps
title: 'Task detail: Description before Activity, absolute timestamps'
status: done
---

User feedback (round-10): in the task slide-over/detail overlay the section order is wrong — ACTIVITY (execution attempts + strides) currently renders above DESCRIPTION. Correct order: header (status/priority/type chips) -> PLAN SECTIONS -> DESCRIPTION -> ACTIVITY. Also: activity entries must show explicit date+time (e.g. 'Jul 3, 18:42' — locale-aware, today can shorten to time-only), not only relative ages; keep the relative age as secondary ('Jul 3, 18:42 · 4h ago'). Apply to every task detail surface (slide-over + overlay) and the plan-document recent-activity rail if it renders timestamps. Component tests assert section order and timestamp format.
