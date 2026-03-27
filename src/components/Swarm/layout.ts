import type { SwarmNode } from "./types";

const NODE_W = 120;
const NODE_H = 56;
const V_GAP = 120;
const H_GAP = 40;

/**
 * Simple tree layout: orchestrator at top center, leads below, workers below their lead.
 * Mutates targetX/targetY on each node in-place.
 */
export function computeTreeLayout(
  nodes: Map<string, SwarmNode>,
  canvasW: number,
  _canvasH: number,
): void {
  if (nodes.size === 0) return;

  // Build adjacency: parentId -> children
  const children = new Map<string | null, SwarmNode[]>();
  const roots: SwarmNode[] = [];

  for (const node of nodes.values()) {
    if (node.parentId === null || !nodes.has(node.parentId)) {
      roots.push(node);
    } else {
      const list = children.get(node.parentId) || [];
      list.push(node);
      children.set(node.parentId, list);
    }
  }

  if (roots.length === 0) {
    // All nodes have parents but parents are missing — treat all as roots
    for (const node of nodes.values()) roots.push(node);
  }

  // Measure subtree widths for centering
  function subtreeWidth(nodeId: string): number {
    const kids = children.get(nodeId) || [];
    if (kids.length === 0) return NODE_W;
    const totalKids = kids.reduce((sum, k) => sum + subtreeWidth(k.id), 0);
    const gaps = (kids.length - 1) * H_GAP;
    return Math.max(NODE_W, totalKids + gaps);
  }

  // Position a node and its descendants
  function positionSubtree(node: SwarmNode, cx: number, y: number): void {
    node.targetX = cx - NODE_W / 2;
    node.targetY = y;

    const kids = children.get(node.id) || [];
    if (kids.length === 0) return;

    const totalW = kids.reduce((sum, k) => sum + subtreeWidth(k.id), 0)
      + (kids.length - 1) * H_GAP;
    let left = cx - totalW / 2;

    for (const kid of kids) {
      const w = subtreeWidth(kid.id);
      positionSubtree(kid, left + w / 2, y + NODE_H + V_GAP);
      left += w + H_GAP;
    }
  }

  // Position all root trees side by side, centered horizontally
  const totalRootsW = roots.reduce((sum, r) => sum + subtreeWidth(r.id), 0)
    + (roots.length - 1) * H_GAP;
  let startX = canvasW / 2 - totalRootsW / 2;
  const startY = 80;

  for (const root of roots) {
    const w = subtreeWidth(root.id);
    positionSubtree(root, startX + w / 2, startY);
    startX += w + H_GAP;
  }
}

export { NODE_W, NODE_H };
