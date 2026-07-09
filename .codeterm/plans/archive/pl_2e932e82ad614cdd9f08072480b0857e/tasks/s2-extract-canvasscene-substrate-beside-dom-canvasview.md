---
plan_id: pl_2e932e82ad614cdd9f08072480b0857e
kind: task
id: task_638dbd55d0894626ab459d5d6112f09d
handle: s2-extract-canvasscene-substrate-beside-dom-canvasview
title: S2 — Extract CanvasScene substrate beside DOM CanvasView
status: done
tags:
- canvas,figma-substrate,scene
---

Extract a pure CanvasScene substrate beside the current DOM CanvasView.

Scope:
- Create scene records for plan/spec/task/edge/group nodes from existing layout output: id, kind, world box, anchors, semantic text, status, LOD inputs.
- Keep existing DOM cards as the active UI; this slice must be observational/passive.
- Add stable debug/readback API for tests to inspect scene world boxes and projected screen boxes.
- No per-node CSS zoom/counter-scale changes in this slice.

Acceptance:
- Scene builder is deterministic and unit-tested with existing layout fixtures.
- World boxes remain stable across camera scale changes.
- Source files stay under repo line limits.
