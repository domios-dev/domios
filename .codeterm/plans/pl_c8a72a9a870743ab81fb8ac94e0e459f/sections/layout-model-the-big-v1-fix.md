**HOW the v1 layout was replaced.** v1 rendered a plan as one giant frame with three disconnected bands (scattered sections, floating tasks, milestones flying off edgeless). v2 is a compact composition computed by `layout/intraPlan.ts` → `layout/taskCloud.ts`: one COMPACT sectioned plan node (the hero) + a separate SPEC node + satellite TASK cards joined by curved edges to their exact section rows + STEP bubble clusters hung off each task. Nothing floats.

Tasks don't go in a column anymore — they pack into a dense cloud via golden-angle phyllotaxis, then gravity-compact so the real card hulls nestle:

```ts
// taskCloud.ts — the irrational that gives maximal angular spacing so tiles never
// align into spokes; gravity-compaction then pulls blobs toward the centroid until
// their actual card hulls (corners included) are about to touch.
export const GOLDEN_ANGLE = Math.PI * (3 - Math.sqrt(5));   // 137.5°
const COMPACT_ITERS = 28;
export const SECTION_GROUP_GAP = 14;
```

Subtasks render as a *smaller nested cloud* swarming their parent (parent = slot 0 at the local center), and the whole parent+satellites cluster packs as one blob among siblings — so a task always reads as a main tile haloed by its subtasks, solid at every zoom. Coverage shows as a fill tint on covered section rows (amber on uncovered). Layout is deterministic auto-pack — no manual dragging — so a realtime task addition slots in cleanly.

## Key decisions
- **Phyllotaxis cloud over the v1 task column** — kills the kilometer-tall column the spec called out; a dense disk reads as one mass at W0 and resolves into tiles on approach.
- **Edges to exact section rows** — `railAnchorOffsets` distributes rows inside each node's own height, so 1:1 spec↔plan edges go diagonal across differing Ys and nothing is edgeless.
- **Cluster-of-cluster packing for subtasks** — one packing algorithm handles both the disk and the satellites by treating a parent+children as a single footprint.
