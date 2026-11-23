import { EDGES, hash, NODES } from "../lab/labGraph";
import type { Layout } from "./holoTheme";

// The hologram's volume: where every memory sits in 3D. Kept free of three.js so a scene can steer its camera at a node
// (the opening dives into one) while the renderer itself still loads as its own chunk.

/** The hologram camera's vertical field of view, and the tangent of its half: stage pixels per world unit follow from it. */
export const FOV = 30;
export const TAN = Math.tan(((FOV / 2) * Math.PI) / 180);

/** How many stage pixels one world unit at depth z covers, for a camera framing the plane at `scale` in a frame `height` tall. */
export function scaleAtDepth(scale: number, height: number, z: number): number {
  const distance = height / 2 / (TAN * scale);
  return (scale * distance) / Math.max(1, distance - z);
}

const N = NODES.length;
const BOUNDS = NODES.reduce((b, [x, y]) => [Math.min(b[0], x), Math.max(b[1], x), Math.min(b[2], y), Math.max(b[3], y)], [Infinity, -Infinity, Infinity, -Infinity]);
const CENTRE: [number, number] = [(BOUNDS[0] + BOUNDS[1]) / 2, (BOUNDS[2] + BOUNDS[3]) / 2];
const RADII: [number, number] = [(BOUNDS[1] - BOUNDS[0]) / 2, (BOUNDS[3] - BOUNDS[2]) / 2];
/** How far clusters spread in depth, either side of the plane: about the layout's full height, so it reads as a body. */
export const DEPTH = 1.9 * RADII[1];

const NEIGHBOURS: number[][] = NODES.map(() => []);
for (const [a, b] of EDGES) NEIGHBOURS[a].push(b), NEIGHBOURS[b].push(a);
const unit = (i: number, salt: number) => (hash(i, salt) / 2 ** 32) * 2 - 1;

/** Repeatedly mixes each value with its neighbours' mean: values settle towards their cluster's. */
function smoothAlong(values: Float32Array, passes: number, keep: number): Float32Array {
  let u = values;
  for (let pass = 0; pass < passes; pass++) {
    const next = new Float32Array(N);
    for (let i = 0; i < N; i++) {
      const list = NEIGHBOURS[i];
      let sum = 0;
      for (const j of list) sum += u[j];
      next[i] = list.length ? keep * u[i] + (1 - keep) * (sum / list.length) : u[i];
    }
    u = next;
  }
  return u;
}

/** Rescales values to mean 0 with `reach` standard deviations at ±1, clamped. */
function spreadOut(u: Float32Array, reach: number): Float32Array {
  let mean = 0;
  for (const v of u) mean += v / N;
  let spread = 0;
  for (const v of u) spread += (v - mean) ** 2 / N;
  const sd = Math.sqrt(spread) || 1;
  return Float32Array.from(u, (v) => Math.max(-1, Math.min(1, (v - mean) / (sd * reach))));
}

/**
 * Where each cluster sits in depth, -1..1: a fixed random draw smoothed along the graph's links so a cluster moves as
 * one, spread back out (smoothing alone pulls every cluster towards the middle, and the volume flattens), then filled
 * out to an ellipsoid so the flat outline becomes a body.
 */
function macroZ(): Float32Array {
  const settled = smoothAlong(Float32Array.from({ length: N }, (_, i) => unit(i, 11)), 22, 0.25);
  const z = spreadOut(settled, 1.8);
  return Float32Array.from(z, (v, i) => {
    const [x, y] = NODES[i];
    const shell = Math.sqrt(Math.max(0.15, 1 - ((x - CENTRE[0]) / RADII[0]) ** 2 - ((y - CENTRE[1]) / RADII[1]) ** 2));
    return v * shell;
  });
}

/**
 * Each node's place in the volume as an offset from its flat position (dx, dy) and a depth z, in NODES' orientation.
 * A cluster in the flat layout is a disc; seen turning it reads as a sliver. Here every member is put on a ball around
 * its cluster's centre instead: its flat direction from the centre kept, a fixed random latitude added.
 */
