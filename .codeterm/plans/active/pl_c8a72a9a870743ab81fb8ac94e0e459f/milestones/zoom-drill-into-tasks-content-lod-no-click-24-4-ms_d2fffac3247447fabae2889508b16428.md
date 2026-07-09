---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_d2fffac3247447fabae2889508b16428
handle: zoom-drill-into-tasks-content-lod-no-click-24-4-ms_d2fffac3247447fabae2889508b16428
title: decision
---

§25 synthesis (opus 5f5ac58f + cursor 9376b50e converged). PICKS: (1) LOD driver = appW (apparent on-screen size = cellWidth_world × cameraScale), NOT raw camera scale — each node bands by its own readable size. (2) RESERVE-AT-DEEPEST packing: cell sized at deepest-LOD bbox, content-LOD only changes what is painted → footprint fixed (§22.1), zero reflow, overlap structurally impossible. (3) Two orthogonal mechanisms: BAND (paint, zoom-only-per-node) ⊥ DOMINANCE (focus-assist, viewport-relative, elevate+dim, no footprint change, no auto-pan). Double-click stays the committed/locked twin.
