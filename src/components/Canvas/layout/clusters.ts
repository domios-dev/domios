import type { Plan, Task } from "../types";
import type { CanvasLayoutSettings } from "../settings";
import type { Box } from "./geometry";
import type { LaidOutEdge, LaidOutNode } from "./types";

export interface LaneIndex {
  laneOf: (kind: string) => number;
  laneCount: () => number;
}

// Declared milestone_kinds define the lane order/color; unknown kinds append.
export function buildLaneIndex(plan: Plan): LaneIndex {
  const index = new Map<string, number>();
  plan.milestone_kinds.forEach((k, i) => index.set(k.key, i));
  let next = index.size;
  return {
    laneOf(kind: string): number {
      let i = index.get(kind);
      if (i == null) {
        i = next++;
        index.set(kind, i);
      }
      return i;
    },
    laneCount: () => Math.max(next, 1),
  };
}

export interface ClusterResult {
  nodes: LaidOutNode[];
  edges: LaidOutEdge[];
  width: number;
  height: number;
}

// Milestone bubble cluster (§2.4): a COMPACT 2-column wrapped grid that abuts the
// task card — never streams off to the right. Milestones are grouped by kind for
// adjacency (color lanes), then flow row-major; m.links draw bubble→bubble. Local
// coords from (0,0). The kind ordering keeps same-kind bubbles next to each other.
const CLUSTER_COLS = 2;

export function layoutCluster(
  task: Task,
  planId: string,
  lanes: LaneIndex,
  s: CanvasLayoutSettings,
): ClusterResult {
  const cell = s.bubbleRadius * 2;
  const stepX = cell + s.bubbleGapX;
  const stepY = cell + s.bubbleGapY;

  // Stable order: by declared kind lane, then by iteration, then input order.
  const ordered = task.milestones
    .map((m, i) => ({ m, i }))
    .sort((a, b) => {
      const la = lanes.laneOf(a.m.kind);
      const lb = lanes.laneOf(b.m.kind);
      if (la !== lb) return la - lb;
      const ia = a.m.iteration ?? 0;
      const ib = b.m.iteration ?? 0;
      return ia - ib || a.i - b.i;
    })
    .map((x) => x.m);

  const cols = Math.min(CLUSTER_COLS, Math.max(1, ordered.length));
  const nodes: LaidOutNode[] = ordered.map((m, idx) => {
    const col = idx % cols;
    const row = Math.floor(idx / cols);
    const box: Box = { x: col * stepX, y: row * stepY, width: cell, height: cell };
    return { id: m.id, kind: "milestone", planId, box, iteration: m.iteration, manual: false };
  });

  const present = new Set(task.milestones.map((m) => m.id));
  const edges: LaidOutEdge[] = [];
  for (const m of task.milestones) {
    for (const target of new Set(m.links)) {
      if (present.has(target)) {
        edges.push({ id: `${m.id}->${target}`, kind: "milestone-chain", from: m.id, to: target });
      }
    }
  }

  const rows = Math.ceil(ordered.length / cols);
  const width = ordered.length ? cols * cell + (cols - 1) * s.bubbleGapX : 0;
  const height = ordered.length ? rows * cell + (rows - 1) * s.bubbleGapY : 0;
  return { nodes, edges, width, height };
}
