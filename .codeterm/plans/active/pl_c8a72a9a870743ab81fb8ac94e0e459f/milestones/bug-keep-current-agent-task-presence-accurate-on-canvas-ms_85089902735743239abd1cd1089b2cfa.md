---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_85089902735743239abd1cd1089b2cfa
handle: bug-keep-current-agent-task-presence-accurate-on-canvas-ms_85089902735743239abd1cd1089b2cfa
title: status_update
---

STATUS: in_review
CHANGES: CanvasPresenceLayer.tsx, CanvasView.tsx(+layerRef world layer), presenceOverlays.ts(world anchors), presenceGlide.ts, useCanvasViewport.ts(export paintLayer), presenceOverlays.test.ts @ c04d9939 (on top of c3dd2290 exclusive-pane gating)
BUILD/TYPECHECK/TESTS: npx tsx presenceOverlays.test.ts PASS (11); taskPresence.test.ts PASS; paneCurrentTask.test.ts PASS
FOLLOWUP: Root cause of pan lag — pins used screen-space anchors recomputed only on debounced React transform (~130ms) while canvas blits via transformRef every rAF. Fix mounts presence inside layerRef; buildPresenceOverlays emits world top-right anchors; paintLayer updates layer on every gesture event. Connectors use per-pin local SVG at world anchor. Orchestrator gate: merge lane/presence-binding, dev build, pan canvas during active task assignment — pin+connector must track node with zero trail; verify reassignment still clears stale pins (c3dd2290).
