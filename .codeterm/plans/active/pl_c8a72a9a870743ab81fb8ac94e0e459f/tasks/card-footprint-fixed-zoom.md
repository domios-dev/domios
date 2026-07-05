---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: task
id: task_c18b4f507608431d93b3fed71d57e1e4
handle: card-footprint-fixed-zoom
title: Card footprint must NOT change on zoom (§22.1) — crisp-text regression?
status: done
---

# crisp-text v2: footprint-invariant on zoom (§22.1) + EXTEND crisp to TASK cards
Andrey live: (A) the SPEC/PLAN node BOX resizes when zooming — footprint must be FIXED (§22.1); the crispScale re-rasterization (counter-scale text 1/k + font-size*k) must be visually identity, so a size change means the box isn't compensated → fix so the node footprint is invariant in WORLD units across all zoom. (B) TASK cards text is STILL BLURRY — the first crisp pass only covered SectionNode (spec/plan); TaskCard was intentionally out of scope. Now EXTEND the same crispScale re-rasterization to TaskCard (and StrideSnake text if blurry) so task text is sharp at deep zoom too.
INSPECT: src/components/Canvas/{CanvasView.tsx, cards/SectionNode.tsx (the box-resize), cards/TaskCard.tsx (extend crisp), worlds.ts, nodeRegistry.tsx, layout/taskFootprint.ts}. crispScale already exists on NodeRenderArgs — apply it in TaskCard, and fix the SectionNode footprint growth. Verify: node box identical world-size at 100%/200%/400%; task + spec + plan text all crisp.
