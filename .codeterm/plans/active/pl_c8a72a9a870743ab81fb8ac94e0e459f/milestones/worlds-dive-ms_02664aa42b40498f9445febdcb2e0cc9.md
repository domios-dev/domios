---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_02664aa42b40498f9445febdcb2e0cc9
handle: worlds-dive-ms_02664aa42b40498f9445febdcb2e0cc9
title: testing
---

body-reveal diagnosis — DIAG showed cards:[] at the in-place deep-zoom point: zooming toward plan-center culls the right-side task cloud from the DOM. Centering the probe card (panBy) brings cards back but canvas-task-body visible() STILL false at scale 1.6-5. So it's a TEST-HARNESS issue (reaching a task at W2 with body in view), likely NOT a render bug — the worlds-dive TaskCard body renders at showBodySlot=atLeast(W2). NEXT: fresh focused harness pass — center+zoom a single task to W2, instrument data-world + body computed-style/height; the feature is deployed & likely correct
