---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_96a22918d7544850b2a5201cbc4b849c
handle: bug-keep-current-agent-task-presence-accurate-on-canvas-ms_96a22918d7544850b2a5201cbc4b849c
title: status_update
---

STATUS: in_review
CHANGES: CanvasPresenceLayer.tsx(unified PresencePin unit), presencePinUnit.ts+.test.ts, CanvasView.tsx(onPaintImpl+paintWorldLayer+layerRef fix), CanvasWorldCanvas.tsx(onCameraPaint), presenceOverlays.ts, useCanvasOverlays.ts @ 6d351496 (branch lane/presence-binding, commits c3dd2290+c04d9939+6d351496)
BUILD/TYPECHECK/TESTS: presencePinUnit.test PASS(4); presenceOverlays.test PASS(11); taskPresence.test PASS; paneCurrentTask.test PASS
FOLLOWUP: Pan lag root cause: viewport paintLayer targeted layerRef but a duplicate local ref was attached to the DOM in an intermediate patch — schedule() painted a detached ref while canvas blitted with live transformRef. Fixed: single viewport layerRef on canvas-world-layer div; paintLayer also runs on every canvas rAF via onCameraPaint. Stale connector-alone: separate SVG loop + glide moving only avatar — merged connector+dot+avatar into one glide span; livePresencePins gates on non-peer lead agent. Orchestrator gate: merge lane/presence-binding, dev build, pan during active assignment — no trailing lines, no orphan dashes.
