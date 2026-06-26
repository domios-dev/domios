---
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
