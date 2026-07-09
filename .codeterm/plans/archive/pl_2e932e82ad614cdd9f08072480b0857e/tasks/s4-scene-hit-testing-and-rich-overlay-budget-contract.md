---
plan_id: pl_2e932e82ad614cdd9f08072480b0857e
kind: task
id: task_7f3081ed0b9a4f609e59599ab17a211e
handle: s4-scene-hit-testing-and-rich-overlay-budget-contract
title: S4 — Scene hit-testing and rich overlay budget contract
status: done
tags:
- canvas,figma-substrate,overlay
---

Move focus/hit-test contract toward scene geometry and define the rich DOM overlay budget.

Scope:
- Add inverse-camera picking against CanvasScene geometry for task/plan/spec nodes.
- Wire focus-by-id and selection to scene ids behind a feature flag.
- Define OverlayNode contract: exact screen rect from world box x camera, max active overlay count, focused rich markdown/Mermaid/copy behavior.
- Add tests that clicking a drawn node focuses the correct semantic id.

Acceptance:
- Hit-test does not depend on DOM card layout.
- Overlay count remains bounded on large fixtures.
- Focused overlay can render rich content without resizing the world box.
