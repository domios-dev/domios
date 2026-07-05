---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_099e9a7bd56b4fc7a18e003a31588555
handle: t22-canvas-bounce-fixes
title: 'Canvas v3 bounce fixes: build import + geometry units'
status: done
---

Fix the blocking t7 review findings in the Canvas v3 lane. Scope: src/components/Canvas/canvasView/useCanvasFlyTo.ts must import CANVAS_MIN_SCALE/CANVAS_MAX_SCALE as runtime values, not import type; fix TypeScript errors in layout.test.ts and zoneGrid.test.ts; make layout/cloudFlush.test.ts and layout/layoutInputRevision.test.ts green against the intended zone-first geometry without weakening behavioral coverage. Do not redesign canvas. Keep files under size limits. Report with exact failing-before/passing-after tests.
