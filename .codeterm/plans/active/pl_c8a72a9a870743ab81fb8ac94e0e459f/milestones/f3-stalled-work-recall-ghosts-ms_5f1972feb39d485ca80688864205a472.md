---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_5f1972feb39d485ca80688864205a472
handle: f3-stalled-work-recall-ghosts-ms_5f1972feb39d485ca80688864205a472
title: testing
---

ACCEPTED on review. GhostRecall.tsx (new) + App.tsx 2-line wiring (collision-free). Debounced 400ms on canvas-focus change, reuses /api/mem/search (F1 index), filters abandoned statuses, sessionStorage per-card dismiss, click->openFocus, fails silent, pointer-events scoped. Imports Canvas/plansApi+canvasFocusStore READ-ONLY (no edits). tsc clean. @e6a171d on canvas/f3-ghosts. GAP flagged: PlanSearchHit has no updated_at/age -> status-only filtering (no 'older' up-weight) until BE adds the field.
