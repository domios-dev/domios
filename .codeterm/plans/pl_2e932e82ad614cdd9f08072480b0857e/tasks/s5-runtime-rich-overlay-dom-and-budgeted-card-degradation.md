---
tags:
- canvas
- substrate
- performance
---

Implement the first runtime rich overlay slice that turns the S4 contract into visible behavior and starts reducing DOM-card cost.

Context:
- S3 added passive canvas rendering behind current DOM cards.
- S4 added scene hit-testing and `OverlayNode` readback contract with max active overlays = 2.
- The user still observes canvas jank because the DOM cards are still mounted; the next win must cap rich DOM work, not add more passive drawing.

Scope:
1. Mount a visible rich overlay from `OverlayNode` for focused plan/task/spec nodes under the `canvas.substrateV2` feature flag.
2. The overlay must use the exact `screenRect` from scene world box x camera, and must not resize the world box.
3. Keep rich markdown/Mermaid/copy behavior for focused overlays where existing node/card components allow it without a huge rewrite.
4. Add a conservative DOM budget path under flag: non-focused/far rich card content should be hidden/degraded when overlay budget is enforced, while preserving basic visible geometry and labels.
5. Add/extend test hooks and Playwright assertions so the orchestrator can verify max rich overlays and no blank canvas.

Acceptance:
- With `canvas.substrateV2=true` and `canvas.overlayBudget=true`, rich DOM overlays are <= 2 on a large fixture or seeded board.
- Focused overlay is positioned from `OverlayNode.screenRect` and exposes the same node id as scene picking.
- Existing non-flag canvas behavior remains unchanged.
- Touched files stay under 1000 lines; split helper/components if CanvasView approaches the limit.
