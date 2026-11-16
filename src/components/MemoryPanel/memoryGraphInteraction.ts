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
  const trim = Math.floor(finite.length / 50);
  const minX = xs[trim], maxX = xs[finite.length - 1 - trim];
  const minY = ys[trim], maxY = ys[finite.length - 1 - trim];
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
