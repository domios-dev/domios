import type { CanvasLayoutSettings } from "../settings";
import { translateBox, unionBox } from "./geometry";
import type { Box } from "./geometry";
import type { IntraPlanLayout, LaidOutNode, PlanFrame, WorkspaceLayout } from "./types";

function groupByOwner(layouts: IntraPlanLayout[]): Map<string, IntraPlanLayout[]> {
  const groups = new Map<string, IntraPlanLayout[]>();
  for (const l of layouts) {
    const g = groups.get(l.ownerKey);
    if (g) g.push(l);
    else groups.set(l.ownerKey, [l]);
  }
  return groups;
}

// Region packing (§6.1 phase 2): owner clusters stacked vertically, frames shelf-
// packed within each cluster, frameGap padding. Deterministic: same data → same layout.
export function packRegions(layouts: IntraPlanLayout[], s: CanvasLayoutSettings): WorkspaceLayout {
  const groups = groupByOwner(layouts);
  const ownerKeys = [...groups.keys()].sort();

  const frames: PlanFrame[] = [];
  let clusterY = 0;

  for (const key of ownerKeys) {
    const cluster = groups.get(key)!;
    cluster.sort((a, b) => (a.updatedAt - b.updatedAt) || a.planId.localeCompare(b.planId));

    let rowX = 0;
    let rowY = clusterY;
    let rowH = 0;

    for (const l of cluster) {
      if (rowX > 0 && rowX + l.width > s.rowMaxWidth) {
        rowY += rowH + s.frameGap;
        rowX = 0;
        rowH = 0;
      }
      const ox = rowX;
      const oy = rowY;
      const box: Box = { x: ox, y: oy, width: l.width, height: l.height };
      const nodes: LaidOutNode[] = l.nodes.map((n) => ({ ...n, box: translateBox(n.box, ox, oy) }));
      frames.push({
        planId: l.planId,
        title: l.title,
        owner: l.owner,
        ownerKey: l.ownerKey,
        box,
        nodes,
        edges: l.edges,
      });
      rowX += l.width + s.frameGap;
      rowH = Math.max(rowH, l.height);
    }

    clusterY = rowY + rowH + s.ownerClusterGapY;
  }

  const nodeIndex = new Map<string, LaidOutNode>();
  for (const f of frames) for (const n of f.nodes) nodeIndex.set(n.id, n);

  const bounds = unionBox(frames.map((f) => f.box));
  return { frames, nodeIndex, bounds };
}
