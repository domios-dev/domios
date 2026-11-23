// Builds a label-free memory topology from the rollacodebot mem stats counts.
// usage: node playground/src/film/data/generate-topology.mjs
import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const stats = JSON.parse(readFileSync(join(here, "rollacodebot-stats.json"), "utf8"));

const SEED = 0x5eed1a;
const CLUSTERS = 38;
const INTRA_CLUSTER_SHARE = 0.9;
const RELAX_STEPS = 70;
const WORLD_RADIUS = 1000;

function mulberry32(seed) {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

const rand = mulberry32(SEED);
const gauss = () => Math.sqrt(-2 * Math.log(1 - rand())) * Math.cos(2 * Math.PI * rand());
const pareto = (alpha) => (1 - rand()) ** (-1 / alpha);

function pickWeighted(weights, total) {
  let r = rand() * total;
  for (let i = 0; i < weights.length; i++) {
    r -= weights[i];
    if (r <= 0) return i;
  }
  return weights.length - 1;
}

const kinds = Object.keys(stats.by_kind);
const itemCount = kinds.reduce((sum, kind) => sum + stats.by_kind[kind], 0);
const entityCount = stats.total_entities;
const nodeCount = itemCount + entityCount;
const edgeCount = stats.total_relations;

const golden = Math.PI * (3 - Math.sqrt(5));
const clusterSize = Array.from({ length: CLUSTERS }, () => pareto(1.6));
const clusterCenter = clusterSize.map((_, i) => {
  const r = WORLD_RADIUS * 0.82 * Math.sqrt((i + 0.5) / CLUSTERS);
  const a = i * golden;
  return [r * Math.cos(a) * 1.35 + gauss() * 40, r * Math.sin(a) * 0.85 + gauss() * 40];
});
const clusterTotal = clusterSize.reduce((s, x) => s + x, 0);

const kindHomes = kinds.map(() => Array.from({ length: 3 }, () => Math.floor(rand() * CLUSTERS)));

const nodeKind = new Int16Array(nodeCount);
const nodeCluster = new Int16Array(nodeCount);
const fitness = new Float64Array(nodeCount);
let n = 0;
kinds.forEach((kind, k) => {
  for (let i = 0; i < stats.by_kind[kind]; i++, n++) {
    nodeKind[n] = k;
    nodeCluster[n] = rand() < 0.72 ? kindHomes[k][Math.floor(rand() * 3)] : pickWeighted(clusterSize, clusterTotal);
    fitness[n] = pareto(1.9) * 2.4;
  }
});
for (; n < nodeCount; n++) {
  nodeKind[n] = -1;
  nodeCluster[n] = pickWeighted(clusterSize, clusterTotal);
  fitness[n] = pareto(2.3);
}

const members = Array.from({ length: CLUSTERS }, () => []);
for (let i = 0; i < nodeCount; i++) members[nodeCluster[i]].push(i);
const memberFitness = members.map((list) => list.map((i) => fitness[i]));
const memberFitnessTotal = memberFitness.map((list) => list.reduce((s, x) => s + x, 0));
const clusterWeight = memberFitnessTotal.map((x, c) => (members[c].length ? x : 0));
const clusterWeightTotal = clusterWeight.reduce((s, x) => s + x, 0);

const neighbourWeights = clusterCenter.map(([ax, ay], c) =>
  clusterCenter.map(([bx, by], d) => (d === c || !members[d].length ? 0 : clusterWeight[d] / (1 + Math.hypot(ax - bx, ay - by) / 150) ** 3.2)),
);

const pickIn = (c) => members[c][pickWeighted(memberFitness[c], memberFitnessTotal[c])];
const seen = new Set();
const edges = [];
while (edges.length < edgeCount) {
  const c = pickWeighted(clusterWeight, clusterWeightTotal);
  const a = pickIn(c);
  let d = c;
  if (rand() > INTRA_CLUSTER_SHARE) {
    const w = neighbourWeights[c];
    d = pickWeighted(w, w.reduce((s, x) => s + x, 0));
  }
  const b = pickIn(d);
  if (a === b) continue;
  const key = a < b ? a * nodeCount + b : b * nodeCount + a;
  if (seen.has(key)) continue;
  seen.add(key);
  edges.push([a, b]);
}

const degree = new Int32Array(nodeCount);
for (const [a, b] of edges) {
  degree[a]++;
  degree[b]++;
}

const x = new Float64Array(nodeCount);
const y = new Float64Array(nodeCount);
for (let i = 0; i < nodeCount; i++) {
  const c = nodeCluster[i];
  const spread = 26 + Math.sqrt(members[c].length) * 8.5;
  const pull = 1 / (1 + Math.sqrt(degree[i]) * 0.12);
  x[i] = clusterCenter[c][0] + gauss() * spread * pull;
  y[i] = clusterCenter[c][1] + gauss() * spread * pull;
}

const CELL = 20;
for (let step = 0; step < RELAX_STEPS; step++) {
  const cool = 1 - step / RELAX_STEPS;
  const fx = new Float64Array(nodeCount);
  const fy = new Float64Array(nodeCount);
  const grid = new Map();
  for (let i = 0; i < nodeCount; i++) {
    const key = `${Math.floor(x[i] / CELL)},${Math.floor(y[i] / CELL)}`;
    const bucket = grid.get(key);
    if (bucket) bucket.push(i);
    else grid.set(key, [i]);
  }
  for (let i = 0; i < nodeCount; i++) {
    const gx = Math.floor(x[i] / CELL);
    const gy = Math.floor(y[i] / CELL);
    for (let ox = -1; ox <= 1; ox++) {
      for (let oy = -1; oy <= 1; oy++) {
        for (const j of grid.get(`${gx + ox},${gy + oy}`) ?? []) {
          if (j <= i) continue;
          const dx = x[i] - x[j];
          const dy = y[i] - y[j];
          const d2 = dx * dx + dy * dy + 0.01;
          if (d2 > CELL * CELL) continue;
          const f = (CELL * CELL - d2) / d2 * 0.9;
          fx[i] += dx * f * 0.05;
          fy[i] += dy * f * 0.05;
          fx[j] -= dx * f * 0.05;
          fy[j] -= dy * f * 0.05;
        }
      }
    }
  }
  for (const [a, b] of edges) {
    if (nodeCluster[a] !== nodeCluster[b]) continue;
    const dx = x[b] - x[a];
    const dy = y[b] - y[a];
    fx[a] += dx * 0.004;
    fy[a] += dy * 0.004;
    fx[b] -= dx * 0.004;
    fy[b] -= dy * 0.004;
  }
  for (let i = 0; i < nodeCount; i++) {
    const [cx, cy] = clusterCenter[nodeCluster[i]];
    fx[i] += (cx - x[i]) * 0.003;
    fy[i] += (cy - y[i]) * 0.003;
    const len = Math.hypot(fx[i], fy[i]);
    const cap = 6 * cool + 0.5;
    const s = len > cap ? cap / len : 1;
    x[i] += fx[i] * s;
    y[i] += fy[i] * s;
  }
}

const round = (v) => Math.round(v * 10) / 10;
const topology = {
  source: "rollacodebot",
  totals: {
    items: stats.total_items,
    entities: stats.total_entities,
    relations: stats.total_relations,
    vectors: stats.total_vectors,
    nodes: nodeCount,
    edges: edges.length,
  },
  kinds,
  nodes: Array.from({ length: nodeCount }, (_, i) => [round(x[i]), round(y[i]), nodeKind[i], degree[i]]),
  edges,
};

writeFileSync(join(here, "memory-topology.json"), JSON.stringify(topology));
console.log(`memory-topology.json: ${nodeCount} nodes (${itemCount} items, ${entityCount} entities), ${edges.length} edges, max degree ${Math.max(...degree)}`);
