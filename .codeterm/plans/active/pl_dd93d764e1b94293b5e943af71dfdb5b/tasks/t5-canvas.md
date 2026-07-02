---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_3e3c1a9ffb6c4866b4363b3528dd644e
handle: t5-canvas
title: Canvas legibility + performance (LOD, zoom floor, done compression)
status: pending
---

Canvas legibility + performance: kill the dot-cloud overview and bound frame cost. Read spec sections canvas-req + done-req first. Renderer-side only — no REST/data-layer changes, no new canvas features.

Steps:
1. Zoom floor + LOD: in useCanvasViewport.ts clamp min zoom; in canvasRender/drawScene.ts switch below a readability threshold to per-section cluster chips (section title + task counts by status + presence dots) instead of tiny cards. Use canvasSpatialIndex.ts for cluster buckets.
2. Done compression at normal zoom: done tasks render as a per-section done-stack marker (count badge); clicking it opens the section or detail overlay. Align hit-testing (hitTest.ts + tests).
3. Frame budget: extend perfFixture.ts with a 200-done + 10-active plan; use useCanvasFrameProbe.ts to measure pan/zoom p95 frame time before/after; record numbers.
4. CanvasMinimap.tsx stays consistent with LOD world.
5. Unit tests: drawCard.test.ts, drawScene coverage as feasible, hitTest.test.ts, useCanvasViewport.test.tsx.

Commands: npm run test:unit; ./build.sh dev then manual pan/zoom on the fixture; quote frame-probe numbers and check ~16ms p95; watch dev log for [LONGTASK].
Report: before/after frame numbers, LOD threshold chosen and why, screenshots of overview LOD, test output.
