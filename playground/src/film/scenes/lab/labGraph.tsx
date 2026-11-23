import { useLayoutEffect, useRef } from "react";
import { SpillSurface } from "../../breakout";
import { readMemoryGraphPalette, type MemoryGraphPalette } from "../mesh/holoPalette";
import topology from "../../data/memory-topology.json";
import { useStage } from "../../engine";
import { holoColor, holoPalette, holoRgb, LIGHT_BOOST, themeIsLight } from "../mesh/holoTheme";
import { GraphGl, NODE_TEXELS_PER_ROW, type GlEdges, type GlLayer } from "./graphGl";

export type GraphNode = [x: number, y: number, kind: number, degree: number];

export const NODES = topology.nodes as GraphNode[];
export const EDGES = topology.edges as Array<[number, number]>;
export const KINDS = topology.kinds;
export const TOTALS = topology.totals;

const ALPHA_BUCKETS = 4;
export const hash = (i: number, salt = 0) => (Math.imul(i + 1 + salt * 7919, 2654435761) >>> 0);
const EDGE_BUCKET = EDGES.map((_, i) => hash(i) % ALPHA_BUCKETS);

/** The node memory-graph centres on at its first frame. */
export const HUB = NODES.reduce((best, n, i) => (n[2] >= 0 && n[3] > NODES[best][3] ? i : best), NODES.findIndex((n) => n[2] >= 0));

export const ADJACENCY: number[][] = NODES.map(() => []);
for (const [a, b] of EDGES) {
  ADJACENCY[a].push(b);
  ADJACENCY[b].push(a);
}

/** Every node ranked by hop distance from `from`, ties broken by a fixed hash so the order never changes. */
export function rankFrom(from: number): Int32Array {
  const dist = new Int32Array(NODES.length).fill(-1);
  dist[from] = 0;
  const queue = [from];
  for (let q = 0; q < queue.length; q++) for (const w of ADJACENCY[queue[q]]) if (dist[w] < 0) queue.push(w), (dist[w] = dist[queue[q]] + 1);
  const order = NODES.map((_, i) => i).sort((a, b) => (dist[a] < 0 ? 99 : dist[a]) - (dist[b] < 0 ? 99 : dist[b]) || hash(a) - hash(b));
  const rank = new Int32Array(NODES.length);
  order.forEach((node, r) => (rank[node] = r));
  return rank;
}

/** Kinds from the most common down. The graph wears the hologram's palette everywhere: memories orange, entities blue. */
export const KIND_ORDER = KINDS.map((kind, k) => ({ kind, k, count: NODES.filter((n) => n[2] === k).length })).sort((a, b) => b.count - a.count);

/** World point (cx, cy) lands on screen point (fx, fy); rotation turns the world about that point. */
export interface GraphView {
  cx: number;
  cy: number;
  scale: number;
  fx: number;
  fy: number;
  rotate?: number;
}

/** The view that fits every node inside `box` (stage pixels), centred in it. */
export function fitView(box: { left: number; top: number; right: number; bottom: number }): GraphView {
  let [x0, y0, x1, y1] = [Infinity, Infinity, -Infinity, -Infinity];
  for (const [x, y] of NODES) {
    x0 = Math.min(x0, x);
    x1 = Math.max(x1, x);
    y0 = Math.min(y0, y);
    y1 = Math.max(y1, y);
  }
  const scale = Math.min((box.right - box.left) / (x1 - x0), (box.bottom - box.top) / (y1 - y0));
  return { cx: (x0 + x1) / 2, cy: (y0 + y1) / 2, scale, fx: (box.left + box.right) / 2, fy: (box.top + box.bottom) / 2 };
}

export function project(view: GraphView, x: number, y: number): [number, number] {
  const c = Math.cos(view.rotate ?? 0);
  const s = Math.sin(view.rotate ?? 0);
  const dx = (x - view.cx) * view.scale;
  const dy = (y - view.cy) * view.scale;
  return [view.fx + dx * c - dy * s, view.fy + dx * s + dy * c];
}

export const nodeRadius = (node: GraphNode, scale: number) => (node[2] >= 0 ? 2.4 + Math.sqrt(node[3]) * 0.45 : 1.1 + Math.sqrt(node[3]) * 0.22) * scale ** 0.6;

export interface GraphLayer {
  view: GraphView;
  /** Per-node visibility 0..1; omitted means every node is fully shown. */
  nodeAlpha?: Float32Array;
  /** Per-edge multiplier 0..1 on top of its endpoints' visibility. */
  edgeAlpha?: Float32Array;
  /** World-position overrides for nodes in motion. */
  moved?: Map<number, [number, number]>;
  /** A whole alternative layout in world coordinates, index for index with NODES. */
  positions?: ReadonlyArray<readonly [number, number, ...unknown[]]>;
  /** Extra radius (before zoom) for a set of nodes. */
  lift?: { nodes: ReadonlySet<number>; amount: number };
  /** A second, stronger set of edges drawn over the rest, at `strength` times the palette's ego-edge alpha. */
  highlight?: { edges: ReadonlyArray<readonly [number, number]>; strength: number; width: number };
  edgeBoost?: number;
  opacity?: number;
}

