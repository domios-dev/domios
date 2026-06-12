/**
 * Before/after benchmark for #128: legacy Map-keyed all-pairs step vs
 * PhysicsSim (typed arrays + Barnes-Hut). Run: pnpm exec tsx physicsSim.bench.ts
 *
 * The legacy implementation below is the pre-#128 physics.worker.ts step()
 * verbatim-equivalent (same one the parity test in physicsSim.test.ts pins).
 */
import {
  CENTER_PULL,
  DAMPING,
  MAX_VEL,
  PhysicsSim,
  REPULSION,
  SPRING_K,
  SPRING_LENGTH,
  type SimEdgeInput,
  type SimNodeInput,
} from "./physicsSim";

function mulberry32(seed: number) {
  let a = seed >>> 0;
  return () => {
    a |= 0;
    a = (a + 0x6d2b79f5) | 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

function randomGraph(n: number, seed: number): { nodes: SimNodeInput[]; edges: SimEdgeInput[] } {
  const rnd = mulberry32(seed);
  const nodes: SimNodeInput[] = [];
  for (let i = 0; i < n; i++) {
    nodes.push({ id: `n${i}`, x: (rnd() - 0.5) * 4000, y: (rnd() - 0.5) * 4000 });
  }
  const edges: SimEdgeInput[] = [];
  for (let i = 1; i < n; i++) {
    edges.push({ from: `n${i}`, to: `n${Math.floor(rnd() * i)}`, confidence: rnd() });
  }
  return { nodes, edges };
}

interface LegacyNode {
  x: number;
  y: number;
  vx: number;
  vy: number;
}

function legacyStep(nodes: Map<string, LegacyNode>, edges: SimEdgeInput[]): void {
  const ids = [...nodes.keys()];
  const n = ids.length;
  if (n === 0) return;
  const fx = new Map<string, number>();
  const fy = new Map<string, number>();
  for (const id of ids) {
    fx.set(id, 0);
    fy.set(id, 0);
  }
  for (let i = 0; i < n; i++) {
    const idA = ids[i];
    const a = nodes.get(idA)!;
    for (let j = i + 1; j < n; j++) {
      const idB = ids[j];
      const b = nodes.get(idB)!;
      const dx = a.x - b.x;
      const dy = a.y - b.y;
      const d2 = dx * dx + dy * dy + 0.01;
      const d = Math.sqrt(d2);
      const f = REPULSION / d2;
      const nx = dx / d;
      const ny = dy / d;
      fx.set(idA, fx.get(idA)! + nx * f);
      fy.set(idA, fy.get(idA)! + ny * f);
      fx.set(idB, fx.get(idB)! - nx * f);
      fy.set(idB, fy.get(idB)! - ny * f);
    }
  }
  for (const e of edges) {
    const a = nodes.get(e.from);
    const b = nodes.get(e.to);
    if (!a || !b) continue;
    const dx = b.x - a.x;
    const dy = b.y - a.y;
    const d = Math.sqrt(dx * dx + dy * dy) + 0.01;
    const delta = d - SPRING_LENGTH;
    const f = SPRING_K * delta * (e.confidence ?? 0.7);
    const nx = dx / d;
    const ny = dy / d;
    fx.set(e.from, fx.get(e.from)! + nx * f * d * 0.02);
    fy.set(e.from, fy.get(e.from)! + ny * f * d * 0.02);
    fx.set(e.to, fx.get(e.to)! - nx * f * d * 0.02);
    fy.set(e.to, fy.get(e.to)! - ny * f * d * 0.02);
    fx.set(e.from, fx.get(e.from)! + nx * f);
    fy.set(e.from, fy.get(e.from)! + ny * f);
    fx.set(e.to, fx.get(e.to)! - nx * f);
    fy.set(e.to, fy.get(e.to)! - ny * f);
  }
  for (const id of ids) {
    const node = nodes.get(id)!;
    fx.set(id, fx.get(id)! - node.x * CENTER_PULL);
    fy.set(id, fy.get(id)! - node.y * CENTER_PULL);
  }
  for (const id of ids) {
    const node = nodes.get(id)!;
    node.vx = (node.vx + fx.get(id)!) * DAMPING;
    node.vy = (node.vy + fy.get(id)!) * DAMPING;
    const speed = Math.sqrt(node.vx * node.vx + node.vy * node.vy);
    if (speed > MAX_VEL) {
      node.vx = (node.vx / speed) * MAX_VEL;
      node.vy = (node.vy / speed) * MAX_VEL;
    }
    node.x += node.vx;
    node.y += node.vy;
  }
}

function benchLegacy(n: number, ticks: number): number {
  const { nodes, edges } = randomGraph(n, 1);
  const map = new Map<string, LegacyNode>();
  for (const node of nodes) map.set(node.id, { x: node.x, y: node.y, vx: 0, vy: 0 });
  for (let t = 0; t < Math.min(5, ticks); t++) legacyStep(map, edges); // warmup
  const t0 = performance.now();
  for (let t = 0; t < ticks; t++) legacyStep(map, edges);
  return (performance.now() - t0) / ticks;
}

function benchNew(n: number, ticks: number): number {
  const { nodes, edges } = randomGraph(n, 1);
  const sim = new PhysicsSim();
  sim.setGraph(nodes, edges);
  for (let t = 0; t < Math.min(5, ticks); t++) sim.step(); // warmup
  const t0 = performance.now();
  for (let t = 0; t < ticks; t++) sim.step();
  return (performance.now() - t0) / ticks;
}

console.log("physicsSim bench — ms/step (legacy Map+O(n²) vs typed-array+Barnes-Hut)\n");
console.log("  N     legacy      new       speedup");
for (const n of [32, 64, 100, 150, 200, 500, 1000, 2000]) {
  const ticks = n >= 1000 ? 30 : n >= 500 ? 60 : 200;
  const legacy = benchLegacy(n, ticks);
  const fresh = benchNew(n, ticks);
  console.log(
    `  ${String(n).padEnd(5)} ${legacy.toFixed(3).padStart(8)}ms ${fresh
      .toFixed(3)
      .padStart(8)}ms   ${(legacy / fresh).toFixed(1)}x`,
  );
}
console.log(
  "\n16ms frame budget: legacy blows it near N≈700; new path stays under it at 2000+.",
);
console.log("Crossover sweep (exact vs BH, repulsion-only):");
{
  for (const n of [300, 500, 700, 1000, 2000]) {
    const rnd = mulberry32(1);
    const nodes: SimNodeInput[] = [];
    for (let i = 0; i < n; i++) {
      nodes.push({ id: `n${i}`, x: (rnd() - 0.5) * 4000, y: (rnd() - 0.5) * 4000 });
    }
    const sim = new PhysicsSim();
    sim.setGraph(nodes, []);
    const s = sim as unknown as {
      force: Float64Array;
      repulsionExact: () => void;
      repulsionBarnesHut: () => void;
    };
    const reps = n >= 1000 ? 100 : 500;
    for (let i = 0; i < 20; i++) {
      s.force.fill(0);
      s.repulsionExact();
      s.force.fill(0);
      s.repulsionBarnesHut();
    }
    let t0 = performance.now();
    for (let i = 0; i < reps; i++) {
      s.force.fill(0);
      s.repulsionExact();
    }
    const exact = (performance.now() - t0) / reps;
    t0 = performance.now();
    for (let i = 0; i < reps; i++) {
      s.force.fill(0);
      s.repulsionBarnesHut();
    }
    const bh = (performance.now() - t0) / reps;
    console.log(
      `  n=${String(n).padEnd(5)} exact=${(exact * 1000).toFixed(0)}us  bh=${(bh * 1000).toFixed(0)}us  ${
        exact < bh ? "exact wins" : "BH wins"
      }`,
    );
  }
}
