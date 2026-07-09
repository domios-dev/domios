---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_d67ed81867f34f74802381befc8c61de
handle: worlds-v3-fix-w2-inner-slot-stability-at-scale-3-after-deep-zoom-body-reveal-passes-ms_d67ed81867f34f74802381befc8c61de
title: decision
---

Cursor partial: focused @canvas-worlds-v3 still fails W2 name.x stability at scale 3 after DriftBadge/header cleanup. Measured drift is caused by per-card crisp zoom quantization (e.g. W2 crisp 2.5 -> 3.0) shifting the title slot. This overlaps the Figma-grade substrate plan pl_2e932e82: D1/S0/S1 already identify per-card crispScale as the offender and prove substrateV2 removes .ct-canvas-crisp with fixed footprint/crisp text. Orchestrator decision: do not pursue broader DOM W2 hacks; block/defer this task on substrate integration, keeping only independent layout cleanup after review.
