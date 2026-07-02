---
plan_id: pl_2e932e82ad614cdd9f08072480b0857e
kind: task
id: task_008a761617084c54994b172c1b22a201
handle: ui-ux-iter13-split-canvasview-sectionnode-back-under-1000-line-limit-refactor
title: UI/UX iter13 — split CanvasView + SectionNode back under 1000-line limit (refactor)
status: done
---

Pure behavior-preserving split (no feature change). CanvasView 1073->729 (new canvasView/ hooks: models, useCanvasLookups, useCanvasFlyTo, useCanvasOverlays, useCanvasChangeSignal). SectionNode 845->252 (new cards/sectionNode/: shared + LinkageStrip/VocabularyLegend/HeartbeatStrip/LoopsStrip/SwarmPanel/SectionRowView). All public exports unchanged. Behavior-identical: real W0-W3 PNG diff <=0.0004% (W3 byte-identical), fixture-30 <=0.0029% (residual=heartbeat-dot animation only). Memos verbatim+cameraScale-decoupled, single camera transform untouched. tsc+cards 20/20+cloudFlush 9/9+swarmRoster 16/16+presenceOverlays 10/10+flyToTarget 7/7+golden pass. file-sizes clears both. Committed feat/canvas-perf-polish (unpushed).
