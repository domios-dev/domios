---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_6924326a5a394dd681d5857bbac8a596
handle: worlds-zoom-model-unify-20-23-7-25-into-one-lod-ladder-26-ms_6924326a5a394dd681d5857bbac8a596
title: decision
---

WORLDS synth (opus 56c1e70f, 1/3 reported). ROOT BUG located in code: CanvasView.tsx:273 contentNear=scale>=1.3 is ONE global raw-scale flag (anti-§25.1); SectionNode.tsx:133 hard-gates plan bodies via !isPlan. FIX: kill global flag → per-node band off appW=bboxW*scale; one ladder W0 mass/W1 structure/W2 CONTENT(bodies reveal)/W3 deep for PLAN+SPEC+TASK alike. Reveal scales: spec(440px)~0.73, plan(300)~1.07, task(252)~1.27.
