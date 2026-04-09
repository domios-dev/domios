import type { MemNode, MemEdge } from "./types";

interface LayoutInput {
  nodes: MemNode[];
  edges: MemEdge[];
  iterations?: number;
  width?: number;
  height?: number;
}

export type LayoutOutput = Record<string, { x: number; y: number }>;

function hashString(s: string): number {
  let h = 2166136261 >>> 0;
  for (let i = 0; i < s.length; i++) {
    h ^= s.charCodeAt(i);
    h = Math.imul(h, 16777619);
  }
  return h >>> 0;
}

function mulberry32(seed: number): () => number {
  let t = seed >>> 0;
  return () => {
    t = (t + 0x6d2b79f5) >>> 0;
    let r = Math.imul(t ^ (t >>> 15), 1 | t);
    r = (r + Math.imul(r ^ (r >>> 7), 61 | r)) ^ r;
    return ((r ^ (r >>> 14)) >>> 0) / 4294967296;
  };
}

export function forceLayout(input: LayoutInput): LayoutOutput {
  const iterations = input.iterations ?? 200;
  const width = input.width ?? 1400;
  const height = input.height ?? 1000;
  const area = width * height;
  const n = input.nodes.length;
  if (n === 0) return {};

  const seed = hashString(input.nodes.map((nd) => nd.id).join(""));
  const rand = mulberry32(seed);

  const k = Math.sqrt(area / n);
  const pos: Record<string, { x: number; y: number }> = {};

  for (const node of input.nodes) {
    pos[node.id] = {
      x: (rand() - 0.5) * width,
      y: (rand() - 0.5) * height,
    };
  }

  const idToIdx: Record<string, number> = {};
  input.nodes.forEach((nd, i) => { idToIdx[nd.id] = i; });

  let temp = width / 10;
  const cool = temp / iterations;

  for (let iter = 0; iter < iterations; iter++) {
    const disp: Array<{ x: number; y: number }> = input.nodes.map(() => ({ x: 0, y: 0 }));

    // Repulsive forces
    for (let i = 0; i < n; i++) {
      for (let j = i + 1; j < n; j++) {
        const a = pos[input.nodes[i].id];
        const b = pos[input.nodes[j].id];
        let dx = a.x - b.x;
        let dy = a.y - b.y;
        let d = Math.sqrt(dx * dx + dy * dy);
        if (d < 0.01) { d = 0.01; dx = rand(); dy = rand(); }
        const force = (k * k) / d;
        const fx = (dx / d) * force;
        const fy = (dy / d) * force;
        disp[i].x += fx; disp[i].y += fy;
        disp[j].x -= fx; disp[j].y -= fy;
      }
    }

    // Attractive forces along edges
    for (const e of input.edges) {
      const ai = idToIdx[e.from];
      const bi = idToIdx[e.to];
      if (ai == null || bi == null) continue;
      const a = pos[e.from];
      const b = pos[e.to];
      let dx = a.x - b.x;
      let dy = a.y - b.y;
      let d = Math.sqrt(dx * dx + dy * dy);
      if (d < 0.01) d = 0.01;
      const force = (d * d) / k;
      const fx = (dx / d) * force;
      const fy = (dy / d) * force;
      disp[ai].x -= fx; disp[ai].y -= fy;
      disp[bi].x += fx; disp[bi].y += fy;
    }

    // Apply displacement with temperature cap
    for (let i = 0; i < n; i++) {
      const d = disp[i];
      const mag = Math.sqrt(d.x * d.x + d.y * d.y);
      if (mag < 0.01) continue;
      const capped = Math.min(mag, temp);
      const p = pos[input.nodes[i].id];
      p.x += (d.x / mag) * capped;
      p.y += (d.y / mag) * capped;
    }

    temp = Math.max(0.1, temp - cool);
  }

  return pos;
}
