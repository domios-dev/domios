---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_6cc630997cb4428cbc955cece7af74cd
handle: worlds-zoom-model-unify-20-23-7-25-into-one-lod-ladder-26-ms_6cc630997cb4428cbc955cece7af74cd
title: testing
---

Shipped §26 WORLDS: replaced the single global contentNear raw-scale flag with a per-node W0-W3 band driven by appW = nodeFootprintWorldWidth × cameraScale (±10% hysteresis, wider thresholds for SPEC). New worlds.ts (nextWorld + thresholds). All spec/plan/task cards now world-driven.
