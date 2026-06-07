/**
 * Pure force-directed simulation core — no worker, no timers, so it can be
 * unit-tested and benchmarked via tsx (#128).
 *
 * Hot-path layout: node state lives in flat Float64Array buffers indexed by
 * contiguous integers (id → index packed once per setGraph), not
 * Map<string, number> — string hashing in the force loop dominated the old
 * profile. Repulsion is exact all-pairs below BH_THRESHOLD_NODES and
 * Barnes-Hut (θ = 0.5) above it, taking O(n²) → O(n log n).
 */

export const REPULSION = 120000;
export const SPRING_LENGTH = 700;
export const SPRING_K = 0.01;
export const CENTER_PULL = 0.00001;
export const DAMPING = 0.78;
export const MAX_VEL = 35;
export const ENERGY_SLEEP = 0.05;

/**
 * Below this node count exact all-pairs repulsion (already on typed arrays)
 * wins — quadtree build + traversal overhead only pays off above it.
 * Measured with physicsSim.bench.ts repulsion-only sweep (BLACKSTAR, Node
 * 22): exact wins through n=500 (497us vs 623us), even at n=700 (973us
 * both), BH wins from there (1000: 2.0ms vs 1.6ms; 2000: 7.9ms vs 3.7ms).
 * Crossover ≈600. The bigger #128 win was Map→typed-array force
 * accumulation: full-step at n=2000 dropped 142.8ms → 4.5ms total.
 */
export const BH_THRESHOLD_NODES = 600;
export const BH_THETA = 0.5;
const BH_THETA2 = BH_THETA * BH_THETA;

export interface SimNodeInput {
  id: string;
  x: number;
  y: number;
}

export interface SimEdgeInput {
  from: string;
  to: string;
  confidence: number | null;
}

export interface SimPosition {
  id: string;
  x: number;
  y: number;
}

/** Quadtree node pool, struct-of-arrays. children[4i..4i+3] = -1 → leaf. */
interface QuadTree {
  cx: Float64Array;
  cy: Float64Array;
  half: Float64Array;
  comX: Float64Array;
  comY: Float64Array;
  mass: Float64Array;
  children: Int32Array;
  pointIndex: Int32Array;
  count: number;
}

function makeTree(capacity: number): QuadTree {
  return {
    cx: new Float64Array(capacity),
    cy: new Float64Array(capacity),
    half: new Float64Array(capacity),
    comX: new Float64Array(capacity),
    comY: new Float64Array(capacity),
    mass: new Float64Array(capacity),
    children: new Int32Array(capacity * 4).fill(-1),
    pointIndex: new Int32Array(capacity).fill(-1),
    count: 0,
  };
}

export class PhysicsSim {
  private ids: string[] = [];
  private indexOf = new Map<string, number>();
  private pos = new Float64Array(0);
  private vel = new Float64Array(0);
  private force = new Float64Array(0);
  private edgeIdx = new Int32Array(0);
  private edgeConfidence = new Float64Array(0);
  private edgeCount = 0;
  private tree: QuadTree = makeTree(0);

  get nodeCount(): number {
    return this.ids.length;
  }

