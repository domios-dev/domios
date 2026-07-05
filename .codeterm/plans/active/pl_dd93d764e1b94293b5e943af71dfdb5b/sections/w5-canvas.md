---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: section
id: sec_5912f5ccab8642c9b483a0b8535c485f
handle: w5-canvas
title: Wave 5 — Canvas simplification and performance
---

Wave 5: Canvas simplification and performance — tertiary view. Independent of Waves 3-4; can run in parallel after Wave 1.

Scope (legibility + perf only, no new features):
- Zoom floor + LOD switch: below a readability threshold, render per-section cluster chips (title + counts + presence dots) instead of individual cards; no dot-cloud state. Touch points: canvasRender/drawScene.ts, drawCard.ts, useCanvasViewport.ts (zoom clamp), canvasSpatialIndex.ts (cluster buckets), palette/textMetrics as needed.
- Done compression: done tasks render as a per-section done-stack marker at normal zoom (expand on click via flyTo or detail overlay), aligning with the done-heavy spec.
- Frame budget: use canvasRender/useCanvasFrameProbe.ts to assert p95 frame time on the 200-task perfFixture; record before/after numbers in the task report.
- Minimap sanity: CanvasMinimap stays consistent with the LOD world.

File ownership: src/components/Canvas/canvasRender/*, useCanvasViewport.ts, CanvasMinimap.tsx, perfFixture.ts (extend to 200-done fixture), unit tests beside each changed module (drawCard.test.ts, hitTest.test.ts, useCanvasViewport.test.tsx).
Verification: npm run test:unit; manual dev pan/zoom on fixture with frame probe numbers quoted in the report; no [LONGTASK]/[SLOW-IPC] regressions in dev log.
Hard boundary: no changes to plan data fetching or REST; renderer-side only.