/** Offset (stage pixels) and density (device pixels per stage pixel) from stage space to a canvas. */
interface Placement {
  density: number;
  ox: number;
  oy: number;
}

/** x, y, radius in canvas pixels and visibility per node, laid out for the GPU texture. */
const packed = new Float32Array(Math.ceil(NODES.length / NODE_TEXELS_PER_ROW) * NODE_TEXELS_PER_ROW * 4);

function packNodes(layer: GraphLayer, { density, ox, oy }: Placement): Float32Array {
  const { view, nodeAlpha, moved, positions, lift } = layer;
  const c = Math.cos(view.rotate ?? 0);
  const s = Math.sin(view.rotate ?? 0);
  const zoom = view.scale ** 0.6;
  for (let i = 0; i < NODES.length; i++) {
    const world = moved?.get(i) ?? positions?.[i] ?? NODES[i];
    const dx = (world[0] - view.cx) * view.scale;
    const dy = (world[1] - view.cy) * view.scale;
    const k = Math.max(0, nodeAlpha ? nodeAlpha[i] : 1);
    let r = nodeRadius(NODES[i], view.scale) + (lift?.nodes.has(i) ? lift.amount * zoom : 0);
    if (k < 1) r *= 0.4 + 0.6 * k;
    packed[i * 4] = (view.fx + dx * c - dy * s + ox) * density;
    packed[i * 4 + 1] = (view.fy + dx * s + dy * c + oy) * density;
    packed[i * 4 + 2] = r * density;
    packed[i * 4 + 3] = k;
  }
  return packed;
}

function edgeLook(palette: MemoryGraphPalette, layer: GraphLayer) {
  const { view, edgeBoost = 1, opacity = 1 } = layer;
  const zoomFade = Math.min(1.15, 0.85 + 0.15 * view.scale);
  // Light palettes start from much higher alphas; the film's boosts would saturate the deep blue to a solid sheet.
  const light = themeIsLight();
  const buckets = [0, 1, 2, 3].map((bucket) => {
    const boosted = palette.realEdgeAlpha(bucket / (ALPHA_BUCKETS - 1)) * 2.2 * zoomFade * edgeBoost * opacity;
    // The hologram's blue is darker than the grey foreground it replaces, so the dark theme lifts it to read as lit.
    return light ? Math.min(boosted * 0.55 * LIGHT_BOOST, 0.24 * LIGHT_BOOST * opacity) : boosted * 1.5;
  }) as [number, number, number, number];
  return { buckets, width: Math.max(0.6, 0.55 * Math.sqrt(view.scale)) };
}

const ENTITY_ALPHA = 0.55;
/** The highlighted links of one memory, in its orange; a light page takes them near full strength. */
export const egoAlpha = (palette: MemoryGraphPalette) => Math.min(0.95, palette.egoEdgeAlpha * 1.4 * (themeIsLight() ? LIGHT_BOOST : 1));
/** Entities on a light page: deep blue at nearly full strength, or the body of the graph washes out. */
const entityAlpha = (light: boolean) => (light ? Math.min(0.95, ENTITY_ALPHA * LIGHT_BOOST) : ENTITY_ALPHA);
const css = (rgb: readonly number[]) => rgb.map((c) => Math.round(c * 255)).join(",");

const ENTITIES = [...NODES.keys()].filter((i) => NODES[i][2] < 0);
const KIND_MEMBERS = KINDS.map((_, k) => [...NODES.keys()].filter((i) => NODES[i][2] === k));
const NODE_ORDER = new Float32Array([...ENTITIES.flatMap((i) => [i, -1]), ...KIND_ORDER.flatMap(({ k }) => KIND_MEMBERS[k].flatMap((i) => [i, k]))]);
const GL_EDGES: GlEdges = { pairs: new Float32Array(EDGES.flatMap(([a, b], e) => [a, b, EDGE_BUCKET[e]])), count: EDGES.length };
const highlightEdges = new WeakMap<object, GlEdges>();
function glEdges(pairs: ReadonlyArray<readonly [number, number]>): GlEdges {
  let edges = highlightEdges.get(pairs);
  if (!edges) highlightEdges.set(pairs, (edges = { pairs: new Float32Array(pairs.flatMap(([a, b]) => [a, b, 0])), count: pairs.length }));
  return edges;
}

function kindRgb(light: boolean): number[] {
  const out = new Array(16 * 3).fill(0.5);
  KINDS.forEach((_, k) => holoRgb(k, light).forEach((c, i) => (out[k * 3 + i] = c)));
  return out;
}

