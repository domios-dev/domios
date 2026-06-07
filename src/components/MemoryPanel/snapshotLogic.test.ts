import {
  applyDelta,
  edgeCandidates,
  indexSnapshot,
  inViewport,
  isZoomedOut,
  labelCandidates,
  SMALL_GRAPH_THRESHOLD,
  overviewNodes,
  LOD_CLUSTER_RATIO,
  LOD_OVERVIEW_IMPORTANCE,
  LABEL_MIN_IMPORTANCE,
  type Bounds,
} from "./snapshotLogic";
import type { DeltaChanged, GraphSnapshot, SnapshotNode } from "./snapshotTypes";

let passed = 0;
let failed = 0;

function assert(label: string, ok: boolean, detail?: string) {
  if (ok) {
    console.log(`  ✓ ${label}`);
    passed++;
  } else {
    console.error(`  ✗ ${label}${detail ? `: ${detail}` : ""}`);
    failed++;
  }
}

function node(id: string, over: Partial<SnapshotNode> = {}): SnapshotNode {
  return {
    id,
    x: 0,
    y: 0,
    kind: "fact",
    importance: 1,
    cluster_id: 0,
    has_vector: true,
    pinned: false,
    ...over,
  };
}

function snap(nodes: SnapshotNode[]): GraphSnapshot {
  return {
    layout_version: 1,
    computed_at: "now",
    seq: 10,
    nodes,
    clusters: [],
    cluster_edges: [],
    stats: { node_count: nodes.length, cluster_count: 0, edge_count: 0, vectorized_without_position: 0 },
  };
}

const FULL: Bounds = { minX: -1000, maxX: 1000, minY: -1000, maxY: 1000 };

// --- LOD thresholds ---
console.log("isZoomedOut threshold");
const BIG = SMALL_GRAPH_THRESHOLD + 1;
assert("big graph above ratio = zoomed out", isZoomedOut(LOD_CLUSTER_RATIO + 0.01, BIG));
assert("big graph at ratio = not zoomed out", !isZoomedOut(LOD_CLUSTER_RATIO, BIG));
assert("big graph below ratio = not zoomed out", !isZoomedOut(0.2, BIG));
assert("small graph never zoomed out", !isZoomedOut(10, SMALL_GRAPH_THRESHOLD));

console.log("\ninViewport null bounds = no filtering");
assert("null bounds passes all", inViewport(node("n", { x: 9999, y: -9999 }), null));

console.log("\noverviewNodes keeps only top-importance anchors");
{
  const nodes = [
    node("hi", { importance: LOD_OVERVIEW_IMPORTANCE }),
    node("higher", { importance: LOD_OVERVIEW_IMPORTANCE + 1 }),
    node("lo", { importance: LOD_OVERVIEW_IMPORTANCE - 1 }),
  ];
  const kept = overviewNodes(nodes).map((n) => n.id);
  assert("keeps >= threshold", kept.includes("hi") && kept.includes("higher"));
  assert("drops below threshold", !kept.includes("lo"));
}

console.log("\ninViewport bounds check");
{
  const b: Bounds = { minX: 0, maxX: 10, minY: 0, maxY: 10 };
  assert("inside", inViewport(node("a", { x: 5, y: 5 }), b));
  assert("outside x", !inViewport(node("b", { x: 20, y: 5 }), b));
  assert("outside y", !inViewport(node("c", { x: 5, y: -1 }), b));
}

console.log("\nlabelCandidates: importance filter + viewport + dedupe + sort");
{
  const nodes = [
    node("low", { importance: LABEL_MIN_IMPORTANCE - 0.1, x: 1, y: 1 }),
    node("mid", { importance: 3, x: 1, y: 1 }),
    node("top", { importance: 5, x: 1, y: 1 }),
    node("far", { importance: 5, x: 9999, y: 9999 }),
    node("done", { importance: 5, x: 1, y: 1 }),
  ];
  const ids = labelCandidates(nodes, FULL, new Set(["done"]));
  assert("below min importance excluded", !ids.includes("low"));
  assert("out-of-viewport excluded", !ids.includes("far"));
  assert("already-fetched excluded", !ids.includes("done"));
  assert("highest importance first", ids[0] === "top" && ids[1] === "mid");
}

console.log("\nlabelCandidates respects cap");
{
  const many = Array.from({ length: 800 }, (_, i) => node(`n${i}`, { importance: 5, x: 1, y: 1 }));
  const ids = labelCandidates(many, FULL, new Set(), 500);
  assert("capped at 500", ids.length === 500);
}

console.log("\nedgeCandidates: viewport + dedupe + cap");
{
  const nodes = [
    node("a", { x: 1, y: 1 }),
    node("b", { x: 9999, y: 9999 }),
    node("c", { x: 1, y: 1 }),
  ];
  const ids = edgeCandidates(nodes, FULL, new Set(["c"]));
  assert("includes in-viewport new", ids.includes("a"));
  assert("excludes out-of-viewport", !ids.includes("b"));
  assert("excludes cached", !ids.includes("c"));
  const many = Array.from({ length: 800 }, (_, i) => node(`e${i}`, { x: 1, y: 1 }));
  assert("capped at 500", edgeCandidates(many, FULL, new Set(), 500).length === 500);
}

// --- delta merge ---
console.log("\napplyDelta: moves, adds, removes");
{
  const idx = indexSnapshot(snap([node("a", { x: 0, y: 0 }), node("b")]));
  const delta: DeltaChanged = {
    layout_version: 1,
    seq: 20,
    added_or_moved: [
      { id: "a", x: 5, y: 6, cluster_id: 2 },
      { id: "new", x: 7, y: 8, cluster_id: 3 },
    ],
    removed: ["b"],
  };
  const next = applyDelta(idx, delta);
  const a = next.byId.get("a");
  assert("moved node updated position", a?.x === 5 && a?.y === 6 && a?.cluster_id === 2);
  assert("moved node preserves kind/importance", a?.kind === "fact" && a?.importance === 1);
  assert("removed node gone", !next.byId.has("b"));
  assert("new node added", next.byId.get("new")?.x === 7);
  assert("seq advanced", next.seq === 20);
  assert("original index untouched (immutable)", idx.byId.get("a")?.x === 0 && idx.byId.has("b"));
}

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) throw new Error(`${failed} test(s) failed`);
