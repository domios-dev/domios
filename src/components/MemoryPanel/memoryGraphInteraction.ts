interface Point {
  x: number;
  y: number;
}

export function hasDragged(start: Point, current: Point): boolean {
  return Math.hypot(current.x - start.x, current.y - start.y) >= 5;
}

export function fitFramedNodes(points: Point[], viewportStart: Point, viewportEnd: Point) {
  const finite = points.filter(point => Number.isFinite(point.x) && Number.isFinite(point.y));
  if (!finite.length) return null;
  const xs = finite.map(point => point.x).sort((a, b) => a - b);
  const ys = finite.map(point => point.y).sort((a, b) => a - b);
  const lower = Math.floor((finite.length - 1) / 2), upper = Math.floor(finite.length / 2);
  const x = (xs[lower] + xs[upper]) / 2, y = (ys[lower] + ys[upper]) / 2;
  const radii = finite.map(point => Math.hypot(point.x - x, point.y - y)).sort((a, b) => a - b);
  const radius = radii[Math.floor((finite.length - 1) * 0.95)] || radii[radii.length - 1];
  const diameter = 2 * radius * 1.15;
  const width = Math.abs(viewportEnd.x - viewportStart.x);
  const height = Math.abs(viewportEnd.y - viewportStart.y);
  if (!width || !height) return null;
  return {
    x,
    y,
    ratio: Math.max(0.05, Math.min(20, diameter / Math.min(width, height))),
    angle: 0,
  };
}
