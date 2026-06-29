import type { PresenceSource } from "./presenceOverlays";

// Dev harness, LOCAL-ONLY (EMERGENCE audit): flip localStorage
// "canvas.presenceAnimateDemo"="1" to periodically reassign the synthetic swarm
// between tasks, so you SEE the presence pins GLIDE (presenceGlide.ts). Off and
// side-effect-free unless set. Real agent→task transitions are a DATA DEPENDENCY:
// they will come from the pane/agent registry, not this rotator.
export function presenceAnimateDemoEnabled(): boolean {
  if (typeof window === "undefined") return false;
  try {
    return window.localStorage.getItem("canvas.presenceAnimateDemo") === "1";
  } catch {
    return false;
  }
}

// Advance the swarm one hop: each source moves to the next host task around the
// ring of presence-bearing tasks, preserving the agent identity (so the glide
// controller sees a real A→B move) while leaving every other field untouched.
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
