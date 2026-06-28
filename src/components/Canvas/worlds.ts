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
export const World = { W0: "W0", W1: "W1", W2: "W2", W3: "W3" } as const satisfies Record<string, World>;
export const WORLD_ORDER: readonly World[] = [World.W0, World.W1, World.W2, World.W3];
export const DEEP_DIVE_CRISP_SCALE = 7.5;

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

// §34 CRISP TEXT: the whole world zooms via a single `transform: scale(cam)` div, so
// a node's 1× raster is GPU-upscaled at deep zoom → blurry glyphs ("shakal" at 400%).
// Readable-band nodes (W2/W3) re-rasterize at on-screen size: the card is `zoom`-ed
// by `cam` (real layout reflow → glyphs shaped at display px) then counter-scaled by
// transform: scale(1/cam) back to world size. Net geometry is unchanged (§22/§31),
// only the raster is produced at display resolution. Quantized to 0.25 steps so the
// reflow fires a bounded number of times across a zoom sweep, not every frame; the
// SAME value drives zoom AND the counter-transform, so they cancel to exactly 1×
// world size regardless of rounding (rounding only shifts raster sharpness ±a few %).
export const CRISP_MIN_SCALE = 1.05;
export function crispRasterScale(world: World, cameraScale: number): number {
  if (!atLeast(world, "W2") || cameraScale <= CRISP_MIN_SCALE) return 1;
  return Math.max(1, Math.round(cameraScale * 4) / 4);
}

// §35 reading-size ladder. Under the single camera transform on-screen px =
// worldFontPx × cameraScale, so a fixed-13px world root balloons text at deep zoom
// (an H1 hit ~60px at 5×). Instead, at the reading bands (W2/W3) hold the on-screen
// size near a comfortable target and let overflow SCROLL — never grow the glyphs.
// The camera is quantized to 0.5 steps so the size is DISCRETE and stable within a
// band (no continuous reflow); it only re-steps on a real zoom move. Glance bands
// (W0/W1) keep the world-fixed 13px label layer. Returns the world-px font for the
// node's em root; the em cascade resizes every child in proportion.
export const READING_ON_SCREEN_PX = 13;
export function readingRootFontPx(world: World, cameraScale: number): number {
  if (!atLeast(world, "W2") || cameraScale <= 1) return READING_ON_SCREEN_PX;
  const step = Math.max(1, Math.round(cameraScale * 2) / 2);
  return Math.max(2.5, Math.min(READING_ON_SCREEN_PX, READING_ON_SCREEN_PX / step));
}
