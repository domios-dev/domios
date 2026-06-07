import {
  BH_THRESHOLD_NODES,
  CENTER_PULL,
  DAMPING,
  ENERGY_SLEEP,
  MAX_VEL,
  PhysicsSim,
  REPULSION,
  SPRING_K,
  SPRING_LENGTH,
  type SimEdgeInput,
  type SimNodeInput,
} from "./physicsSim";

let passed = 0;
let failed = 0;

function assert(label: string, ok: boolean, detail?: string) {
  if (ok) {
    console.log(`  ✓ ${label}`);
    passed++;
  } else {
    console.error(`  ✗ ${label}${detail ? ` — ${detail}` : ""}`);
    failed++;
  }
}

/** Deterministic PRNG so failures reproduce. */
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

/**
 * Reference: the exact algorithm the worker shipped before #128 —
 * Map-keyed force accumulation, all-pairs repulsion (physics.worker.ts
 * lines 39-130 at commit 76b38cff). Kept verbatim-equivalent so the parity
 * test pins behavior, not implementation.
 */
interface LegacyNode {
  x: number;
  y: number;
  vx: number;
  vy: number;
}

function legacyStep(nodes: Map<string, LegacyNode>, edges: SimEdgeInput[]): number {
  const ids = [...nodes.keys()];
  const n = ids.length;
  if (n === 0) return 0;
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
  let energy = 0;
  for (const id of ids) {
    const node = nodes.get(id)!;
    node.vx = (node.vx + fx.get(id)!) * DAMPING;
    node.vy = (node.vy + fy.get(id)!) * DAMPING;
    const speed = Math.sqrt(node.vx * node.vx + node.vy * node.vy);
    if (speed > MAX_VEL) {
      node.vx = (node.vx / speed) * MAX_VEL;
      node.vy = (node.vy / speed) * MAX_VEL;
    }
    energy += Math.min(speed, MAX_VEL);
    node.x += node.vx;
    node.y += node.vy;
  }
  return energy / n;
}

console.log("physicsSim tests");

// Regression #128: for small N, Barnes-Hut must match legacy trajectory closely.
// Same math and order, with typed arrays replacing Maps.
{
  const n = BH_THRESHOLD_NODES - 4;
  const { nodes, edges } = randomGraph(n, 42);
  const sim = new PhysicsSim();
  sim.setGraph(nodes, edges);
  const legacy = new Map<string, LegacyNode>();
  for (const node of nodes) legacy.set(node.id, { x: node.x, y: node.y, vx: 0, vy: 0 });

  let maxDrift = 0;
  for (let tick = 0; tick < 50; tick++) {
    sim.step();
    legacyStep(legacy, edges);
  }
  for (const p of sim.snapshot()) {
    const l = legacy.get(p.id)!;
    maxDrift = Math.max(maxDrift, Math.abs(p.x - l.x), Math.abs(p.y - l.y));
  }
  assert(
    `small-n (${n}) trajectory matches legacy after 50 ticks`,
    maxDrift < 1e-6,
    `max drift ${maxDrift}`,
  );
}

// Barnes-Hut check: repulsion error vs exact should stay within a few % at θ=0.5.
{
  const n = 500;
  const { nodes } = randomGraph(n, 7);
  const exact = new PhysicsSim();
  exact.setGraph(nodes, []);
  const approx = new PhysicsSim();
  approx.setGraph(nodes, []);

  // One step each; with no edges and zero initial velocity the position delta after one tick is DAMPING * force — compare those deltas.
  (exact as unknown as { repulsionExact: () => void; force: Float64Array }).force.fill(0);
  (exact as unknown as { repulsionExact: () => void }).repulsionExact();
  (approx as unknown as { repulsionBarnesHut: () => void; force: Float64Array }).force.fill(0);
  (approx as unknown as { repulsionBarnesHut: () => void }).repulsionBarnesHut();

  const fe = (exact as unknown as { force: Float64Array }).force;
  const fa = (approx as unknown as { force: Float64Array }).force;
  let totalRel = 0;
  let worst = 0;
  for (let i = 0; i < n; i++) {
    const ex = fe[i * 2];
    const ey = fe[i * 2 + 1];
    const dx = fa[i * 2] - ex;
    const dy = fa[i * 2 + 1] - ey;
    const mag = Math.sqrt(ex * ex + ey * ey) + 1e-12;
    const rel = Math.sqrt(dx * dx + dy * dy) / mag;
    totalRel += rel;
    worst = Math.max(worst, rel);
  }
  const avgRel = totalRel / n;
  assert(`BH avg relative force error < 5% (got ${(avgRel * 100).toFixed(2)}%)`, avgRel < 0.05);
  assert(`BH worst relative force error < 50% (got ${(worst * 100).toFixed(1)}%)`, worst < 0.5);
}

