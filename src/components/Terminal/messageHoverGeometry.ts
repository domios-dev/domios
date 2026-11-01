type Point = { x: number; y: number };
type Bounds = Pick<DOMRect, "left" | "right" | "top" | "bottom">;

const cross = (a: Point, b: Point, c: Point) => (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x);

function corners(rect: Bounds, margin: number): Point[] {
  return [
    { x: rect.left - margin, y: rect.top - margin },
    { x: rect.right + margin, y: rect.top - margin },
    { x: rect.right + margin, y: rect.bottom + margin },
    { x: rect.left - margin, y: rect.bottom + margin },
  ];
}

export function withinMessageHoverArea(point: Point, anchor: Bounds, menu: Bounds): boolean {
  const points = [...corners(anchor, 2), ...corners(menu, 6)].sort((a, b) => a.x - b.x || a.y - b.y);
  const half = (ordered: Point[]) => {
    const hull: Point[] = [];
    for (const next of ordered) {
      while (hull.length > 1 && cross(hull[hull.length - 2], hull[hull.length - 1], next) <= 0) hull.pop();
      hull.push(next);
    }
    return hull.slice(0, -1);
  };
  const hull = [...half(points), ...half([...points].reverse())];
  return hull.every((start, index) => cross(start, hull[(index + 1) % hull.length], point) >= 0);
}
