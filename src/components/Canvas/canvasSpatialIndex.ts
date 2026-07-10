import type { Box, Point } from "./layout";


export interface IndexedNode {
  id: string;
  kind: string;
  planId: string;
  box: Box;
}

export interface SpatialIndex {
  queryPoint: (p: Point) => IndexedNode | null;
  size: number;
}

function contains(b: Box, p: Point): boolean {
  return p.x >= b.x && p.x <= b.x + b.width && p.y >= b.y && p.y <= b.y + b.height;
}

function area(b: Box): number {
  return b.width * b.height;
}

// A box overlapping more cells than this goes to the overflow list (scanned every query)
// instead of bloating the grid — bounds insertions for huge plan/spec boxes.
const MAX_CELLS_PER_BOX = 48;

export function buildSpatialIndex(nodes: readonly IndexedNode[], cellSize?: number): SpatialIndex {
  // Cell ≈ median node width (clamped) so a typical small box touches ~1–4 cells.
  let cell = cellSize ?? 0;
  if (!cell) {
    const widths = nodes.map((n) => n.box.width).filter((w) => w > 0).sort((a, b) => a - b);
    const median = widths.length ? widths[widths.length >> 1] : 160;
    cell = Math.max(80, Math.min(400, median));
  }
  const grid = new Map<string, IndexedNode[]>();
  const oversized: IndexedNode[] = [];
  const cellOf = (v: number) => Math.floor(v / cell);

  for (const n of nodes) {
    const x0 = cellOf(n.box.x);
    const y0 = cellOf(n.box.y);
    const x1 = cellOf(n.box.x + n.box.width);
    const y1 = cellOf(n.box.y + n.box.height);
    if ((x1 - x0 + 1) * (y1 - y0 + 1) > MAX_CELLS_PER_BOX) {
      oversized.push(n);
      continue;
    }
    for (let cx = x0; cx <= x1; cx++) {
      for (let cy = y0; cy <= y1; cy++) {
        const key = `${cx}:${cy}`;
        const bucket = grid.get(key);
        if (bucket) bucket.push(n);
        else grid.set(key, [n]);
      }
    }
  }

  return {
    size: nodes.length,
    queryPoint(p: Point): IndexedNode | null {
      let best: IndexedNode | null = null;
      const consider = (n: IndexedNode) => {
        if (contains(n.box, p) && (!best || area(n.box) < area(best.box))) best = n;
      };
      const bucket = grid.get(`${cellOf(p.x)}:${cellOf(p.y)}`);
      if (bucket) for (const n of bucket) consider(n);
      for (const n of oversized) consider(n);
      return best;
    },
  };
}
