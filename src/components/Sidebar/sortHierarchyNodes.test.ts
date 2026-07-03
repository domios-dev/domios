// Run: npx tsx src/components/Sidebar/sortHierarchyNodes.test.ts

import type { Pane, PaneStatus } from "../../lib/types";
import {
  HIERARCHY_SORT_STABILITY_MS,
  sortHierarchyNodes,
  statusSortRank,
  type HierarchyNodeBase,
} from "./sortHierarchyNodes";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

function pane(id: string): Pane {
  return {
    id,
    title: id,
    pane_type: "terminal",
    title_level: "Default",
    cwd: null,
  } as unknown as Pane;
}

function node(id: string, status: PaneStatus | null = null): HierarchyNodeBase {
  return { pane: pane(id), status };
}

assert(statusSortRank("Working") < statusSortRank("Waiting"), "Working ranks above Waiting");
assert(statusSortRank("Waiting") < statusSortRank("Idle"), "Waiting ranks above Idle");
assert(statusSortRank("Idle") < statusSortRank("Dead"), "Idle ranks above Dead");

const t0 = 1_700_000_000_000;
const nodes = [node("idle-1", "Idle"), node("work-1", "Working"), node("wait-1", "Waiting")];
const statuses: Record<string, PaneStatus> = {
  "idle-1": "Idle",
  "work-1": "Working",
  "wait-1": "Waiting",
};

const first = sortHierarchyNodes(nodes, statuses, null, t0);
assert(
  first.nodes.map((n) => n.pane.id).join(",") === "work-1,wait-1,idle-1",
  `status groups sort Working→Waiting→Idle, got ${first.nodes.map((n) => n.pane.id).join(",")}`,
);

const tieNodes = [node("b", "Idle"), node("a", "Idle")];
const tieStatuses: Record<string, PaneStatus> = { a: "Idle", b: "Idle" };
const tie = sortHierarchyNodes(tieNodes, tieStatuses, null, t0);
assert(
  tie.nodes.map((n) => n.pane.id).join(",") === "b,a",
  `tiebreak preserves incoming order, got ${tie.nodes.map((n) => n.pane.id).join(",")}`,
);

const flapNodes = [node("w", "Idle"), node("i", "Idle")];
let state = sortHierarchyNodes(flapNodes, { w: "Idle", i: "Idle" }, null, t0).state;
const midFlap = sortHierarchyNodes(
  flapNodes,
  { w: "Working", i: "Idle" },
  state,
  t0 + 500,
);
assert(
  midFlap.nodes.map((n) => n.pane.id).join(",") === "w,i",
  "status flap inside stability window keeps previous order",
);
state = midFlap.state;
const afterStable = sortHierarchyNodes(
  flapNodes,
  { w: "Working", i: "Idle" },
  state,
  t0 + 500 + HIERARCHY_SORT_STABILITY_MS,
);
assert(
  afterStable.nodes[0]?.pane.id === "w",
  "working row rises after stability window",
);

const swapNodes = [node("x"), node("y")];
const flipAt = t0 + 1_000;
let swapState = sortHierarchyNodes(
  swapNodes,
  { x: "Working", y: "Idle" },
  null,
  t0,
).state;
assert(
  sortHierarchyNodes(swapNodes, { x: "Working", y: "Idle" }, swapState, t0).nodes
    .map((n) => n.pane.id)
    .join(",") === "x,y",
  "working x sorts above idle y",
);
const insideWindow = sortHierarchyNodes(
  swapNodes,
  { x: "Idle", y: "Working" },
  swapState,
  flipAt + 500,
);
assert(
  insideWindow.nodes.map((n) => n.pane.id).join(",") === "x,y",
  "order unchanged inside stability window after flip",
);
const swapped = sortHierarchyNodes(
  swapNodes,
  { x: "Idle", y: "Working" },
  insideWindow.state,
  flipAt + 500 + HIERARCHY_SORT_STABILITY_MS,
);
assert(
  swapped.nodes.map((n) => n.pane.id).join(",") === "y,x",
  `committed swap after stability, got ${swapped.nodes.map((n) => n.pane.id).join(",")}`,
);

console.log("✓ sortHierarchyNodes: ranks, tiebreaks, stability window");
