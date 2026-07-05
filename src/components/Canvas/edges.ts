import type { LaidOutEdge, LaidOutNode } from "./layout";
import type { Box, Point } from "./layout";

function center(b: Box): Point {
  return { x: b.x + b.width / 2, y: b.y + b.height / 2 };
}

// Edge midpoint of a box on the side facing `toward` — keeps curves off the card.
function edgePoint(b: Box, toward: Point): Point {
  const c = center(b);
  const dx = toward.x - c.x;
  const dy = toward.y - c.y;
  if (Math.abs(dx) >= Math.abs(dy)) {
    return { x: dx >= 0 ? b.x + b.width : b.x, y: c.y };
  }
  return { x: c.x, y: dy >= 0 ? b.y + b.height : b.y };
}

export function resolveEdgeEnds(
  edge: LaidOutEdge,
  nodeById: Map<string, LaidOutNode>,
): { a: Point; b: Point } | null {
  const fromBox = edge.from ? nodeById.get(edge.from)?.box : undefined;
  const toBox = edge.to ? nodeById.get(edge.to)?.box : undefined;
  if (edge.from && !fromBox) return null;
  if (edge.to && !toBox) return null;

  const aSeed = edge.fromPoint ?? (fromBox ? center(fromBox) : null);
  const bSeed = edge.toPoint ?? (toBox ? center(toBox) : null);
  if (!aSeed || !bSeed) return null;

  const a = fromBox ? edgePoint(fromBox, bSeed) : aSeed;
  const b = toBox ? edgePoint(toBox, a) : bSeed;
  return { a, b };
}

// Smooth horizontal-biased cubic bezier between two anchor points (no arrowheads).
export function bezierPath(a: Point, b: Point, curve: number): string {
  const dx = b.x - a.x;
  const h = Math.max(Math.abs(dx) * curve, 24);
  const c1x = a.x + (dx >= 0 ? h : -h);
  const c2x = b.x + (dx >= 0 ? -h : h);
  return `M ${a.x} ${a.y} C ${c1x} ${a.y}, ${c2x} ${b.y}, ${b.x} ${b.y}`;
}
