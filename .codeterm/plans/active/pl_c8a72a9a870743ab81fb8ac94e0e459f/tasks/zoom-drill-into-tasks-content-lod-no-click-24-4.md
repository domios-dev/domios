---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: task
id: task_5d88084d727444da9d2d46e438bf0e5b
handle: zoom-drill-into-tasks-content-lod-no-click-24-4
title: Zoom-drill into tasks (content-LOD, no click) §24.4
status: done
---

Zoom-drill into tasks, no click (§24.4 → §25). LOD driven by appW (apparent on-screen px), reserve-at-deepest packing so footprint is fixed and content-LOD only repaints. L0 glyph / L1 thread / L2 cards / L3 reading bands (appW 150/320/560 @280px cell). BAND (paint) orthogonal to DOMINANCE focus-assist (elevate+dim when task fills ~60% viewport, no auto-pan, lockless). Hysteresis ±10%, no reflow. Playwright: fixed footprint ±2px across bands, LOD changes per band, node-only overflow scroll. Spec: design §25.
