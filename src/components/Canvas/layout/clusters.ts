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

// Milestone bubble cluster (§2.4): kinds become columns, iterations stack as
// rows, m.links draw bubble→bubble inside the cluster. Local coords from (0,0).
export function layoutCluster(
  task: Task,
  planId: string,
  lanes: LaneIndex,
  s: CanvasLayoutSettings,
): ClusterResult {
  const cell = s.bubbleRadius * 2;
  const stepX = cell + s.bubbleGapX;
  const stepY = cell + s.bubbleGapY;
  const top = s.clusterLabelHeight;

  const rowByLane = new Map<number, number>();
  const nodes: LaidOutNode[] = [];
  let maxCol = 0;
  let maxRow = 0;

  for (const m of task.milestones) {
    const col = lanes.laneOf(m.kind);
    const seq = rowByLane.get(col) ?? 0;
    const row = m.iteration ?? seq;
    rowByLane.set(col, Math.max(seq, row) + 1);
    if (col > maxCol) maxCol = col;
    if (row > maxRow) maxRow = row;
    const box: Box = { x: col * stepX, y: top + row * stepY, width: cell, height: cell };
    nodes.push({ id: m.id, kind: "milestone", planId, box, iteration: m.iteration, manual: false });
  }

  const present = new Set(task.milestones.map((m) => m.id));
  const edges: LaidOutEdge[] = [];
  for (const m of task.milestones) {
    for (const target of new Set(m.links)) {
      if (present.has(target)) {
        edges.push({ id: `${m.id}->${target}`, kind: "milestone-chain", from: m.id, to: target });
      }
    }
  }

  const width = nodes.length ? (maxCol + 1) * stepX - s.bubbleGapX : 0;
  const height = nodes.length ? top + (maxRow + 1) * stepY - s.bubbleGapY : 0;
  return { nodes, edges, width, height };
}
