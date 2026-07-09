import type { PresenceSource } from "./presenceOverlays";

export function presenceAnimateDemoEnabled(): boolean {
  if (typeof window === "undefined") return false;
  try {
    return window.localStorage.getItem("canvas.presenceAnimateDemo") === "1";
  } catch {
    return false;
  }
}

export function rotatePresenceForDemo(
  base: ReadonlyMap<string, PresenceSource>,
  tick: number,
): Map<string, PresenceSource> {
  const taskIds = [...base.keys()].sort();
  const out = new Map<string, PresenceSource>();
  const n = taskIds.length;
  if (n < 2 || tick === 0) {
    for (const id of taskIds) out.set(id, base.get(id)!);
    return out;
  }
  const shift = ((tick % n) + n) % n;
  for (let i = 0; i < n; i++) {
    out.set(taskIds[(i + shift) % n], base.get(taskIds[i])!);
  }
  return out;
}
