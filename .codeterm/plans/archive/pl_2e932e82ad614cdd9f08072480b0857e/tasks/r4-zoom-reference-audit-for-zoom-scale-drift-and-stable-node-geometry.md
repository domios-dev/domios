---
plan_id: pl_2e932e82ad614cdd9f08072480b0857e
kind: task
id: task_4791af2e2a8c4a56b58dde843144e3b8
handle: r4-zoom-reference-audit-for-zoom-scale-drift-and-stable-node-geometry
title: R4-Zoom — Reference audit for zoom scale drift and stable node geometry
status: done
tags:
- canvas
- zoom
- research
- substrate
---

Investigate the specific visual bug: when zooming into the canvas, node/card internals appear to change scale or transform unexpectedly. From far away it looks acceptable; while approaching, element sizes/slots/text appear to shift or rescale instead of preserving stable world geometry.

Research goal:
- Explain the correct architecture/invariant for zooming rich nodes on an infinite canvas.
- Compare CodeTerm's current approach against reference implementations.
- Recommend how to eliminate scale drift and layout changes during zoom.

Reference projects to inspect or download if not already local:
- tldraw
- Excalidraw
- xyflow / React Flow
- sigma.js
- PixiJS examples or source
- Konva
- Fabric.js
- CodeCharta or other large canvas/graph renderers if useful

Questions to answer:
- Which projects keep object/world geometry stable under one camera matrix?
- Which projects rasterize/cache/mipmap content at zoom levels, and how?
- Which projects use DOM nodes under transform, and what compromises do they make?
- How should CodeTerm keep node footprint, slots, text, and overlays stable while zooming?
- Is the observed scale drift likely caused by CSS zoom/counter-scale, React re-layout, LOD threshold swaps, transform quantization, or DOM intrinsic sizing?

Deliverables:
- A focused report with source links/file paths from reference projects.
- A concrete invariant for CodeTerm: what is allowed to change at zoom thresholds and what must never change.
- Next implementation task(s) to remove zoom scale drift, separate from general perf work.
