export type Point = { x: number; y: number };
export type StoryCurve = { from: Point; to: Point; controlFrom: Point; controlTo: Point };

function edgePoint(rect: DOMRect, toward: Point, origin: DOMRect): Point {
  const center = { x: rect.left + rect.width / 2, y: rect.top + rect.height / 2 };
  const dx = toward.x - center.x;
  const dy = toward.y - center.y;
  const scale = Math.min(Math.abs(dx) > 0.001 ? rect.width / 2 / Math.abs(dx) : Infinity,
    Math.abs(dy) > 0.001 ? rect.height / 2 / Math.abs(dy) : Infinity);
  return { x: center.x + dx * scale - origin.left, y: center.y + dy * scale - origin.top };
}

export function storyCurve(fromRect: DOMRect, toRect: DOMRect, origin: DOMRect, kind: string): StoryCurve {
  let from = edgePoint(fromRect, { x: toRect.left + toRect.width / 2, y: toRect.top + toRect.height / 2 }, origin);
  let to = edgePoint(toRect, { x: fromRect.left + fromRect.width / 2, y: fromRect.top + fromRect.height / 2 }, origin);
  if (kind === 'team') {
    from = { x: fromRect.right - origin.left, y: fromRect.top + fromRect.height / 2 - origin.top };
    to = { x: toRect.left - origin.left, y: toRect.top + toRect.height / 2 - origin.top };
    const x = (from.x + to.x) / 2;
    return { from, to, controlFrom: { x, y: from.y }, controlTo: { x, y: to.y } };
  }
  if (kind === 'telegram' && toRect.top >= fromRect.bottom && toRect.left < fromRect.right) {
    from = { x: fromRect.left - origin.left, y: fromRect.top + fromRect.height / 2 - origin.top };
    to = { x: toRect.left - origin.left, y: toRect.top + toRect.height / 2 - origin.top };
    const x = Math.min(from.x, to.x) - 16;
    return { from, to, controlFrom: { x, y: from.y }, controlTo: { x, y: to.y } };
  }
  return { from, to,
    controlFrom: { x: from.x + (to.x - from.x) / 3, y: from.y + (to.y - from.y) / 3 },
    controlTo: { x: from.x + (to.x - from.x) * 2 / 3, y: from.y + (to.y - from.y) * 2 / 3 } };
}

export function curvePose(curve: StoryCurve, t: number): Point & { angle: number } {
  const { from, controlFrom: a, controlTo: b, to } = curve;
  const s = 1 - t;
  const x = s ** 3 * from.x + 3 * s ** 2 * t * a.x + 3 * s * t ** 2 * b.x + t ** 3 * to.x;
  const y = s ** 3 * from.y + 3 * s ** 2 * t * a.y + 3 * s * t ** 2 * b.y + t ** 3 * to.y;
  const dx = 3 * s ** 2 * (a.x - from.x) + 6 * s * t * (b.x - a.x) + 3 * t ** 2 * (to.x - b.x);
  const dy = 3 * s ** 2 * (a.y - from.y) + 6 * s * t * (b.y - a.y) + 3 * t ** 2 * (to.y - b.y);
  return { x, y, angle: Math.atan2(dy, dx) * 180 / Math.PI };
}
