import { DEFAULT_CANVAS_SETTINGS, type CanvasLayoutSettings } from "./settings";

export type World = "W0" | "W1" | "W2" | "W3";
export const World = { W0: "W0", W1: "W1", W2: "W2", W3: "W3" } as const satisfies Record<string, World>;
export const WORLD_ORDER: readonly World[] = [World.W0, World.W1, World.W2, World.W3];

export function worldThresholds(kind: string, s: CanvasLayoutSettings = DEFAULT_CANVAS_SETTINGS): [number, number, number] {
  return kind === "spec" ? s.worldThresholdsSpec : s.worldThresholds;
}

export function rawWorld(appW: number, t: [number, number, number]): World {
  if (appW < t[0]) return "W0";
  if (appW < t[1]) return "W1";
  if (appW < t[2]) return "W2";
  return "W3";
}

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
