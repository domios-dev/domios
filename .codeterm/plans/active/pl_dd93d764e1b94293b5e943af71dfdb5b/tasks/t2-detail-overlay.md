---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_65a11939d78c44a0818030c0b23e7c1e
handle: t2-detail-overlay
title: Shared PlanDetailOverlay (view-agnostic detail surface)
status: done
---

Extract a view-agnostic detail overlay so Board and List can open task/plan/spec details with the same surface Canvas uses.

Steps:
1. Create src/components/Canvas/planView/PlanDetailOverlay.tsx with model-level props: {kind: "task"|"plan"|"spec", task?, plan?, spec?, specSections?, panes?, onOpenPane?, onClose}. Move the fullscreen/Esc/focus-trap shell from CanvasDetailOverlay.tsx; body renders via canvasRender/DetailContent.tsx.
2. Refactor CanvasDetailOverlay.tsx into a thin adapter: LaidOutNode + resolveModel -> PlanDetailOverlay props. Canvas behavior (camera untouched, ⤢ expand, section nav callbacks) must not change.
3. Loosen DetailContent inputs where they assume canvas node context; keep it the single body renderer (no forked copies).
4. Unit tests: new PlanDetailOverlay.test.tsx (renders task/plan/spec kinds, Esc closes, focus trap, open-pane action); existing canvas overlay/DetailContent tests stay green.

Commands: npm run test:unit; manual dev check of Canvas expand behavior (./build.sh dev on 7686, open any plan canvas, expand a card).
Report: new API surface (props signature), what CanvasDetailOverlay kept vs delegated, test output.
