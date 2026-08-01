import type { CanvasLayoutSettings } from "../settings";
import { ownerName } from "../owner";
import { translateBox, unionBox } from "./geometry";
import type { Box, Point } from "./geometry";
import type { IntraPlanLayout, LaidOutEdge, LaidOutNode, OwnerGroup, PlanFrame, SectionZone, WorkspaceLayout } from "./types";

function groupByOwner(layouts: IntraPlanLayout[]): Map<string, IntraPlanLayout[]> {
  const groups = new Map<string, IntraPlanLayout[]>();
  for (const l of layouts) {
    const g = groups.get(l.ownerKey);
    if (g) g.push(l);
    else groups.set(l.ownerKey, [l]);
  }
  return groups;
}

function shiftPoint(p: Point, dx: number, dy: number): Point {
  return { x: p.x + dx, y: p.y + dy };
}

function shiftEdge(e: LaidOutEdge, dx: number, dy: number): LaidOutEdge {
  return {
    ...e,
    fromPoint: e.fromPoint ? shiftPoint(e.fromPoint, dx, dy) : undefined,
    toPoint: e.toPoint ? shiftPoint(e.toPoint, dx, dy) : undefined,
  };
}

function shiftZone(z: SectionZone, dx: number, dy: number): SectionZone {
  return { ...z, hull: z.hull.map((p) => shiftPoint(p, dx, dy)), labelAnchor: shiftPoint(z.labelAnchor, dx, dy) };
}

function ownerTitle(owner: IntraPlanLayout["owner"]): string {
  return ownerName(owner) ?? "Unowned · claimable";
}

// each cluster, group wrapper boxes computed per owner. Deterministic.
export function packRegions(layouts: IntraPlanLayout[], s: CanvasLayoutSettings): WorkspaceLayout {
  const groups = groupByOwner(layouts);
  const ownerKeys = [...groups.keys()].sort();

  const frames: PlanFrame[] = [];
  const ownerGroups: OwnerGroup[] = [];
  let clusterY = 0;

  for (const key of ownerKeys) {
    const cluster = groups.get(key)!;
    cluster.sort((a, b) => a.updatedAt - b.updatedAt || a.planId.localeCompare(b.planId));

    const innerTop = clusterY + s.groupLabelHeight + s.groupPadding;
    let rowX = s.groupPadding;
    let rowY = innerTop;
    let rowH = 0;
    const clusterFrames: PlanFrame[] = [];

    for (const l of cluster) {
      if (rowX > s.groupPadding && rowX + l.width > s.rowMaxWidth) {
        rowY += rowH + s.frameGap;
        rowX = s.groupPadding;
        rowH = 0;
      }
      const ox = rowX;
      const oy = rowY;
      const box: Box = { x: ox, y: oy, width: l.width, height: l.height };
      const nodes: LaidOutNode[] = l.nodes.map((n) => ({ ...n, box: translateBox(n.box, ox, oy) }));
      const anchors = new Map<string, Point>();
      for (const [id, p] of l.anchors) anchors.set(id, shiftPoint(p, ox, oy));
      const frame: PlanFrame = {
        planId: l.planId,
        title: l.title,
        owner: l.owner,
        ownerKey: l.ownerKey,
        box,
        nodes,
        edges: l.edges.map((e) => shiftEdge(e, ox, oy)),
        anchors,
        zones: l.zones.map((z) => shiftZone(z, ox, oy)),
      };
      frames.push(frame);
      clusterFrames.push(frame);
      rowX += l.width + s.frameGap;
      rowH = Math.max(rowH, l.height);
    }

    const inner = unionBox(clusterFrames.map((f) => f.box));
    const groupBox: Box = {
      x: inner.x - s.groupPadding,
      y: clusterY,
      width: inner.width + s.groupPadding * 2,
      height: inner.height + s.groupLabelHeight + s.groupPadding * 2,
    };
    ownerGroups.push({
      ownerKey: key,
      title: ownerTitle(cluster[0].owner),
      ownerTabId: cluster[0].owner.tab_id,
      box: groupBox,
      planIds: cluster.map((c) => c.planId),
    });

    clusterY = groupBox.y + groupBox.height + s.groupGapY;
  }

  const nodeIndex = new Map<string, LaidOutNode>();
  for (const f of frames) for (const n of f.nodes) nodeIndex.set(n.id, n);

  // keeps the composition filling the viewport instead of framing dead padding.
  const bounds = unionBox(frames.map((f) => f.box));
  return { frames, nodeIndex, groups: ownerGroups, bounds };
}