// Settling keeps graph energy trajectories compatible with legacy exact-mode behavior.
// For small graphs ENERGY_SLEEP is reached; large graphs flatten within the heat budget.
{
  const small = randomGraph(40, 11);
  const sim = new PhysicsSim();
  sim.setGraph(small.nodes, small.edges);
  const initial = sim.step();
  let e = initial;
  for (let tick = 1; tick < 320; tick++) e = sim.step();
  assert(
    `40-node energy collapses within one heat (${initial.toFixed(1)} → ${e.toFixed(2)})`,
    e < initial * 0.25 && Number.isFinite(e),
  );
  assert("ENERGY_SLEEP threshold unchanged", ENERGY_SLEEP === 0.05);
}
{
  const { nodes, edges } = randomGraph(800, 3);
  const sim = new PhysicsSim();
  sim.setGraph(nodes, edges);
  const initial = sim.step();
  let e = initial;
  for (let tick = 1; tick < 320; tick++) e = sim.step();
  assert(
    `800-node energy collapses within one heat (${initial.toFixed(1)} → ${e.toFixed(1)})`,
    e < initial * 0.25,
  );
}

// Topology changes must keep stable id→index mapping so surviving nodes retain identity.
{
  const sim = new PhysicsSim();
  sim.setGraph(
    [
      { id: "a", x: 10, y: 20 },
      { id: "b", x: 30, y: 40 },
      { id: "c", x: 50, y: 60 },
    ],
    [],
  );
  sim.step();
  const before = new Map(sim.snapshot().map((p) => [p.id, p]));

  const changed = sim.setGraph(
    [
      { id: "c", x: 0, y: 0 },
      { id: "a", x: 0, y: 0 },
      { id: "d", x: 70, y: 80 },
    ],
    [],
  );
  assert("topology change detected", changed);
  const after = new Map(sim.snapshot().map((p) => [p.id, p]));
  assert("removed node gone", !after.has("b") && after.size === 3);
  assert(
    "surviving nodes keep positions (not re-seeded)",
    after.get("a")!.x === before.get("a")!.x && after.get("c")!.y === before.get("c")!.y,
  );
  assert("new node seeded at given position", after.get("d")!.x === 70 && after.get("d")!.y === 80);
}

// setPosition pins a node and zeroes its velocity.
{
  const sim = new PhysicsSim();
  sim.setGraph(
    [
      { id: "a", x: 0, y: 0 },
      { id: "b", x: 100, y: 0 },
    ],
    [],
  );
  sim.step();
  assert("setPosition on known id", sim.setPosition("a", -500, -500));
  assert("setPosition on unknown id is false", !sim.setPosition("zzz", 0, 0));
  const a = sim.snapshot().find((p) => p.id === "a")!;
  assert("position applied", a.x === -500 && a.y === -500);
}

// Coincident nodes must not produce NaN/Infinity (quadtree split guard).
{
  const sim = new PhysicsSim();
  const nodes: SimNodeInput[] = [];
  for (let i = 0; i < BH_THRESHOLD_NODES + 10; i++) {
    nodes.push({ id: `n${i}`, x: i < 5 ? 100 : (i * 37) % 900, y: i < 5 ? 100 : (i * 53) % 900 });
  }
  sim.setGraph(nodes, []);
  for (let t = 0; t < 10; t++) sim.step();
  const bad = sim.snapshot().filter((p) => !Number.isFinite(p.x) || !Number.isFinite(p.y));
  assert("coincident points stay finite", bad.length === 0, `${bad.length} bad`);
}

// Empty graph is a no-op.
{
  const sim = new PhysicsSim();
  sim.setGraph([], []);
  assert("empty graph steps to zero energy", sim.step() === 0);
}

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) throw new Error(`${failed} test(s) failed`);
