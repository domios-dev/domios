---
plan_id: pl_2e932e82ad614cdd9f08072480b0857e
kind: milestone
id: ms_5311a9121d3746559b2019581b8a03a3
handle: r4-zoom-reference-audit-for-zoom-scale-drift-and-stable-node-geometry-ms_5311a9121d3746559b2019581b8a03a3
title: status_update
---

R4-Zoom research accepted.

Root cause:
- High-confidence drift source is the per-card `.ct-canvas-crisp` wrapper in TaskCard and SectionNode: CSS `zoom: crispScale` plus inverse `transform: scale(1 / crispScale)`.
- CSS zoom performs real layout reflow: text re-wraps, fractional pixels round differently, flex/grid slots redistribute. It is not a pure paint operation and does not cancel with transform.
- `crispScale` is quantized in 0.25 steps, causing visible snap/pop while approaching the node. Slot transition animation makes the snap more visible.
- Secondary visual movement is expected from per-node W0-W3 content LOD reveals, but that should never change footprint or slot geometry.
- `substrateV2Enabled()` already bypasses this branch and uses the plain single-transform path, but the flag defaults off, so default runtime still ships the drift.

Reference survey conclusion:
- tldraw, Excalidraw, xyflow/React Flow, sigma, Pixi, Konva, Fabric, and CodeCharta keep object/world geometry under one camera matrix.
- None use per-object CSS zoom reflow as a crispness fix.
- DOM-under-transform systems accept mild GPU blur rather than layout-changing counter-scale.
- Crisp without drift is done via raster/texture caches keyed by zoom bucket, not CSS layout zoom.

Decision:
- Execute T-ZD.1 first: remove/collapse the `.ct-canvas-crisp` zoom-reflow branch and make the plain single-transform card path the default. Accept temporary deep-zoom blur as the correct tradeoff while performance/canvas-primary work continues.
- Optional later: Excalidraw-style zoom-bucket raster cache if crispness remains required without drift.
