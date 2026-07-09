---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: task
id: task_a737c12733e3434788422ebc0c0ba61e
handle: plan-recall-plansearchhit-needs-updated-at-age-for-time-up-weighting-f2-f3
title: 'Plan recall: PlanSearchHit needs updated_at/age for time up-weighting (F2/F3)'
status: cancelled
tags:
- friction
---

F2 related-work + F3 ghosts both want to up-weight older/stalled tasks but PlanSearchHit (service/plan/search_index.rs) has no updated_at/age; fell back to status-only. Add the field to hit struct + recall response. Plan-BE lane.