/** Like smoothAlong, over each node's near neighbours only: its own cluster, without the far ends of its bridges. */
function smoothNear(values: Float32Array, passes: number, keep: number): Float32Array {
  const NEAR = NODES.map(([x, y], i) => NEIGHBOURS[i].filter((j) => Math.hypot(NODES[j][0] - x, NODES[j][1] - y) < 90));
  let u = values;
  for (let pass = 0; pass < passes; pass++) {
    const next = new Float32Array(N);
    for (let i = 0; i < N; i++) {
      const list = NEAR[i];
      let sum = 0;
      for (const j of list) sum += u[j];
      next[i] = list.length ? keep * u[i] + (1 - keep) * (sum / list.length) : u[i];
    }
    u = next;
  }
  return u;
}

// Built on first use, not at module load: the page loads this module with the film, long before the volume is needed.
let volume: Float32Array | null = null;
const vol = () => (volume ??= buildVolume());

function buildVolume(): Float32Array {
  const MACRO_Z = macroZ();
  // A bridge between two clusters averaged over all its links would land in the empty space between them, and a link
  // highlighted to it would end there; its cluster centre comes from its near neighbours only.
  const cx = smoothNear(Float32Array.from(NODES, (n) => n[0]), 6, 0.35);
  const cy = smoothNear(Float32Array.from(NODES, (n) => n[1]), 6, 0.35);
  const d = Float32Array.from(NODES, ([x, y], i) => Math.hypot(x - cx[i], y - cy[i]));
  const radius = smoothNear(d, 4, 0.4);
  const out = new Float32Array(N * 3);
  for (let i = 0; i < N; i++) {
    const [x, y] = NODES[i];
    const ball = Math.max(8, 1.5 * radius[i]);
    // Inside the ball by the cube root of the flat distance, so a hub stays central and its leaves reach the surface.
    const r = ball * Math.min(1, d[i] / ball) ** (1 / 3);
    const w = unit(i, 23);
    const ring = Math.sqrt(1 - w * w);
    const ux = d[i] > 1e-3 ? (x - cx[i]) / d[i] : 1;
    const uy = d[i] > 1e-3 ? (y - cy[i]) / d[i] : 0;
    out[i * 3] = cx[i] + ux * r * ring - x;
    out[i * 3 + 1] = cy[i] + uy * r * ring - y;
    out[i * 3 + 2] = MACRO_Z[i] * DEPTH + r * w;
  }
  return out;
}

/** The portrait frame turns the flat layout a quarter, [x, y] -> [-y, x]; the volume's offsets turn with it. */
export const isTurned = (layout: Layout) => layout[1][0] !== NODES[1][0];

/** Node i's place in the volume, in the renderer's space (three.js: y up): [x, -y, z] of its flat position plus its offset. */
export function volumeOf(i: number, layout: Layout, turned = isTurned(layout)): [number, number, number] {
  const VOL = vol();
  const dx = VOL[i * 3];
  const dy = VOL[i * 3 + 1];
  const [x, y] = layout[i];
  const [ox, oy] = turned ? [-dy, dx] : [dx, dy];
  return [x + ox, -(y + oy), VOL[i * 3 + 2]];
}

const centres = new WeakMap<Layout, [number, number]>();

/** The point the volume turns about: the layout's mean, in flat coordinates. */
export function centreOf(layout: Layout): [number, number] {
  let centre = centres.get(layout);
  if (!centre) {
    let sx = 0;
    let sy = 0;
    for (let i = 0; i < N; i++) (sx += layout[i][0]), (sy += layout[i][1]);
    centres.set(layout, (centre = [sx / N, sy / N]));
  }
  return centre;
}

/**
 * Where node i is right now, once settled in the volume, folded `flatten` of the way back to the plane and turned by
 * yaw and pitch about the centre: flat coordinates (x, y; y down) and depth z towards the camera.
 */
export function placeOf(i: number, layout: Layout, flatten: number, yaw: number, pitch: number): { x: number; y: number; z: number } {
  const [vx, vy, vz] = volumeOf(i, layout);
  const [fx, fy] = layout[i];
  const [cx, cy] = centreOf(layout);
  // Local, renderer space, relative to the centre.
  let x = vx + (fx - vx) * flatten - cx;
  let y = vy + (-fy - vy) * flatten + cy;
  let z = vz * (1 - flatten);
  // three.js Euler XYZ on the pivot: Rx(pitch) · Ry(yaw).
  [x, z] = [x * Math.cos(yaw) + z * Math.sin(yaw), -x * Math.sin(yaw) + z * Math.cos(yaw)];
  [y, z] = [y * Math.cos(pitch) - z * Math.sin(pitch), y * Math.sin(pitch) + z * Math.cos(pitch)];
  return { x: x + cx, y: -(y - cy), z };
}
