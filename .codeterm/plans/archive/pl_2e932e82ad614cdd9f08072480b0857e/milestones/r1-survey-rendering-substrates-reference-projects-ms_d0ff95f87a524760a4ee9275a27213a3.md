---
plan_id: pl_2e932e82ad614cdd9f08072480b0857e
kind: milestone
id: ms_d0ff95f87a524760a4ee9275a27213a3
handle: r1-survey-rendering-substrates-reference-projects-ms_d0ff95f87a524760a4ee9275a27213a3
title: status_update
---

R1 DONE. Scored table (top): DOM-world+single-CSS-transform-camera 37/40 > SVG 30 > PixiJS/WebGL 27 > react-konva 26 > r3f+troika 24 > Canvas2D 22 > Motion Canvas 14. Criteria: crisp-text, camera-matrix fit, 100s-nodes-weak-HW, md/mermaid feasibility, hit-test, live-update, React, impl-cost. Refs: tldraw (DOM-world, content stays DOM, overlays->canvas 25x), Figma (full GPU/WASM, custom text layout - the cost we avoid), Excalidraw (two-canvas split + culling). Doc: docs/spikes/r1-canvas-substrate-survey.md @4351454
