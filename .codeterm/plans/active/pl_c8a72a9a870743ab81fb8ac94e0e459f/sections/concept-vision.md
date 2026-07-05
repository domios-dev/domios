---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: section
id: sec_d2d466e4c7eb474db55aa3ee102bc08f
handle: concept-vision
title: Concept Vision
---

**HOW the board projects the live orchestration graph.** The vision (a workspace-scoped Figma board over orchestrators → plans → Spec/Task/milestone cascades) is implemented as a **pure projection**: `PlansProvider` subscribes to the daemon's plan store, and `layoutWorkspace` turns that data into positioned nodes. No bespoke "canvas store" — the canvas is a view of the same JSON the CLI writes.

`layoutWorkspace` is the top of the projection and is deterministic — same data + settings → identical positions (the property the whole "no manual dragging" promise rests on):

```ts
export function layoutWorkspace(plans, settings = DEFAULT_CANVAS_SETTINGS, sectionTitles?): WorkspaceLayout {
  const active = plans.filter((p) => p.status === "active");
  const intra = active.map((p) => layoutPlanIntra(p, settings, sectionTitles));
  const packed = packRegions(intra, settings);          // owner clusters, shelf-packed frames
  return attachPlanLineage(packed, active, settings);   // follows/supersedes ghost edges
}
```

The two entry points are two camera targets over the *same* scene, not two screens: the workspace overview opens the full board (`canvasFocusStore.openWorkspace`), and the per-pane **Plan** button opens a focus on the plan/task that pane owns (`openFocus({ planId, focusPaneId })`). `paneOwnedPlans`/`planPaneMatch` resolve which plan a pane is working, so the camera lands on *that agent's* region.

## Key decisions
- **Projection over a second store** — the board can never drift from the orchestration truth, and any CLI mutation shows up on the canvas via the file-watcher → `plan-changed` loop.
- **Active-only at the top level** — archived plans don't clutter the board; they survive as lineage *ghosts* (`attachPlanLineage`/`ghostPlanNodeId`) so a "supersedes" edge still has a target to fly to.
- **Owner-clustered regions** — `packRegions` stacks plans by their orchestrator, so "who owns what" is spatial, matching the orchestrator→worker hierarchy.
