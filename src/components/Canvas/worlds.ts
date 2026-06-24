import { DEFAULT_CANVAS_SETTINGS, type CanvasLayoutSettings } from "./settings";

// §26 WORLDS: per-node content-LOD band driven by APPARENT on-screen width
// (appW = nodeFootprintWorldWidth × cameraScale), NOT raw camera scale (§25.1).
// Each node crosses its own bands when it is physically big enough to read, so a
// wide SPEC and a small TASK reveal their interiors at the right moment.
//   W0 MASS      — territories: titles/mass/coverage, status rings, volume cues.
//   W1 STRUCTURE — section rows / handles / step-thread titles / subtask roster.
//   W2 CONTENT   — bodies reveal (the bug fix): markdown excerpts, step/subtask cards.
//   W3 DEEP      — full readable content, node-internal scroll.
export type World = "W0" | "W1" | "W2" | "W3";
export const WORLD_ORDER: readonly World[] = ["W0", "W1", "W2", "W3"];

export function worldThresholds(kind: string, s: CanvasLayoutSettings = DEFAULT_CANVAS_SETTINGS): [number, number, number] {
  return kind === "spec" ? s.worldThresholdsSpec : s.worldThresholds;
}

export function rawWorld(appW: number, t: [number, number, number]): World {
  if (appW < t[0]) return "W0";
  if (appW < t[1]) return "W1";
  if (appW < t[2]) return "W2";
  return "W3";
}

// Hysteresis (±h, §26.2) at the band boundaries: a node parked near a threshold
// must cross it by ±h to switch, so camera jitter never flickers the band. Large
// jumps (programmatic fit) snap directly; only adjacent moves are damped.
export function nextWorld(
  prev: World | undefined,
  appW: number,
  t: [number, number, number],
  h: number = DEFAULT_CANVAS_SETTINGS.worldHysteresis,
): World {
  const raw = rawWorld(appW, t);
  if (!prev) return raw;
  const pi = WORLD_ORDER.indexOf(prev);
  const ri = WORLD_ORDER.indexOf(raw);
  if (Math.abs(ri - pi) !== 1) return raw;
  const b = Math.min(pi, ri); // boundary t[b] separates band b and b+1
  const lo = t[b] * (1 - h);
  const hi = t[b] * (1 + h);
  if (appW >= hi) return WORLD_ORDER[b + 1];
  if (appW < lo) return WORLD_ORDER[b];
  return prev; // inside the dead zone → hold the previous band
}

export function worldIndex(world: World): number {
  return WORLD_ORDER.indexOf(world);
}

// "Is this node at least band `min`?" — the predicate cards use to gate reveals.
export function atLeast(world: World, min: World): boolean {
  return worldIndex(world) >= worldIndex(min);
}

export function appWidth(footprintWorldWidth: number, cameraScale: number): number {
  return footprintWorldWidth * cameraScale;
}
