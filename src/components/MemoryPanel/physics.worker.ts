/// <reference lib="webworker" />
import type {
  PhysicsEdge,
  PhysicsNode,
  PhysicsPosition,
  PhysicsRequest,
  PhysicsResponse,
} from "./physicsProtocol";

// Lower repulsion — unconnected nodes don't fly apart on their own.
// Cluster-to-cluster separation comes from each cluster's collective mass
// pushing outward, amplified by the tighter spring (more cohesive cluster).
const REPULSION = 120000;
const SPRING_LENGTH = 700;
const SPRING_K = 0.010;
const CENTER_PULL = 0.00001;
const DAMPING = 0.78;
const MAX_VEL = 35;
const ENERGY_SLEEP = 0.05;
const TICK_INTERVAL_MS = 16;
// Hard cool-down: after this many ticks since the last heat the simulation
// stops regardless of residual energy. Without this the slow CENTER_PULL
// drift compounds forever as new nodes arrive.
const MAX_TICKS_PER_HEAT = 320; // ~5s at 16ms ticks

interface NodeState {
  x: number;
  y: number;
  vx: number;
  vy: number;
}

const nodes = new Map<string, NodeState>();
const pinned = new Set<string>();
let edges: PhysicsEdge[] = [];
let enabled = true;
let sleeping = false;
let timer: ReturnType<typeof setInterval> | null = null;
let ticksSinceHeat = 0;
// Flips true the first time the simulation reaches sleep (energy or wallclock).
// After that, automatic re-wakes from setGraph topology changes are suppressed —
// new nodes from background workers (extraction, dreamer) shouldn't kick the
// whole layout into motion. Only explicit user intent (drag, manual heat,
// pinning) re-wakes the loop, and even then only for ~5s before it freezes
// again.
let firstSettleDone = false;

function post(msg: PhysicsResponse) {
  (self as unknown as Worker).postMessage(msg);
}

function step() {
  if (!enabled) return;
  const ids = [...nodes.keys()];
  const n = ids.length;
  if (n === 0) return;
  ticksSinceHeat++;

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

  const positions: PhysicsPosition[] = [];
  let energy = 0;
  for (const id of ids) {
    const node = nodes.get(id)!;
    if (pinned.has(id)) {
      node.vx = 0;
      node.vy = 0;
      continue;
    }
    node.vx = (node.vx + fx.get(id)!) * DAMPING;
    node.vy = (node.vy + fy.get(id)!) * DAMPING;
    const speed = Math.sqrt(node.vx * node.vx + node.vy * node.vy);
    if (speed > MAX_VEL) {
      node.vx = (node.vx / speed) * MAX_VEL;
      node.vy = (node.vy / speed) * MAX_VEL;
    }
    energy += speed;
    node.x += node.vx;
    node.y += node.vy;
    positions.push({ id, x: node.x, y: node.y });
  }

  if (positions.length > 0) post({ type: "tick", positions });

  const energySleep = energy / n < ENERGY_SLEEP && pinned.size === 0;
  const wallclockSleep = ticksSinceHeat >= MAX_TICKS_PER_HEAT && pinned.size === 0;
  if (energySleep || wallclockSleep) {
    // Zero velocities so any residual drift halts cleanly. Heat resumes the loop.
    for (const node of nodes.values()) {
      node.vx = 0;
      node.vy = 0;
    }
    sleeping = true;
    firstSettleDone = true;
    stopLoop();
  }
}

function startLoop() {
  if (timer != null) return;
  sleeping = false;
  ticksSinceHeat = 0;
  timer = setInterval(step, TICK_INTERVAL_MS);
}

function stopLoop() {
  if (timer != null) {
    clearInterval(timer);
    timer = null;
  }
}

function setGraph(next: PhysicsNode[], nextEdges: PhysicsEdge[]) {
  const seen = new Set<string>();
  let topologyChanged = false;
  for (const n of next) {
    seen.add(n.id);
    const existing = nodes.get(n.id);
    if (existing) {
      if (n.pinned) {
        existing.x = n.x;
        existing.y = n.y;
      }
    } else {
      nodes.set(n.id, { x: n.x, y: n.y, vx: 0, vy: 0 });
      topologyChanged = true;
    }
    if (n.pinned) pinned.add(n.id);
    else pinned.delete(n.id);
  }
  for (const id of [...nodes.keys()]) {
    if (!seen.has(id)) {
      nodes.delete(id);
      pinned.delete(id);
      topologyChanged = true;
    }
  }
  if (edges.length !== nextEdges.length) topologyChanged = true;
  edges = nextEdges;
  // Only auto-wake on the FIRST graph load. Once the layout has settled
  // (`firstSettleDone`), subsequent topology changes from background workers
  // do NOT restart the simulation — new nodes sit at their seed position and
  // the existing layout stays frozen. User-driven `heat` / drag still wakes.
  if (enabled && topologyChanged && !firstSettleDone) {
    sleeping = false;
    startLoop();
  }
}

self.addEventListener("message", (ev: MessageEvent<PhysicsRequest>) => {
  const msg = ev.data;
  switch (msg.type) {
    case "setGraph":
      setGraph(msg.nodes, msg.edges);
      break;
    case "setPinned":
      pinned.clear();
      for (const id of msg.pinned) pinned.add(id);
      if (enabled && sleeping) {
        sleeping = false;
        startLoop();
      }
      break;
    case "updatePosition": {
      const node = nodes.get(msg.id);
      if (node) {
        node.x = msg.x;
        node.y = msg.y;
        node.vx = 0;
        node.vy = 0;
      }
      if (enabled && sleeping) {
        sleeping = false;
        startLoop();
      }
      break;
    }
    case "heat":
      if (enabled && sleeping) {
        sleeping = false;
        startLoop();
      }
      break;
    case "setEnabled":
      enabled = msg.enabled;
      if (enabled) {
        sleeping = false;
        startLoop();
      } else {
        stopLoop();
      }
      break;
  }
});
