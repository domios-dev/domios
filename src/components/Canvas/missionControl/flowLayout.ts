// Flow edges are derived from MEASURED station rects (not fixed pixel paths),
// so curves dock to real station edges at any width or zoom.

/** Design width of the plan-river stack; stations flex responsively within it. */
export const RIVER_WIDTH = 1180;

export interface Point {
  x: number;
  y: number;
}

export interface MeasuredEdge {
  from: string;
  to: string;
  d: string;
  /** Midpoint of the curve — pulse-dot + carrier anchor. */
  mid: Point;
  /** Horizontal gap between the two stations (carrier only rides a wide edge). */
  span: number;
  active: boolean;
}

export interface StationRect {
  key: string;
  left: number;
  top: number;
  width: number;
  height: number;
}

/** A soft horizontal S-curve between two points. */
export function curvePath(a: Point, b: Point): string {
  const dx = Math.max(24, (b.x - a.x) * 0.5);
  return `M${a.x} ${a.y} C ${a.x + dx} ${a.y}, ${b.x - dx} ${b.y}, ${b.x} ${b.y}`;
}

/** Right-edge / left-edge anchor points at a station's vertical center. */
function rightAnchor(r: StationRect): Point {
  return { x: r.left + r.width, y: r.top + r.height / 2 };
}
function leftAnchor(r: StationRect): Point {
  return { x: r.left, y: r.top + r.height / 2 };
}

/**
 * Connect consecutive stations (already in flow order) with measured curves.
 * The milestone→verify edge is flagged active (pulse + carrier travel it).
 */
export function edgesFromRects(rects: StationRect[]): MeasuredEdge[] {
  const edges: MeasuredEdge[] = [];
  for (let i = 0; i < rects.length - 1; i += 1) {
    const a = rects[i];
    const b = rects[i + 1];
    const p0 = rightAnchor(a);
    const p1 = leftAnchor(b);
    edges.push({
      from: a.key,
      to: b.key,
      d: curvePath(p0, p1),
      mid: { x: (p0.x + p1.x) / 2, y: (p0.y + p1.y) / 2 },
      span: p1.x - p0.x,
      active: a.key === "milestone" && b.key === "verify",
    });
  }
  return edges;
}
