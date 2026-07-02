---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: task
id: task_7554f2cec0fc416e94f80906de699997
handle: worlds-v3-body-reveal
title: §35.3 zoom-into-task body reveal (fixed slots, content per LOD band)
status: cancelled
---

Per §35.3/§37.1: zooming into a task card reveals its content progressively in FIXED inner boxes (title/description/work-points slots that never shift). canvas-task-body must become visible at the W2/W3 band (the geometry criterion @canvas-worlds-v3 annotates this as pending; make it pass = the acceptance criterion). Content swaps per LOD band; boxes stay put; footprint stays fixed (already verified). Files: src/components/Canvas/cards/TaskCard.tsx + worlds.ts LOD bands. test_plan: the @canvas-worlds-v3 body-reveal assertion turns from soft-annotation to hard pass.
