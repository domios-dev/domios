---
plan_id: pl_2e932e82ad614cdd9f08072480b0857e
kind: task
id: task_efc06368a5d54aea8b0844b2fd62fbd9
handle: r3-research-world-figma-canvas-architecture
title: R3 — Research World/Figma canvas architecture
status: done
tags:
- substrate
- worlds
- architecture
- research
---

Research the architecture we should move to instead of continuing Worlds v3 DOM/CSS zoom fixes. Start from the observed failure: cards/nodes visually shrink, wobble, and reflow on approach; W2 slot stability fails because per-card crispScale/CSS zoom changes layout. Treat S0/S1 as prior evidence, not a final decision. Compare viable architectures for a Figma-like world canvas that supports rich plan/spec/task content: DOM world under one camera matrix, Canvas2D/WebGL/SVG/hybrid, DOM overlay for focused nodes, text crispness, markdown/mermaid, hit-testing, selection/copy, focus-by-id, LOD/worlds, performance on weak hardware, and incremental migration from current CanvasView/TaskCard. Deliver a concrete recommendation and migration sketch; do not implement product code in this task.
