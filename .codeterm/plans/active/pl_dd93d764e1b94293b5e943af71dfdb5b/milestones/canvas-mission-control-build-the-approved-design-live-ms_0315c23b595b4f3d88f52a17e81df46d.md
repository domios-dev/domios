---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_0315c23b595b4f3d88f52a17e81df46d
handle: canvas-mission-control-build-the-approved-design-live-ms_0315c23b595b4f3d88f52a17e81df46d
title: testing
---

VISUAL GATE FAILED (user live dogfood, dev d4db9cb2): (1) canvas PAN IS BROKEN — user cannot move the viewport at all; (2) severe lag on interaction. Daemon log clean of SLOW-IPC => frontend-side: suspect per-frame model rebuilds / non-memoized selectors / rAF churn in the live mission-control wiring, and a pointer-event handler swallowing pan gestures. Task bounced in_review -> in_progress; fix forward in lane/canvas-mission-control-live.
