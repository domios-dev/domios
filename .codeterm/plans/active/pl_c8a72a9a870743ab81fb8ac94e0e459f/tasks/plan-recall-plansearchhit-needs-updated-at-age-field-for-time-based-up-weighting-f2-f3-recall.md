---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: task
id: task_995f068fc51e4800949166ab1c9fcada
handle: plan-recall-plansearchhit-needs-updated-at-age-field-for-time-based-up-weighting-f2-f3-recall
title: 'Plan recall: PlanSearchHit needs updated_at/age field for time-based up-weighting (F2/F3 recall)'
status: done
tags:
- friction
---

F2 'related work' + F3 'ghosts' both want to up-weight OLDER/stalled tasks, but PlanSearchHit (src-tauri/src/service/plan/search_index.rs) exposes no updated_at/age. Both lanes fell back to status-only filtering. Add updated_at (or age) to the hit struct + recall response so the FE can time-weight. Plan-BE lane.
