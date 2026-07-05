---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: section
id: sec_66fc0b7b0a2b4683a2cb8173216b7144
handle: w2-detail
title: Wave 2 — Shared detail surface
---

Wave 2 (foundation for Board/List interactivity): extract a view-agnostic detail overlay from the canvas-coupled CanvasDetailOverlay.

File ownership:
- New: src/components/Canvas/planView/PlanDetailOverlay.tsx — model-level props ({kind, task?, plan?, spec?, panes, onOpenPane, onClose}), fullscreen overlay, Esc close, focus trap. Reuses canvasRender/DetailContent.tsx as the body renderer.
- Modified: src/components/Canvas/CanvasDetailOverlay.tsx becomes a thin adapter (LaidOutNode -> model props) or is replaced by the shared overlay + adapter.
- canvasRender/DetailContent.tsx — loosen any node-coupled inputs to model-level; keep it the single source of truth for detail bodies.
- Tests: new PlanDetailOverlay.test.tsx (open/close/Esc/focus-trap/content for task/plan/spec), keep existing canvas overlay tests green.

Verification: npm run test:unit; manual dev check that Canvas expand (⤢) behavior is unchanged.
Design rule: no duplicated task-rendering logic between views; Board/List import the overlay from planView/, Canvas adapts. Self-review checkpoint: file sizes under 1000 lines, no canvas types leaking into planView/.
