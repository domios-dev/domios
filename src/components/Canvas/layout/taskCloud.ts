import type { Plan, Task, TaskStatus } from "../types";
import type { CanvasLayoutSettings } from "../settings";
import { taskFootprint } from "./taskFootprint";
import type { Point } from "./geometry";
import type { LaidOutEdge, LaidOutNode } from "./types";

// worlds-v3 §2: golden-angle phyllotaxis pack. The irrational that gives maximal
// angular spacing (137.5°) so successive tiles never align into spokes.
export const GOLDEN_ANGLE = Math.PI * (3 - Math.sqrt(5));

// §2.2 inter-tile gap (g) folded into each tile's enclosing radius. Kept tight so
// the disk reads as a dense, FILLED cloud rather than a sparse ring.
export const TILE_GAP = 5;

// Subtask satellites: children pack as a SMALLER nested cloud swarming around the
// parent (parent = slot 0 at the local center, children orbit it). The whole
// parent+satellites cluster then packs as one circle among sibling clusters, so a
// task reads as a main tile haloed by its subtasks at every zoom — all solid.
export const SUBTASK_SCALE = 0.5; // satellite footprint vs. its own card footprint
export const SUBTASK_MIN_W = 70;
export const SUBTASK_MIN_H = 46;
export const SUBTASK_GAP = 4; // intra-cluster gap (tighter than the outer disk)

// §2.3 Alt C: innermost K slots reserved for in_progress tasks (≈1–2 rings). A
// fixed K is what makes the history-spiral index status-independent — going
// active/done ripples ≤K tiles, never the whole disk.
export const ACTIVE_CORE_K = 9;

function priorityRank(p: Task["priority"]): number {
  switch (p) {
    case "p0": return 0;
    case "p1": return 1;
    case "p3": return 3;
    default: return 2; // p2 / undefined
  }
}

// Stable creation rank over the given tasks by (created_at, id). A newly created
// task sorts last → outermost slot → existing tiles keep their indices (§31).
function creationRanks(tasks: Task[]): Map<string, number> {
  const sorted = [...tasks].sort(
    (a, b) => (a.created_at ?? 0) - (b.created_at ?? 0) || a.id.localeCompare(b.id),
  );
  const m = new Map<string, number>();
  sorted.forEach((t, i) => m.set(t.id, i));
  return m;
}

// §35: done/cancelled tasks SINK away (right/outer); everything else is "active"
// and pulls toward the plan side. in_progress > idle, then priority/recency.
function isSunk(status: TaskStatus): boolean {
  return status === "done" || status === "cancelled";
}

// Active-core rank among NOT-done tasks (in_progress innermost, then idle). idle =
// pending/blocked so they too occupy the plan-side core, not the history spiral.
function activeRanks(tasks: Task[]): Map<string, number> {
  const statusRank = (s: TaskStatus) => (s === "in_progress" ? 0 : 1);
  const active = tasks
    .filter((t) => !isSunk(t.status))
    .sort(
      (a, b) =>
        statusRank(a.status) - statusRank(b.status) ||
        priorityRank(a.priority) - priorityRank(b.priority) ||
        (b.status_changed_at ?? 0) - (a.status_changed_at ?? 0) ||
        (a.created_at ?? 0) - (b.created_at ?? 0) ||
        a.id.localeCompare(b.id),
    );
  const m = new Map<string, number>();
  active.forEach((t, i) => m.set(t.id, i));
  return m;
}

// Right-opening cloud: all tiles on +x so the cloud's left side is a flat edge
// packed flush to the plan; active core (small radius) sits nearest that edge.
function foldedTheta(slot: number): number {
  const base = slot * GOLDEN_ANGLE;
  return Math.atan2(Math.sin(base), Math.abs(Math.cos(base)));
}

// §2.3 two-zone sunflower index: active core (rank < K) at the eye, everything
// else on the history spiral at K + creationRank (independent of active count).
function slotIndex(task: Task, active: Map<string, number>, creation: Map<string, number>, k: number): number {
  const ar = active.get(task.id);
  if (ar != null && ar < k) return ar;
  return k + (creation.get(task.id) ?? 0);
}

export interface CloudItem {
  id: string;
  w: number;
  h: number;
  slot: number;
  theta?: number; // placement direction; default slot * GOLDEN_ANGLE (centered sunflower)
}

export interface PackedItem {
  id: string;
  cx: number;
  cy: number;
  w: number;
  h: number;
}

export interface CloudPack {
  items: PackedItem[];
  radius: number;
}

