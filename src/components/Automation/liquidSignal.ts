const clamp = (value: number) => Math.max(0, Math.min(1, value));

export function liquidStretch(velocity: number, landing: number | null = null) {
  const stretch = landing === null ? Math.min(0.32, Math.abs(velocity) * 0.12) : Math.max(-1, Math.min(1, landing)) * 0.22;
  const scaleX = 1 + stretch;
  return { scaleX, scaleY: 1 / Math.sqrt(Math.max(0.7, scaleX)) };
}

export function liquidSignal(progress: number) {
  const p = clamp(progress);
  const travel = clamp(p / 0.8);
  const landing = clamp((p - 0.8) / 0.2);
  const velocity = 6 * travel * (1 - travel) / 0.8;
  const stretch = liquidStretch(velocity, p >= 0.8 ? Math.cos(landing * Math.PI * 3) * Math.exp(-landing * 5) : null);
  return {
    distance: travel * travel * (3 - 2 * travel),
    opacity: Math.min(1, p / 0.08) * (1 - landing),
    landing,
    ...stretch,
  };
}
