---
plan_id: pl_2e932e82ad614cdd9f08072480b0857e
kind: milestone
id: ms_1a26b633827d46b980922ccf10db415e
handle: r3-research-world-figma-canvas-architecture-ms_1a26b633827d46b980922ccf10db415e
title: status_update
---

R3 accepted. Architecture direction: CodeTerm-owned scene graph + one camera matrix + canvas/WebGL/WebGPU renderer for mass layers + capped DOM overlay for focused rich nodes.

Key finding:
- Current DOM world fails because per-card CSS zoom/crispScale participates in layout, so card internals reflow while world geometry claims to be stable.
- Correct invariant: world geometry and node footprint are fixed; camera matrix changes screen projection only; LOD may swap rendering detail but cannot change geometry.

Decision:
- Do not adopt React Flow/xyflow as endpoint: still DOM React nodes and same scaling/perf class.
- Do not adopt Excalidraw as endpoint: sketch element model does not fit rich plan/task cards.
- Use tldraw as design reference only for shape/geometry/interactions boundaries.
- Consider Pixi/WebGL only as renderer backend after CodeTerm scene contract is stable.

Recommended migration:
1. Extract CanvasScene from current layout output.
2. Add passive canvas renderer and debug draw/readback hooks.
3. Move hit-testing/focus-by-id to inverse-camera scene geometry.
4. Hybrid render: canvas for far/mid nodes, capped DOM overlays for focused rich nodes.
5. Add accelerated backend later, with Canvas2D fallback.

Required gates:
- fixed world geometry across zoom bands
- no per-card CSS zoom path under substrate flag
- nonblank render at fit/pan/zoom
- overlay budget on 500+ task fixture
- coordinate hit-test focus correctness
- rich overlay markdown/Mermaid/copy path
- weak-machine pan/zoom perf proxy