// §2.2 area-accumulated golden-angle pack with outward-only collision resolve.
// Tiles are placed in slot order; each only ever pushes ITSELF outward against
// already-placed tiles → deterministic, append-stable, non-overlapping. Returns
// centers relative to (0,0) and the disk radius R = max(r + ρ).
//
// CENTER-FILL: the seed radius uses the area of the tiles placed BEFORE this one,
// so tile 0 lands at the origin (r=0) and the disk fills from the middle outward.
// (The old `cumArea + area/2` seeded even tile 0 at ≈0.7ρ → a hollow center ring.)
type Placed = { cx: number; cy: number; rho: number };

// Uniform spatial-hash grid over placed tiles: a tile only ever overlaps another
// whose centers are within ρ_i+ρ_j ≤ 2·maxρ, so collision checks query a fixed
// neighborhood of cells instead of ALL placed tiles. Same placement output as the
// brute-force O(N²) loop, but ~O(N) — the layout no longer blows up at scale.
class PackGrid {
  private cells = new Map<number, Placed[]>();
  constructor(private cell: number) {}
  private key(gx: number, gy: number): number {
    return (gx + 16384) * 32768 + (gy + 16384);
  }
  insert(p: Placed): void {
    const k = this.key(Math.floor(p.cx / this.cell), Math.floor(p.cy / this.cell));
    const a = this.cells.get(k);
    if (a) a.push(p);
    else this.cells.set(k, [p]);
  }
  deepestPenetration(cx: number, cy: number, rho: number): number {
    const gx = Math.floor(cx / this.cell);
    const gy = Math.floor(cy / this.cell);
    let pen = 0;
    for (let dx = -2; dx <= 2; dx++) {
      for (let dy = -2; dy <= 2; dy++) {
        const a = this.cells.get(this.key(gx + dx, gy + dy));
        if (!a) continue;
        for (const p of a) {
          const d = Math.hypot(cx - p.cx, cy - p.cy);
          const minD = rho + p.rho;
          if (d < minD - 1e-6) pen = Math.max(pen, minD - d);
        }
      }
    }
    return pen;
  }
}

export function packCloud(items: CloudItem[], gap = TILE_GAP): CloudPack {
  const ordered = [...items].sort((a, b) => a.slot - b.slot || a.id.localeCompare(b.id));
  const rhoOf = (w: number, h: number) => 0.5 * Math.hypot(w, h) + gap; // ρ = ½√(w²+h²) + g
  let maxRho = 1;
  for (const it of ordered) maxRho = Math.max(maxRho, rhoOf(it.w, it.h));
  const grid = new PackGrid(maxRho * 2);
  const result: PackedItem[] = [];
  let cumArea = 0;
  let radius = 0;

  for (const it of ordered) {
    const rho = rhoOf(it.w, it.h);
    const area = Math.PI * rho * rho; // a = π·ρ²
    let r = Math.sqrt(cumArea / Math.PI); // center-out areal-density radius
    const theta = it.theta ?? it.slot * GOLDEN_ANGLE;
    let cx = r * Math.cos(theta);
    let cy = r * Math.sin(theta);

    // Outward-only: bump r by the deepest penetration until no circle overlaps.
    for (let guard = 0; guard < 4096; guard++) {
      const pen = grid.deepestPenetration(cx, cy, rho);
      if (pen <= 1e-6) break;
      r += pen;
      cx = r * Math.cos(theta);
      cy = r * Math.sin(theta);
    }

    grid.insert({ cx, cy, rho });
    result.push({ id: it.id, cx, cy, w: it.w, h: it.h });
    cumArea += area;
    radius = Math.max(radius, r + rho);
  }

  return { items: result, radius };
}

export interface TaskCloudLayout {
  nodes: LaidOutNode[];
  edges: LaidOutEdge[];
  extraPoints: Point[];
  radius: number;
}

// A local placement = one tile's center OFFSET from its cluster center (0,0 = the
// parent). The whole cluster translates together when the outer pack places it.
interface LocalPlace {
  id: string;
  task: Task;
  cx: number;
  cy: number;
  w: number;
  h: number;
}

interface Cluster {
  task: Task;
  slot: number;
  radius: number; // enclosing radius of parent + satellites (0 → childless, tile-only)
  locals: LocalPlace[];
}

// Stable child order (created_at, id) → satellite slot, so a new subtask lands on
// the outermost satellite ring and existing satellites keep their orbit.
function childOrder(children: Task[]): Task[] {
  return [...children].sort((a, b) => (a.created_at ?? 0) - (b.created_at ?? 0) || a.id.localeCompare(b.id));
}

