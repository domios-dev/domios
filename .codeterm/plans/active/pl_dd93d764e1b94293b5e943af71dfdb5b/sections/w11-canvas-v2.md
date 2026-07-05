---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: section
id: sec_53a3e6a381834915b7fc30793201009e
handle: w11-canvas-v2
title: Wave 11 — Canvas zoom + density (round 2)
---

Wave 11 (round-2): Canvas zoom range + density.

Scope (spec: Canvas round-2 block):
- Zoom clamps: max 400%, min 10% (5% only if bounds math keeps the constellation readable), useCanvasViewport.ts.
- Bounds/packing recompute: tighter per-section task packing (measured bounds, grid/cloud packing), done-stacks retained — zoomed-out plan reads as a compact constellation, not sparse dots; canvasRender layout + canvasSpatialIndex.
- Re-tune LOD thresholds to the new range (cards -> cluster chips -> section summary); no dot-cloud at any zoom; minimap consistent.
- Frame budget re-verified on the 200-task fixture (p95 ~16ms), numbers in report.
File ownership: useCanvasViewport.ts (+test), canvasRender/drawScene.ts, drawCard.ts, layout/packing module, canvasSpatialIndex.ts, CanvasMinimap.tsx, perfFixture.
Verification: npm run test:unit; manual pan/zoom sweep 10%-400% with frame probe; screenshots at 10/50/100/400%.
