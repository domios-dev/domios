---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: task
id: task_3835a7208a0440928428e2f3fcad10f9
handle: worlds-dive
title: 'Worlds DIVE: deep zoom (to 1000%) reveals progressively more (title→tags→body→strides→into-stride)'
status: cancelled
---

Andrey: NO support for the worlds vector — zooming should dive in and in, each deeper band revealing MORE (§37.2 granularity). Currently deep zoom just enlarges a card with no new detail (and reflows/garbles). Implement progressive reveal across the FULL range now up to 1000% (maxScale 10): far=title+status, closer=tags+desc, deeper=full body, deeper=strides/work-points, deepest=into a single stride filling the screen. Frame FIXED (§22.1), content adapts, boxes don't shift (§37.1). acceptance_criteria: at each band boundary new detail appears; footprint constant across all bands; no text garble. test_plan: band-spanning @canvas-worlds-v3 across fit→1000%.