// worlds-v3 §2: circle-packed task disk with NESTED satellite sub-clouds. Each
// top-level task is a MAIN tile; its subtasks are SMALLER tiles packed by the same
// phyllotaxis logic, swarming around the parent (parent = local slot 0 at the
// center, children orbit). The parent+satellites form one cluster that packs as a
// single circle among sibling clusters — so from afar you SEE each main task haloed
// by its solid subtasks, and zooming in reveals detail. No transparency, no stacked
// super-cell, no hollow gap. Node positions are relative to the disk center (0,0);
// the caller offsets by diskCenter.
export function layoutTaskCloud(
  plan: Plan,
  s: CanvasLayoutSettings,
  anchors: Map<string, Point>,
): TaskCloudLayout {
  const nodes: LaidOutNode[] = [];
  const edges: LaidOutEdge[] = [];
  const extraPoints: Point[] = [];

  const taskIds = new Set(plan.tasks.map((t) => t.id));
  const childrenByParent = new Map<string, Task[]>();
  for (const t of plan.tasks) {
    const pid = t.parent_task_id;
    if (pid && taskIds.has(pid)) {
      const arr = childrenByParent.get(pid);
      if (arr) arr.push(t);
      else childrenByParent.set(pid, [t]);
    }
  }
  const topLevel = plan.tasks.filter((t) => !t.parent_task_id || !taskIds.has(t.parent_task_id));

  const active = activeRanks(topLevel);
  const creation = creationRanks(topLevel);

  const clusters: Cluster[] = topLevel.map((task) => {
    const children = childOrder(childrenByParent.get(task.id) ?? []);
    const fp = taskFootprint(task, children, s);
    const slot = slotIndex(task, active, creation, ACTIVE_CORE_K);
    if (!children.length) {
      return { task, slot, radius: 0, locals: [{ id: task.id, task, cx: 0, cy: 0, w: fp.width, h: fp.height }] };
    }
    // Local sub-cloud: parent (slot 0, full footprint) + satellites (slots 1..n,
    // halved footprint). packCloud center-fills → parent at origin, children swarm.
    const childById = new Map(children.map((c) => [c.id, c]));
    const localItems: CloudItem[] = [{ id: task.id, w: fp.width, h: fp.height, slot: 0 }];
    children.forEach((c, i) => {
      const cfp = taskFootprint(c, [], s);
      const w = Math.max(SUBTASK_MIN_W, Math.round(cfp.width * SUBTASK_SCALE));
      const h = Math.max(SUBTASK_MIN_H, Math.round(cfp.height * SUBTASK_SCALE));
      localItems.push({ id: c.id, w, h, slot: i + 1 });
    });
    const localPack = packCloud(localItems, SUBTASK_GAP);
    const locals: LocalPlace[] = localPack.items.map((it) => ({
      id: it.id,
      task: it.id === task.id ? task : childById.get(it.id)!,
      cx: it.cx,
      cy: it.cy,
      w: it.w,
      h: it.h,
    }));
    return { task, slot, radius: localPack.radius, locals };
  });

  // Outer pack: a childless task packs by its own footprint; a cluster packs as a
  // square whose circumscribed circle == the cluster radius (½√(2·side²) = side/√2,
  // so side = R·√2 → enclosing radius R). Cluster circles never overlap → no two
  // satellites from different parents collide.
  const outerItems: CloudItem[] = clusters.map((cl) => {
    const theta = foldedTheta(cl.slot);
    if (cl.radius === 0) {
      const p = cl.locals[0];
      return { id: cl.task.id, w: p.w, h: p.h, slot: cl.slot, theta };
    }
    const side = cl.radius * Math.SQRT2;
    return { id: cl.task.id, w: side, h: side, slot: cl.slot, theta };
  });
  const pack = packCloud(outerItems, TILE_GAP);
  const byId = new Map(clusters.map((c) => [c.task.id, c]));

  for (const p of pack.items) {
    const cl = byId.get(p.id)!;
    for (const loc of cl.locals) {
      const cx = p.cx + loc.cx;
      const cy = p.cy + loc.cy;
      nodes.push({ id: loc.id, kind: "task", planId: plan.id, box: { x: cx - loc.w / 2, y: cy - loc.h / 2, width: loc.w, height: loc.h }, iteration: null, manual: false });
    }
    for (const sid of cl.task.section_ids) {
      const anchor = anchors.get(sid);
      if (anchor) edges.push({ id: `${sid}~${cl.task.id}`, kind: "section-task", fromPoint: anchor, to: cl.task.id });
    }
  }

  return { nodes, edges, extraPoints, radius: pack.radius };
}
