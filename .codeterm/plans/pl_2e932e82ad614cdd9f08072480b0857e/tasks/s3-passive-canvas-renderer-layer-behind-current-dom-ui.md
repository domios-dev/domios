---
tags:
- canvas,figma-substrate,renderer
---

Add a passive canvas render layer driven by CanvasScene.

Scope:
- Render fixed world boxes, simple labels, status colors, edges, selection/focus rings onto <canvas data-testid="canvas-render-layer">.
- Use one camera transform/projection; no per-card CSS zoom or DOM reflow tricks.
- Renderer is feature-flagged and passive: current DOM cards remain mounted and interactive.
- Add debug pixel/readback hooks for Playwright.

Acceptance:
- Canvas is nonblank after fit, pan, zoom 0.25x, zoom 4x.
- Projected screen boxes match world box * camera scale within tolerance.
- Existing canvas UX remains unchanged when flag is off.
