import { memo } from "react";
import type { MemEdge, MemNode } from "./types";

interface Props {
  edge: MemEdge;
  fromNode: MemNode;
  toNode: MemNode;
  showLabel?: boolean;
}

function MemoryEdgeImpl({ edge, fromNode, toNode, showLabel = true }: Props) {
  if (
    fromNode.graph_x == null || fromNode.graph_y == null ||
    toNode.graph_x == null || toNode.graph_y == null
  ) {
    return null;
  }
  // Expired relations (valid_to set) render as dashed + half opacity
  const isExpired = edge.valid_to != null && edge.valid_to !== "";
  const baseAlpha = Math.max(0.15, Math.min(1, edge.confidence ?? 0.5));
  const alpha = isExpired ? baseAlpha * 0.4 : baseAlpha;
  const x1 = fromNode.graph_x;
  const y1 = fromNode.graph_y;
  const x2 = toNode.graph_x;
  const y2 = toNode.graph_y;

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
  const labelX = (x1 + 2 * cx + x2) / 4;
  const labelY = (y1 + 2 * cy + y2) / 4;

  const path = `M ${x1} ${y1} Q ${cx} ${cy} ${x2} ${y2}`;
  return (
    <g>
      <path
        d={path}
        fill="none"
        stroke={`rgba(180,180,220,${alpha})`}
        strokeWidth={1.2}
        strokeLinecap="round"
        strokeDasharray={isExpired ? "3 3" : undefined}
      />
      {showLabel && len > 100 && (
        <text
          x={labelX}
          y={labelY}
          textAnchor="middle"
          dominantBaseline="central"
          fill={`rgba(160,160,190,${Math.min(alpha + 0.2, 0.7)})`}
          fontSize={9}
          fontFamily="inherit"
          pointerEvents="none"
        >
          {edge.rel}
        </text>
      )}
    </g>
  );
}

export const MemoryEdge = memo(
  MemoryEdgeImpl,
  (prev, next) =>
    prev.edge === next.edge &&
    prev.fromNode === next.fromNode &&
    prev.toNode === next.toNode &&
    prev.showLabel === next.showLabel,
);