  /** Returns true when the topology changed (node added/removed or edge count). */
  setGraph(nodes: SimNodeInput[], edges: SimEdgeInput[]): boolean {
    let topologyChanged = false;
    const nextIds: string[] = [];
    const nextIndex = new Map<string, number>();
    for (const n of nodes) {
      if (nextIndex.has(n.id)) continue;
      nextIndex.set(n.id, nextIds.length);
      nextIds.push(n.id);
      if (!this.indexOf.has(n.id)) topologyChanged = true;
    }
    if (nextIds.length !== this.ids.length) topologyChanged = true;

    const nextPos = new Float64Array(nextIds.length * 2);
    const nextVel = new Float64Array(nextIds.length * 2);
    for (const n of nodes) {
      const i = nextIndex.get(n.id)!;
      const prev = this.indexOf.get(n.id);
      if (prev !== undefined) {
        nextPos[i * 2] = this.pos[prev * 2];
        nextPos[i * 2 + 1] = this.pos[prev * 2 + 1];
        nextVel[i * 2] = this.vel[prev * 2];
        nextVel[i * 2 + 1] = this.vel[prev * 2 + 1];
      } else {
        nextPos[i * 2] = n.x;
        nextPos[i * 2 + 1] = n.y;
      }
    }

    this.ids = nextIds;
    this.indexOf = nextIndex;
    this.pos = nextPos;
    this.vel = nextVel;
    this.force = new Float64Array(nextIds.length * 2);

    const eIdx = new Int32Array(edges.length * 2);
    const eConf = new Float64Array(edges.length);
    let m = 0;
    for (const e of edges) {
      const a = nextIndex.get(e.from);
      const b = nextIndex.get(e.to);
      if (a === undefined || b === undefined) continue;
      eIdx[m * 2] = a;
      eIdx[m * 2 + 1] = b;
      eConf[m] = e.confidence ?? 0.7;
      m++;
    }
    if (m !== this.edgeCount) topologyChanged = true;
    this.edgeIdx = eIdx;
    this.edgeConfidence = eConf;
    this.edgeCount = m;
    return topologyChanged;
  }

  setPosition(id: string, x: number, y: number): boolean {
    const i = this.indexOf.get(id);
    if (i === undefined) return false;
    this.pos[i * 2] = x;
    this.pos[i * 2 + 1] = y;
    this.vel[i * 2] = 0;
    this.vel[i * 2 + 1] = 0;
    return true;
  }

  zeroVelocities(): void {
    this.vel.fill(0);
  }

  /** One integration tick. Returns average speed per node (energy metric). */
  step(): number {
    const n = this.ids.length;
    if (n === 0) return 0;
    const { pos, vel, force } = this;
    force.fill(0);

    if (n >= BH_THRESHOLD_NODES) {
      this.repulsionBarnesHut();
    } else {
      this.repulsionExact();
    }

    for (let e = 0; e < this.edgeCount; e++) {
      const a = this.edgeIdx[e * 2];
      const b = this.edgeIdx[e * 2 + 1];
      const dx = pos[b * 2] - pos[a * 2];
      const dy = pos[b * 2 + 1] - pos[a * 2 + 1];
      const d = Math.sqrt(dx * dx + dy * dy) + 0.01;
      const delta = d - SPRING_LENGTH;
      const f = SPRING_K * delta * this.edgeConfidence[e];
      const nx = dx / d;
      const ny = dy / d;
      // Two-term spring identical to the original worker: a distance-scaled pull plus the plain Hooke term — applied as two separate adds in the
      // original's order so small-n trajectories stay bit-comparable.
      force[a * 2] += nx * f * d * 0.02;
      force[a * 2 + 1] += ny * f * d * 0.02;
      force[b * 2] -= nx * f * d * 0.02;
      force[b * 2 + 1] -= ny * f * d * 0.02;
      force[a * 2] += nx * f;
      force[a * 2 + 1] += ny * f;
      force[b * 2] -= nx * f;
      force[b * 2 + 1] -= ny * f;
    }

    for (let i = 0; i < n; i++) {
      force[i * 2] -= pos[i * 2] * CENTER_PULL;
      force[i * 2 + 1] -= pos[i * 2 + 1] * CENTER_PULL;
    }

    let energy = 0;
    for (let i = 0; i < n; i++) {
      let vx = (vel[i * 2] + force[i * 2]) * DAMPING;
      let vy = (vel[i * 2 + 1] + force[i * 2 + 1]) * DAMPING;
      const speed = Math.sqrt(vx * vx + vy * vy);
      if (speed > MAX_VEL) {
        vx = (vx / speed) * MAX_VEL;
        vy = (vy / speed) * MAX_VEL;
      }
      energy += Math.min(speed, MAX_VEL);
      vel[i * 2] = vx;
      vel[i * 2 + 1] = vy;
      pos[i * 2] += vx;
      pos[i * 2 + 1] += vy;
    }
    return energy / n;
  }

