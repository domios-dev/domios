---
plan_id: pl_2e932e82ad614cdd9f08072480b0857e
kind: task
id: task_bb87400104f24e13857324fa3ec3b2fd
handle: t-r4-2-tier1-decouple-canvas-derivation-from-transform-frames
title: T-R4.2 — Tier1 decouple canvas derivation from transform frames
status: cancelled
tags:
- canvas
- performance
- implementation
- substrate
---

Tier1 implementation from R4.

Goal: remove per-animation-frame expensive derivation from pure pan/zoom.

Scope:
- Make CanvasScene/buildCanvasScene independent of transform.scale unless semantic LOD truly changes.
- Split stable scene derivation from quantized view/culling derivation.
- Throttle or quantize culling/worldByNode/richOverlays/readback publication to a view snapshot rather than every rAF transform tick.
- Fix memo breakers such as resolveModel returning fresh spreads for plan/spec/ghost on every frame.
- Keep current visual behavior intact.

Acceptance gates:
- Pure pan/zoom does not rebuild full scene on every rAF; instrumentation from T-R4.1 shows scene rebuilds on pure pan near zero or only at LOD thresholds.
- CanvasViewInner render rate during pan/zoom is bounded and materially lower than baseline.
- Existing canvas unit tests, npx tsc, and 13-canvas Playwright goldens pass.