/** One node buffer per layer slot, reused every frame instead of a fresh copy per layer (phones stutter on the churn). */
const LAYER_NODES: Float32Array[] = [];
function layerNodes(slot: number, packedNodes: Float32Array): Float32Array {
  let buffer = LAYER_NODES[slot];
  if (!buffer) LAYER_NODES[slot] = buffer = new Float32Array(packedNodes.length);
  buffer.set(packedNodes);
  return buffer;
}

function glLayers(palette: MemoryGraphPalette, layers: GraphLayer[], placement: Placement): GlLayer[] {
  const holo = holoPalette(themeIsLight());
  return layers.filter((layer) => (layer.opacity ?? 1) > 0).map((layer, slot) => {
    const { buckets, width } = edgeLook(palette, layer);
    const opacity = layer.opacity ?? 1;
    const { highlight } = layer;
    return {
      // Copied into the layer's own slot: every layer is uploaded in turn within one render.
      nodes: layerNodes(slot, packNodes(layer, placement)),
      edgeAlpha: layer.edgeAlpha,
      edgeRgb: holo.edge,
      entityRgb: holo.entity,
      bucketAlpha: buckets,
      edgeHalfWidth: (width * placement.density) / 2,
      extra: highlight && { edges: glEdges(highlight.edges), alpha: egoAlpha(palette) * highlight.strength * opacity, halfWidth: (highlight.width * placement.density) / 2, rgb: holo.ego },
      entityAlpha: entityAlpha(themeIsLight()),
      opacity,
    };
  });
}

/** Opacity levels a partly revealed node or edge snaps to on the 2D path, so its draw calls stay bounded. */
const LEVELS = 8;

/** The 2D fallback for browsers without WebGL2: one path per bucket and opacity level. */
export function drawGraph(context: CanvasRenderingContext2D, palette: MemoryGraphPalette, layer: GraphLayer) {
  const { edgeAlpha, opacity = 1, highlight } = layer;
  if (opacity <= 0) return;
  const nodes = packNodes(layer, { density: 1, ox: 0, oy: 0 });
  const { buckets, width } = edgeLook(palette, layer);
  const level = (k: number) => Math.ceil(Math.min(1, k) * LEVELS);
  const light = themeIsLight();
  const holo = holoPalette(light);
  const edgeRgb = css(holo.edge);

  context.lineWidth = width;
  for (let bucket = 0; bucket < ALPHA_BUCKETS; bucket++) {
    const paths = new Map<number, Path2D>();
    for (let e = 0; e < EDGES.length; e++) {
      if (EDGE_BUCKET[e] !== bucket) continue;
      const [a, b] = EDGES[e];
      const q = level(Math.min(nodes[a * 4 + 3], nodes[b * 4 + 3]) * (edgeAlpha ? edgeAlpha[e] : 1));
      if (q <= 0) continue;
      let path = paths.get(q);
      if (!path) paths.set(q, (path = new Path2D()));
      path.moveTo(nodes[a * 4], nodes[a * 4 + 1]);
      path.lineTo(nodes[b * 4], nodes[b * 4 + 1]);
    }
    for (const [q, path] of paths) {
      context.strokeStyle = `rgba(${edgeRgb},${(buckets[bucket] * q) / LEVELS})`;
      context.stroke(path);
    }
  }
  if (highlight && highlight.strength > 0) {
    context.strokeStyle = `rgba(${css(holo.ego)},${egoAlpha(palette) * highlight.strength * opacity})`;
    context.lineWidth = highlight.width;
    context.beginPath();
    for (const [a, b] of highlight.edges) {
      context.moveTo(nodes[a * 4], nodes[a * 4 + 1]);
      context.lineTo(nodes[b * 4], nodes[b * 4 + 1]);
    }
    context.stroke();
  }

  const fill = (color: string, members: number[], alpha: number) => {
    const paths = new Map<number, Path2D>();
    for (const i of members) {
      const q = level(nodes[i * 4 + 3]);
      if (q <= 0) continue;
      let path = paths.get(q);
      if (!path) paths.set(q, (path = new Path2D()));
      const x = nodes[i * 4];
      const y = nodes[i * 4 + 1];
      const r = nodes[i * 4 + 2];
      path.moveTo(x + r, y);
      path.arc(x, y, r, 0, Math.PI * 2);
    }
    context.fillStyle = color;
    for (const [q, path] of paths) {
      context.globalAlpha = (alpha * opacity * q) / LEVELS;
      context.fill(path);
    }
  };
  fill(`rgb(${css(holo.entity)})`, ENTITIES, entityAlpha(light));
  for (const { k } of KIND_ORDER) fill(holoColor(k, light), KIND_MEMBERS[k], 1);
  context.globalAlpha = 1;
}