  snapshot(): SimPosition[] {
    const out: SimPosition[] = new Array(this.ids.length);
    for (let i = 0; i < this.ids.length; i++) {
      out[i] = { id: this.ids[i], x: this.pos[i * 2], y: this.pos[i * 2 + 1] };
    }
    return out;
  }

  private repulsionExact(): void {
    const n = this.ids.length;
    const { pos, force } = this;
    for (let i = 0; i < n; i++) {
      const ax = pos[i * 2];
      const ay = pos[i * 2 + 1];
      for (let j = i + 1; j < n; j++) {
        const dx = ax - pos[j * 2];
        const dy = ay - pos[j * 2 + 1];
        const d2 = dx * dx + dy * dy + 0.01;
        const d = Math.sqrt(d2);
        const f = REPULSION / d2;
        const nx = dx / d;
        const ny = dy / d;
        force[i * 2] += nx * f;
        force[i * 2 + 1] += ny * f;
        force[j * 2] -= nx * f;
        force[j * 2 + 1] -= ny * f;
      }
    }
  }

  private repulsionBarnesHut(): void {
    const n = this.ids.length;
    const { pos, force } = this;
    const tree = this.buildTree();

    for (let i = 0; i < n; i++) {
      const px = pos[i * 2];
      const py = pos[i * 2 + 1];
      let fx = 0;
      let fy = 0;
      // Iterative traversal with an explicit stack — recursion is measurably slower here and can hit depth limits on degenerate layouts.
      const stack = this.traversalStack;
      let sp = 0;
      stack[sp++] = 0;
      while (sp > 0) {
        const t = stack[--sp];
        const mass = tree.mass[t];
        if (mass === 0) continue;
        const leafPoint = tree.pointIndex[t];
        if (leafPoint === i && mass === 1) continue;
        const dx = px - tree.comX[t];
        const dy = py - tree.comY[t];
        const d2 = dx * dx + dy * dy + 0.01;
        const width = tree.half[t] * 2;
        const isLeaf = tree.children[t * 4] === -1;
        if (isLeaf || (width * width) / d2 < BH_THETA2) {
          if (leafPoint === i) {
            // Cell contains self plus others: subtract nothing — the self term is zero-distance and excluded by construction only for single-point leaves;
            // multi-point cells containing self are split further by the θ criterion (self is always inside the cell, making width²/d² large until the leaf).
            continue;
          }
          const d = Math.sqrt(d2);
          const f = (REPULSION * mass) / d2;
          fx += (dx / d) * f;
          fy += (dy / d) * f;
        } else {
          const base = t * 4;
          for (let c = 0; c < 4; c++) {
            const child = tree.children[base + c];
            if (child !== -1) stack[sp++] = child;
          }
        }
      }
      force[i * 2] += fx;
      force[i * 2 + 1] += fy;
    }
  }

  private traversalStack = new Int32Array(0);

