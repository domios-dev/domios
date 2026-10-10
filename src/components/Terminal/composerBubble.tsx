export type BubblePose = { x: number; y: number; sx: number; sy: number };

const DRAG_FOLLOW = 0.46;
const LIMIT_X = 48;
const LIMIT_Y = 56;
const ELASTICITY = 0.2;
const STRETCH_MAX = 0.26;
const PINCH = 0.45;
const SCALE_MIN = 0.7;
const SCALE_MAX = 1.3;

const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));

export function rubberBandDelta(
  value: number,
  limit: number,
  elasticity = ELASTICITY,
): { clamped: number; squish: number } {
  const abs = Math.abs(value);
  if (abs <= limit) return { clamped: value, squish: 1 };
  const excess = abs - limit;
  return {
    clamped: Math.sign(value) * (limit + excess * elasticity),
    squish: Math.max(0.78, 1 - excess / 150),
  };
}

// Physical droplet: elongate along the drag axis, pinch the perpendicular one.
// Driven by visible displacement, so a bubble at rest is never deformed.
export function bubblePoseFromPointer(
  x: number,
  y: number,
  startX: number,
  startY: number,
): BubblePose {
  const dx = (x - startX) * DRAG_FOLLOW;
  const dy = (y - startY) * DRAG_FOLLOW;
  const rx = rubberBandDelta(dx, LIMIT_X);
  const ry = rubberBandDelta(dy, LIMIT_Y);
  const stretchX = STRETCH_MAX * Math.min(1, Math.abs(rx.clamped) / LIMIT_X);
  const stretchY = STRETCH_MAX * Math.min(1, Math.abs(ry.clamped) / LIMIT_Y);
  const sx = clamp(rx.squish * (1 + stretchX - PINCH * stretchY), SCALE_MIN, SCALE_MAX);
  const sy = clamp(ry.squish * (1 + stretchY - PINCH * stretchX), SCALE_MIN, SCALE_MAX);
  return { x: rx.clamped, y: ry.clamped, sx, sy };
}
