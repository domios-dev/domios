Canvas is the tertiary, spatial/presence view. Evidence (086555a7, 522f9cc6) shows its failure mode: zoomed out it becomes a laggy cloud of tiny unreadable dots with decorative labels, which is confusing and worse than useless for orientation.

Requirements:
- Canvas keeps its role: spatial layout, live worker presence, fly-to focus, stride/milestone theater. It is NOT required for task inspection basics — Board/List cover that.
- Readability floor: at any zoom level, rendered nodes must either be readable cards or explicit aggregate markers (cluster chips with counts), never a dot-cloud. Below the readability threshold the renderer switches to cluster/summary LOD instead of drawing hundreds of tiny cards.
- Performance budget: pan/zoom on a plan with 200+ tasks sustains interactive frame rate (no visible stutter; frame probe p95 under ~16ms on dev hardware) — instrument via existing useCanvasFrameProbe.
- Minimum-zoom clamp: the camera must not reach zoom levels where the whole world is sub-legible; zoom-out beyond the threshold triggers the overview LOD, not smaller pixels.
- Done-heavy plans must not dominate the world: done tasks compress into per-section done-stacks/clusters rather than individual full cards.
- No new features on Canvas in this effort beyond LOD/perf/legibility; feature growth is a non-goal.
