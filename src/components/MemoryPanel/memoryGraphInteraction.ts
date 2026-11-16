interface Point {
  x: number;
  y: number;
}

export function hasDragged(start: Point, current: Point): boolean {
  return Math.hypot(current.x - start.x, current.y - start.y) >= 5;
}

export function fitFramedNodes(points: Point[], viewportStart: Point, viewportEnd: Point) {
  let minX = Infinity, maxX = -Infinity, minY = Infinity, maxY = -Infinity;
  for (const point of points) {
    if (!Number.isFinite(point.x) || !Number.isFinite(point.y)) continue;
    minX = Math.min(minX, point.x);
    maxX = Math.max(maxX, point.x);
    minY = Math.min(minY, point.y);
    maxY = Math.max(maxY, point.y);
  }
  if (!Number.isFinite(minX)) return null;
  const width = Math.abs(viewportEnd.x - viewportStart.x);
  const height = Math.abs(viewportEnd.y - viewportStart.y);
  if (!width || !height) return null;
  return {
    x: (minX + maxX) / 2,
    y: (minY + maxY) / 2,
    ratio: Math.max(0.05, Math.min(20, 1.15 * Math.max((maxX - minX) / width, (maxY - minY) / height))),
    angle: 0,
  };
}
