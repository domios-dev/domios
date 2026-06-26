import type { EffortLinkRel, Plan, PlanStatus } from "../types";
import type { CanvasLayoutSettings } from "../settings";
import { unionBox, type Box } from "./geometry";
import type { LaidOutEdge, LaidOutNode, PlanFrame, WorkspaceLayout } from "./types";

export interface PlanLineageTarget {
  id: string;
  title: string;
  status: PlanStatus;
  sectionCount: number;
  taskCount: number;
}

function countSections(sections: Plan["sections"]): number {
  let n = 0;
  for (const s of sections) n += 1 + countSections(s.subsections);
  return n;
}

export interface PlanLineageEdge {
  from: string;
  to: string;
  rel?: EffortLinkRel;
  via: "lineage" | "links";
}

export function ghostPlanNodeId(planId: string): string {
  return `ghost:${planId}`;
}

export function collectPlanLineageEdges(activePlans: readonly Plan[]): PlanLineageEdge[] {
  const edges: PlanLineageEdge[] = [];
  const seen = new Set<string>();

  const add = (from: string, to: string, rel: EffortLinkRel | undefined, via: PlanLineageEdge["via"]) => {
    if (from === to) return;
    const key = `${from}\0${to}`;
    if (seen.has(key)) return;
    seen.add(key);
    edges.push({ from, to, rel, via });
  };

  for (const plan of activePlans) {
    for (const link of plan.lineage ?? []) {
      add(link.to, plan.id, link.rel, "lineage");
    }
    for (const target of plan.links ?? []) {
      add(plan.id, target, undefined, "links");
    }
  }

  return edges.sort((a, b) => a.from.localeCompare(b.from) || a.to.localeCompare(b.to));
}

function placeGhostBox(
  planNode: LaidOutNode,
  stackIndex: number,
  settings: CanvasLayoutSettings,
  side: "left" | "right",
): Box {
  const x =
    side === "left"
      ? planNode.box.x - settings.ghostPlanGap - settings.ghostPlanW
      : planNode.box.x + planNode.box.width + settings.ghostPlanGap;
  return {
    x,
    y: planNode.box.y + stackIndex * (settings.ghostPlanH + settings.ghostPlanStackGap),
    width: settings.ghostPlanW,
    height: settings.ghostPlanH,
  };
}

function workspaceBounds(frames: PlanFrame[]): Box {
  const boxes = frames.flatMap((f) => [f.box, ...f.nodes.map((n) => n.box)]);
  if (!boxes.length) return { x: 0, y: 0, width: 0, height: 0 };
  return unionBox(boxes);
}

function ensureGhostNode(
  frame: PlanFrame,
  anchor: LaidOutNode,
  planId: string,
  side: "left" | "right",
  settings: CanvasLayoutSettings,
  nodeIndex: Map<string, LaidOutNode>,
  ghostStacks: Map<string, number>,
): string {
  const id = ghostPlanNodeId(planId);
  if (nodeIndex.has(id)) return id;
  const stackKey = `${frame.planId}:${side}`;
  const stack = ghostStacks.get(stackKey) ?? 0;
  ghostStacks.set(stackKey, stack + 1);
  const ghost: LaidOutNode = {
    id,
    kind: "plan-ghost",
    planId: frame.planId,
    box: placeGhostBox(anchor, stack, settings, side),
    iteration: null,
    manual: false,
  };
  frame.nodes.push(ghost);
  nodeIndex.set(id, ghost);
  return id;
}

export function attachPlanLineage(
  layout: WorkspaceLayout,
  activePlans: readonly Plan[],
  settings: CanvasLayoutSettings,
): WorkspaceLayout {
  const activeIds = new Set(activePlans.map((p) => p.id));
  const lineageEdges = collectPlanLineageEdges(activePlans);
  if (!lineageEdges.length) return { ...layout, crossEdges: [] };

  const frameByPlanId = new Map(layout.frames.map((f) => [f.planId, f]));
  const nodeIndex = new Map(layout.nodeIndex);
  const crossEdges: LaidOutEdge[] = [];
  const ghostStacks = new Map<string, number>();

  for (const edge of lineageEdges) {
    const fromActive = activeIds.has(edge.from);
    const toActive = activeIds.has(edge.to);
    const fromFrame = fromActive ? frameByPlanId.get(edge.from) : undefined;
    const toFrame = toActive ? frameByPlanId.get(edge.to) : undefined;
    let fromNodeId = edge.from;
    let toNodeId = edge.to;
    let ownerFrame = toFrame ?? fromFrame;

    if (!ownerFrame) continue;

    if (!fromActive) {
      const anchor = ownerFrame.nodes.find((n) => n.id === edge.to && n.kind === "plan");
      if (!anchor) continue;
      fromNodeId = ensureGhostNode(ownerFrame, anchor, edge.from, "left", settings, nodeIndex, ghostStacks);
    }

    if (!toActive) {
      const anchor = ownerFrame.nodes.find((n) => n.id === edge.from && n.kind === "plan");
      if (!anchor) continue;
      toNodeId = ensureGhostNode(ownerFrame, anchor, edge.to, "right", settings, nodeIndex, ghostStacks);
    }

    const laid: LaidOutEdge = {
      id: `plan-lineage:${edge.from}→${edge.to}`,
      kind: "plan-plan",
      from: fromNodeId,
      to: toNodeId,
    };

    if (fromActive && toActive && fromFrame && toFrame && fromFrame.planId !== toFrame.planId) {
      crossEdges.push(laid);
    } else {
      ownerFrame.edges.push(laid);
    }
  }

  return {
    ...layout,
    nodeIndex,
    crossEdges,
    bounds: workspaceBounds(layout.frames),
  };
}

export function buildLineageCatalog(plans: readonly Plan[]): Map<string, PlanLineageTarget> {
  const out = new Map<string, PlanLineageTarget>();
  for (const p of plans) {
    out.set(p.id, { id: p.id, title: p.title, status: p.status, sectionCount: countSections(p.sections), taskCount: p.tasks.length });
  }
  return out;
}
