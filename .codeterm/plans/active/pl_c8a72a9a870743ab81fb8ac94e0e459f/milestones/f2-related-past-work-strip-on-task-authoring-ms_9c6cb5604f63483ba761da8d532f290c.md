---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_9c6cb5604f63483ba761da8d532f290c
handle: f2-related-past-work-strip-on-task-authoring-ms_9c6cb5604f63483ba761da8d532f290c
title: testing
---

ACCEPTED on review. RelatedWorkStrip.tsx (new, self-contained): debounced 300ms, AbortController inflight-cancel+unmount cleanup, up-weights cancelled/blocked/stalled/abandoned, reuses existing GET /api/mem/search (F1 index, no new index), errors swallowed, hidden <4 chars/no project, click chip -> canvasFocusStore.openFocus. tsc clean (7 pre-existing plugin errors unrelated). Wired into CanvasComposer.tsx (only real authoring surface; not supervisor's) + CanvasView.tsx (+1 line projectPath prop = trivial rebase vs supervisor's uncommitted CanvasView at integration). @2c32e7c6 on canvas/f2-related-work.
