import { memo } from "react";
import type { MemEdge, MemNode } from "./types";

interface Props {
  edge: MemEdge;
  fromNode: MemNode;
  toNode: MemNode;
}

function MemoryEdgeImpl({ edge, fromNode, toNode }: Props) {
  if (
    fromNode.graph_x == null || fromNode.graph_y == null ||
    toNode.graph_x == null || toNode.graph_y == null
  ) {
    return null;
  }
  const alpha = Math.max(0.15, Math.min(1, edge.confidence ?? 0.5));
  const x1 = fromNode.graph_x;
  const y1 = fromNode.graph_y;
  const x2 = toNode.graph_x;
  const y2 = toNode.graph_y;

  // Quadratic bezier — control point offset perpendicular to the midpoint
  // Curvature hashed from ids so each edge has a stable, distinct arc
  const mx = (x1 + x2) / 2;
  const my = (y1 + y2) / 2;
  const dx = x2 - x1;
  const dy = y2 - y1;
  const len = Math.sqrt(dx * dx + dy * dy) || 1;
  const nx = -dy / len;
  const ny = dx / len;
  const hash = (edge.from.charCodeAt(0) + edge.to.charCodeAt(0) + edge.rel.length) % 7;
  const side = hash % 2 === 0 ? 1 : -1;
  const curvature = (0.15 + (hash / 7) * 0.2) * side;
  const cx = mx + nx * len * curvature;
  const cy = my + ny * len * curvature;

  const path = `M ${x1} ${y1} Q ${cx} ${cy} ${x2} ${y2}`;
  return (
    <path
      d={path}
      fill="none"
      stroke={`rgba(180,180,220,${alpha})`}
      strokeWidth={1.2}
      strokeLinecap="round"
    />
  );
}

export const MemoryEdge = memo(
  MemoryEdgeImpl,
  (prev, next) =>
    prev.edge === next.edge &&
    prev.fromNode === next.fromNode &&
    prev.toNode === next.toNode,
);
