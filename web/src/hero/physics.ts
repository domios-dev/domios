export type Point = { x: number; y: number };
export type Spring = { value: number; velocity: number };
export type Curve = { start: Point; end: Point; c1: Point; c2: Point };
export type Channel = { from: number; to: number; curve: Curve; length: number };
type Side = 'left' | 'right' | 'top' | 'bottom';

export const spring = (value = 0, velocity = 0): Spring => ({ value, velocity });
export const clamp = (value: number, min = 0, max = 1) => Math.min(max, Math.max(min, value));

export function stepSpring(state: Spring, target: number, stiffness: number, damping: number, elapsed: number) {
  const steps = Math.ceil(elapsed / (1 / 120));
  const dt = elapsed / steps;
  for (let step = 0; step < steps; step++) {
    state.velocity += ((target - state.value) * stiffness - state.velocity * damping) * dt;
    state.value += state.velocity * dt;
  }
}

export function pointAt(curve: Curve, t: number): Point {
  const u = 1 - t;
  return {
    x: u ** 3 * curve.start.x + 3 * u * u * t * curve.c1.x + 3 * u * t * t * curve.c2.x + t ** 3 * curve.end.x,
    y: u ** 3 * curve.start.y + 3 * u * u * t * curve.c1.y + 3 * u * t * t * curve.c2.y + t ** 3 * curve.end.y,
  };
}

export function tangentAt(curve: Curve, t: number): number {
  const before = pointAt(curve, t - .001);
  const after = pointAt(curve, t + .001);
  return Math.atan2(after.y - before.y, after.x - before.x);
}

export function curvePath(curve: Curve): string {
  return `M${curve.start.x},${curve.start.y}C${curve.c1.x},${curve.c1.y} ${curve.c2.x},${curve.c2.y} ${curve.end.x},${curve.end.y}`;
}

function port(card: HTMLElement, side: Side): Point {
  return {
    x: card.offsetLeft + (side === 'left' ? 0 : side === 'right' ? card.offsetWidth : card.offsetWidth / 2),
    y: card.offsetTop + (side === 'top' ? 0 : side === 'bottom' ? card.offsetHeight : card.offsetHeight / 2),
  };
}

function control(point: Point, side: Side, reach: number): Point {
  return { x: point.x + (side === 'left' ? -reach : side === 'right' ? reach : 0), y: point.y + (side === 'top' ? -reach : side === 'bottom' ? reach : 0) };
}

export function measureChannels(cards: HTMLElement[], mobile: boolean, compact: boolean): Channel[] {
  const links: [number, number, Side, Side][] = mobile
    ? [[0, 1, 'bottom', 'top'], [1, 2, 'bottom', 'top']]
    : [[0, 1, 'right', 'left'], [1, 2, compact ? 'bottom' : 'right', 'top'], [2, 3, 'left', 'right'], [3, 0, 'top', 'bottom']];
  return links.map(([from, to, sourceSide, targetSide]) => {
    const start = port(cards[from], sourceSide);
    const end = port(cards[to], targetSide);
    const reach = Math.min(mobile ? 58 : 125, Math.hypot(start.x - end.x, start.y - end.y) * .5);
    const curve = { start, end, c1: control(start, sourceSide, reach), c2: control(end, targetSide, reach) };
    let length = 0;
    let previous = start;
    for (let i = 1; i <= 30; i++) {
      const next = pointAt(curve, i / 30);
      length += Math.hypot(next.x - previous.x, next.y - previous.y);
      previous = next;
    }
    return { from, to, curve, length };
  });
}

export function reverseCurve(curve: Curve): Curve {
  return { start: curve.end, end: curve.start, c1: curve.c2, c2: curve.c1 };
}
