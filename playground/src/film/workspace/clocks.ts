/**
 * One clock per beat. A beat that never started shows its finished state; one that started runs on wall time,
 * keeps running while the visitor looks elsewhere, and freezes as a whole while the host pauses.
 */

export interface Clocks {
  /** Wall time (ms) at each started beat's zero. */
  starts: Readonly<Record<string, number>>;
  /** Wall time the host paused at, or null while running. */
  pausedAt: number | null;
}

export const IDLE: Clocks = { starts: {}, pausedAt: null };

export function startClock(clocks: Clocks, id: string, now: number): Clocks {
  return { ...clocks, starts: { ...clocks.starts, [id]: clocks.pausedAt ?? now } };
}

/** Seconds into the beat; Infinity for a beat that never started, so it reads as finished. */
export function localTime(clocks: Clocks, id: string, now: number): number {
  const start = clocks.starts[id];
  if (start === undefined) return Infinity;
  return ((clocks.pausedAt ?? now) - start) / 1000;
}

export function pauseClocks(clocks: Clocks, now: number): Clocks {
  return clocks.pausedAt === null ? { ...clocks, pausedAt: now } : clocks;
}

/** Shifts every started beat by the time spent paused, so nothing jumps ahead on resume. */
export function resumeClocks(clocks: Clocks, now: number): Clocks {
  if (clocks.pausedAt === null) return clocks;
  const shift = now - clocks.pausedAt;
  return { pausedAt: null, starts: Object.fromEntries(Object.entries(clocks.starts).map(([id, start]) => [id, start + shift])) };
}

/** Whether any started beat has not reached its end yet. */
export function anyRunning(clocks: Clocks, ends: Readonly<Record<string, number>>, now: number): boolean {
  if (clocks.pausedAt !== null) return false;
  return Object.keys(clocks.starts).some((id) => localTime(clocks, id, now) < (ends[id] ?? 0));
}
