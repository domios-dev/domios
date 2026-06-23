export type BubblePose = { x: number; y: number; sx: number; sy: number };

export function rubberBandDelta(
  value: number,
  limit: number,
  elasticity = 0.26,
): { clamped: number; squish: number } {
  const abs = Math.abs(value);
  if (abs <= limit) return { clamped: value, squish: 1 };
  const excess = abs - limit;
  return {
    clamped: Math.sign(value) * (limit + excess * elasticity),
    squish: Math.max(0.78, 1 - excess / 160),
  };
}

export function bubblePoseFromPointer(x: number, y: number, sx: number, sy: number): BubblePose {
  const dx = (x - sx) * 0.44;
  const dy = (y - sy) * 0.44;
  const rx = rubberBandDelta(dx, 46);
  const ry = rubberBandDelta(dy, 54);
  return { x: rx.clamped, y: ry.clamped, sx: rx.squish, sy: ry.squish };
}

export function ComposerSendIcon({ size = 20 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" aria-hidden>
      <path
        d="M7.5 14.5 12 6.5l4.5 8"
        stroke="currentColor"
        strokeWidth="2.35"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
      <path
        d="M5.5 19h13"
        stroke="currentColor"
        strokeWidth="2.35"
        strokeLinecap="round"
        opacity="0.5"
      />
    </svg>
  );
}
