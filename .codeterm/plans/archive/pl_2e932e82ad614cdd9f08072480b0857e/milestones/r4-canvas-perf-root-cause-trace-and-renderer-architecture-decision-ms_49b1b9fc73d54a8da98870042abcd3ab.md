---
plan_id: pl_2e932e82ad614cdd9f08072480b0857e
kind: milestone
id: ms_49b1b9fc73d54a8da98870042abcd3ab
handle: r4-canvas-perf-root-cause-trace-and-renderer-architecture-decision-ms_49b1b9fc73d54a8da98870042abcd3ab
title: status_update
---

R4 result: done as research/architecture; no product code changed.

Root cause found by static code inspection of origin/feat/canvas@0ad81c9e:
- The viewport paint transform is partly decoupled, but each rAF still calls setTransformState and causes CanvasViewInner to re-render.
- Each frame rebuilds too much derived data: buildCanvasScene keyed on transform.scale, semanticTextForNode does plan.tasks.find per node, anchors/clones/sorts nodes+edges, whole-board culling, worldByNode, richOverlays, readback publishing, and visible DOM reconciliation.
- resolveModel returns fresh spreads for plan/spec/ghost and breaks memoization.
- Passive renderer redraws full scene in useLayoutEffect when enabled.

Why S3-S6 were insufficient:
- They reduced per-card DOM weight but did not remove per-frame scene/cull/readback work.
- Passive canvas is additive under DOM, so flag-on can pay both canvas and DOM costs.
- Overlay budget caps promoted rich overlays only; the visible base node list still scales.
- Current substrate flags default off, so default runtime can still be old heavy path while paying unconditional scene/overlay/readback work.

Decision:
- Tier1 first: decouple derived scene/culling/readback from pure transform frames. Make scene scale-independent, quantize/throttle view-derived work, memoize resolveModel, and ensure pure pan/zoom does not rebuild scene.
- Tier2 next: canvas-primary board with a bounded DOM overlay pool. Render all base nodes in one rAF-driven canvas loop reading transformRef; DOM overlays only for selected/focused rich nodes.
- Tier3 only after measurement: WebGL/Pixi/OffscreenCanvas/raster mipmap cache if 1000-node traces still miss budget.

Caveat:
- No live trace was captured by the remote worker because the live Mac-Air dev data and references were not available on that machine. Next task must create the perf harness and capture baseline locally.