/** The palette only changes with the theme, so it is read once per theme rather than with a style recalc every frame. */
let paletteCache: { key: string; palette: MemoryGraphPalette } | null = null;
function currentPalette(): MemoryGraphPalette {
  const root = document.documentElement;
  const key = `${root.getAttribute("data-ct-theme")}|${root.className}|${root.getAttribute("style")}`;
  if (paletteCache?.key !== key) paletteCache = { key, palette: readMemoryGraphPalette() };
  return paletteCache.palette;
}

type Hole = { x: number; y: number; width: number; height: number };

/** Draws layers onto one canvas through WebGL2 when it can, else through the 2D path; `hole` (stage pixels) stays empty. */
function useGraphPainter() {
  const renderer = useRef<{ canvas: HTMLCanvasElement; gl: GraphGl | null; light: boolean } | null>(null);
  useLayoutEffect(() => () => {
    renderer.current?.gl?.dispose();
    renderer.current = null;
  }, []);
  return (canvas: HTMLCanvasElement, layers: GraphLayer[], placement: Placement, hole?: Hole) => {
    const light = themeIsLight();
    if (renderer.current?.canvas !== canvas) {
      renderer.current?.gl?.dispose();
      renderer.current = { canvas, gl: GraphGl.create(canvas, GL_EDGES, NODE_ORDER, ENTITIES.length, kindRgb(light)), light };
    } else if (renderer.current.light !== light) {
      // Same canvas, new theme: only the kind colours change (a disposed context can't be had again from its canvas).
      if (renderer.current.gl) renderer.current.gl.kindRgb = kindRgb(light);
      renderer.current.light = light;
    }
    const palette = currentPalette();
    const { density } = placement;
    const gl = renderer.current.gl;
    if (gl) {
      gl.render(glLayers(palette, layers, placement), hole && { x: hole.x * density, y: hole.y * density, width: hole.width * density, height: hole.height * density });
      return;
    }
    const context = canvas.getContext("2d");
    if (!context) return;
    context.setTransform(1, 0, 0, 1, 0, 0);
    context.clearRect(0, 0, canvas.width, canvas.height);
    context.setTransform(density, 0, 0, density, placement.ox * density, placement.oy * density);
    for (const layer of layers) drawGraph(context, palette, layer);
    if (hole) {
      context.setTransform(density, 0, 0, density, 0, 0);
      context.clearRect(hole.x, hole.y, hole.width, hole.height);
    }
  };
}

function GraphSpill({ t, layers }: { t: number; layers: GraphLayer[] }) {
  const paint = useGraphPainter();
  return (
    <SpillSurface
      t={t}
      paint={(canvas, { width, height, reachX, reachY, density }) => paint(canvas, layers, { density, ox: reachX, oy: reachY }, { x: reachX, y: reachY, width, height })}
    />
  );
}

/** The stage copy never needs more backing pixels than this per stage pixel. */
const STAGE_DENSITY = 2;

/**
 * A stage-sized graph canvas redrawn for every t, backed by only as many pixels as it covers on screen, plus its spill
 * past the frame. `overlay` draws 2D marks over the stage copy.
 */
export function StageCanvas({ t, layers, overlay }: { t: number; layers: GraphLayer[]; overlay?: (context: CanvasRenderingContext2D) => void }) {
  const ref = useRef<HTMLCanvasElement>(null);
  const overlayRef = useRef<HTMLCanvasElement>(null);
  const { width, height, density: screen } = useStage();
  const density = Math.min(screen, STAGE_DENSITY);
  const paint = useGraphPainter();

  useLayoutEffect(() => {
    const bw = Math.round(width * density);
    const bh = Math.round(height * density);
    for (const canvas of [ref.current, overlayRef.current]) {
      if (canvas && (canvas.width !== bw || canvas.height !== bh)) {
        canvas.width = bw;
        canvas.height = bh;
      }
    }
    if (ref.current) paint(ref.current, layers, { density, ox: 0, oy: 0 });
    const context = overlay && overlayRef.current?.getContext("2d");
    if (context && overlay) {
      context.setTransform(1, 0, 0, 1, 0, 0);
      context.clearRect(0, 0, bw, bh);
      context.setTransform(density, 0, 0, density, 0, 0);
      overlay(context);
    }
  }, [t, width, height, density]);

  return (
    <>
      <canvas ref={ref} aria-hidden="true" data-film-hint="Memory graph: a year of what your agents learned, linked by meaning" style={{ position: "absolute", inset: 0, width, height }} />
      {overlay && <canvas ref={overlayRef} aria-hidden="true" style={{ position: "absolute", inset: 0, width, height, pointerEvents: "none" }} />}
      <GraphSpill t={t} layers={layers} />
    </>
  );
}

export const fmt = (n: number) => Math.round(n).toLocaleString("en-US");
