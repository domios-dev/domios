import { readMemoryGraphPalette } from "./holoPalette";
import topology from "../../data/memory-topology.json";

// The memory graph's palette everywhere it is drawn, flat or as the hologram, plus what the hologram scene needs without
// loading three.js: its state and the theme test.

/** Kind indices from the most common down; tints are handed out in this order. */
const KIND_RANK: number[] = (() => {
  const counts = topology.kinds.map((_, k) => topology.nodes.filter((n) => n[2] === k).length);
  const order = counts.map((_, k) => k).sort((a, b) => counts[b] - counts[a]);
  const rank: number[] = [];
  order.forEach((k, r) => (rank[k] = r));
  return rank;
})();

/** Flat layout, index for index with NODES, in the frame's own orientation (portrait turns it a quarter). */
export type Layout = ReadonlyArray<readonly [number, number, ...unknown[]]>;

export interface HoloState {
  /** Camera: world point (x, y) at the stage centre, `scale` stage pixels per world unit on the flat plane. */
  view: { x: number; y: number; scale: number };
  /** Seconds since the burst began; nodes sit flat before 0. */
  burst: number;
  /** 0 = flat layout, 1 = the burst's volume; pulls everything back into the plane. */
  flatten: number;
  /** 0 = the graph's own kind colours, 1 = the hologram's blue and orange. */
  holo: number;
  /** Turn of the whole volume about its centre, radians. */
  yaw: number;
  pitch: number;
  /** Strength of the first memory's own links. */
  ego: number;
  /** World y of the scan band, or null for none. */
  scan: number | null;
  /** The shockwave leaving the first memory: 0..1 through its travel, or null. */
  ring: number | null;
  opacity: number;
  /** 1: every node starts at `from` (flat world point) and launches from it; 0: they start on the flat layout. */
  collapse?: number;
  from?: readonly [number, number, ...unknown[]];
  /** Instead of `from`: gather into (or launch from) this node's own place in the volume. */
  fromNode?: number;
  /** Seconds one node's flight takes, the overshoot past its place (0 = none) and how much of the hop-distance stagger to use. */
  flight?: number;
  overshoot?: number;
  stagger?: number;
  /** Seconds since a wave of light set off through the relations from the first memory, or null. */
  wave?: number | null;
  /** The new memory's arrival, 0 (far outside) to 1 (in place); null when it is simply in place. */
  arrival?: number | null;
}

/** Per instance: whose links the ego highlight draws, and which node arrives from outside (or -1). */
export interface HoloCast {
  ego: number;
  fresh: number;
}

export type Rgb = [number, number, number];
export const hex = (h: string): Rgb => [0, 2, 4].map((o) => parseInt(h.slice(1 + o, 3 + o), 16) / 255) as Rgb;

/** Memories glow orange, one tint per kind from the most common down; entities and relations are the blue body. */
export const HOLO_DARK = {
  kinds: ["#ff8a2b", "#ffb057", "#ff6a1a", "#ffc888", "#ff9a45", "#ffd9a8", "#ff7d36", "#ffbd6e", "#ff9f5a", "#ffe6c4"].map(hex),
  entity: hex("#5ab8ff"),
  edge: hex("#3d8bff"),
  ego: hex("#ff9a3c"),
  ring: [hex("#ffb24d"), hex("#3d9bff")] as const,
};
/** On a light page the hologram needs depth, not glow: deep blue and a burnt orange, drawn at about twice the dark opacities. */
export const HOLO_LIGHT = {
  kinds: ["#d6620a", "#c75508", "#b84a06", "#dd7317", "#c95e0c", "#e0802a", "#b9520a", "#d26b12", "#c45a0e", "#e08a3a"].map(hex),
  entity: hex("#095cc4"),
  edge: hex("#095cc4"),
  ego: hex("#d6620a"),
  ring: [hex("#d6620a"), hex("#095cc4")] as const,
};

/** How much stronger the light theme draws what the dark theme draws faintly. */
export const LIGHT_BOOST = 1.9;

/** The hologram's tint for a kind index, or -1 for entities. */
export function holoRgb(kind: number, light: boolean): Rgb {
  const palette = light ? HOLO_LIGHT : HOLO_DARK;
  return kind < 0 ? palette.entity : palette.kinds[KIND_RANK[kind] % palette.kinds.length];
}

export const holoPalette = (light: boolean) => (light ? HOLO_LIGHT : HOLO_DARK);

/** The one memory's own orange, as CSS: the seed of the burst, the beacon, the rings around it. */
export function memoryColor(): string {
  return `rgb(${holoPalette(themeIsLight()).ego.map((c) => Math.round(c * 255)).join(",")})`;
}

/** The same tint as CSS, for the legend. */
export function holoColor(kind: number, light: boolean): string {
  return `rgb(${holoRgb(kind, light).map((c) => Math.round(c * 255)).join(",")})`;
}

/** Light palettes are read once per theme, not with a style recalc every frame. */
let lightCache: { key: string; light: boolean } | null = null;
export function themeIsLight(): boolean {
  const root = document.documentElement;
  const key = `${root.getAttribute("data-ct-theme")}|${root.className}|${root.getAttribute("style")}`;
  // The palette derives every alpha from the background's brightness; light backgrounds start their edges at 0.1.
  if (lightCache?.key !== key) lightCache = { key, light: readMemoryGraphPalette().realEdgeAlpha(0) > 0.05 };
  return lightCache.light;
}