  private buildTree(): QuadTree {
    const n = this.ids.length;
    const { pos } = this;
    let minX = Infinity;
    let minY = Infinity;
    let maxX = -Infinity;
    let maxY = -Infinity;
    for (let i = 0; i < n; i++) {
      const x = pos[i * 2];
      const y = pos[i * 2 + 1];
      if (x < minX) minX = x;
      if (x > maxX) maxX = x;
      if (y < minY) minY = y;
      if (y > maxY) maxY = y;
    }
    const half = Math.max(maxX - minX, maxY - minY, 1) / 2 + 1;
    const rootX = (minX + maxX) / 2;
    const rootY = (minY + maxY) / 2;

    // Worst case ≈ 4n internal + n leaves; pool generously to avoid regrow.
    const capacity = Math.max(16, n * 8);
    if (this.tree.cx.length < capacity) {
      this.tree = makeTree(capacity);
    } else {
      this.tree.children.fill(-1, 0, capacity * 4);
      this.tree.pointIndex.fill(-1, 0, capacity);
      this.tree.mass.fill(0, 0, capacity);
    }
    if (this.traversalStack.length < capacity) {
      this.traversalStack = new Int32Array(capacity);
    }
    const tree = this.tree;
    tree.count = 1;
    tree.cx[0] = rootX;
    tree.cy[0] = rootY;
    tree.half[0] = half;
    tree.comX[0] = 0;
    tree.comY[0] = 0;
    tree.mass[0] = 0;
    tree.pointIndex[0] = -1;

    for (let i = 0; i < n; i++) {
      this.insert(tree, 0, i, pos[i * 2], pos[i * 2 + 1]);
    }
    this.computeMass(tree, 0);
    return tree;
  }

  private insert(tree: QuadTree, t: number, point: number, x: number, y: number): void {
    for (;;) {
      if (tree.children[t * 4] !== -1) {
        t = this.childFor(tree, t, x, y);
        continue;
      }
      if (tree.pointIndex[t] === -1 && tree.mass[t] === 0) {
        tree.pointIndex[t] = point;
        tree.mass[t] = 1;
        tree.comX[t] = x;
        tree.comY[t] = y;
        return;
      }
      // Occupied leaf → split. Coincident points would split forever; merge them into one heavier leaf instead (repulsion between exact coincident
      // points is undefined anyway — epsilon handles it).
      const existing = tree.pointIndex[t];
      const ex = tree.comX[t];
      const ey = tree.comY[t];
      if (Math.abs(ex - x) < 1e-9 && Math.abs(ey - y) < 1e-9) {
        tree.mass[t] += 1;
        return;
      }
      this.split(tree, t);
      const back = this.childFor(tree, t, ex, ey);
      tree.pointIndex[back] = existing;
      tree.mass[back] = tree.mass[t];
      tree.comX[back] = ex;
      tree.comY[back] = ey;
      tree.pointIndex[t] = -1;
      tree.mass[t] = 0;
      t = this.childFor(tree, t, x, y);
    }
  }

  private split(tree: QuadTree, t: number): void {
    const h = tree.half[t] / 2;
    const cx = tree.cx[t];
    const cy = tree.cy[t];
    for (let c = 0; c < 4; c++) {
      const idx = tree.count++;
      tree.children[t * 4 + c] = idx;
      tree.cx[idx] = cx + (c & 1 ? h : -h);
      tree.cy[idx] = cy + (c & 2 ? h : -h);
      tree.half[idx] = h;
      tree.comX[idx] = 0;
      tree.comY[idx] = 0;
      tree.mass[idx] = 0;
      tree.pointIndex[idx] = -1;
      tree.children[idx * 4] = -1;
      tree.children[idx * 4 + 1] = -1;
      tree.children[idx * 4 + 2] = -1;
      tree.children[idx * 4 + 3] = -1;
    }
  }

  private childFor(tree: QuadTree, t: number, x: number, y: number): number {
    const right = x >= tree.cx[t] ? 1 : 0;
    const down = y >= tree.cy[t] ? 2 : 0;
    return tree.children[t * 4 + right + down];
  }

  private computeMass(tree: QuadTree, t: number): void {
    if (tree.children[t * 4] === -1) return;
    let mass = 0;
    let comX = 0;
    let comY = 0;
    for (let c = 0; c < 4; c++) {
      const child = tree.children[t * 4 + c];
      this.computeMass(tree, child);
      const m = tree.mass[child];
      if (m > 0) {
        mass += m;
        comX += tree.comX[child] * m;
        comY += tree.comY[child] * m;
      }
    }
    tree.mass[t] = mass;
    if (mass > 0) {
      tree.comX[t] = comX / mass;
      tree.comY[t] = comY / mass;
    }
  }
}
