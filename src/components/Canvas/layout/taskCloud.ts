import type { Plan, Task, TaskStatus } from "../types";
import type { CanvasLayoutSettings } from "../settings";
import { flatSections } from "../sectionRows";
import { zoneLabelBox } from "./sectionZones";
import { taskFootprint } from "./taskFootprint";
import type { Box, Point } from "./geometry";
import type { LaidOutEdge, LaidOutNode } from "./types";

// Tasks with no section pack into one neutral, unlabeled group.
export const UNSECTIONED = "__unsectioned__";

// enough that adjacent blobs nestle into one cohesive cloud, not scattered islands.
export const SECTION_GROUP_GAP = 14;

// angular spacing (137.5°) so successive tiles never align into spokes.
export const GOLDEN_ANGLE = Math.PI * (3 - Math.sqrt(5));

// the disk reads as a dense, FILLED cloud rather than a sparse ring.
export const TILE_GAP = 5;

export const SUBTASK_SCALE = 0.66; // satellite footprint vs. its own card footprint
export const SUBTASK_MIN_W = 140;
export const SUBTASK_MIN_H = 84;
export const SUBTASK_GAP = 4; // intra-cluster gap (tighter than the outer disk)

export const ACTIVE_CORE_K = 9;

function priorityRank(p: Task["priority"]): number {
  switch (p) {
    case "p0": return 0;
    case "p1": return 1;
    case "p3": return 3;
    default: return 2; // p2 / undefined
  }
}

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

// its active core (small radius) sits nearest the plan — §35 active-left within a blob.
function foldedTheta(slot: number): number {
  const base = slot * GOLDEN_ANGLE;
  return Math.atan2(Math.sin(base), Math.abs(Math.cos(base)));
}

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

type Placed = { cx: number; cy: number; rho: number };

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

export function packCloud(items: CloudItem[], gap = TILE_GAP, tight = false): CloudPack {
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
    let r = tight ? 0 : Math.sqrt(cumArea / Math.PI); // tight → tangent; else center-out
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

// the reserved label-tile centre (cloud-local) where the region name pill sits.
export interface TaskCloudGroup {
  sectionId: string;
  boxes: Box[];
  labelAnchor?: Point;
}

function blobMetrics(pack: CloudPack): { cx: number; cy: number; r: number } {
  if (!pack.items.length) return { cx: 0, cy: 0, r: 1 };
  let sx = 0;
  let sy = 0;
  for (const it of pack.items) { sx += it.cx; sy += it.cy; }
  const cx = sx / pack.items.length;
  const cy = sy / pack.items.length;
  let r = 1;
  for (const it of pack.items) r = Math.max(r, Math.hypot(it.cx - cx, it.cy - cy) + 0.5 * Math.hypot(it.w, it.h));
  return { cx, cy, r };
}

export interface TaskCloudLayout {
  nodes: LaidOutNode[];
  edges: LaidOutEdge[];
  extraPoints: Point[];
  radius: number;
  groups: TaskCloudGroup[];
}

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

// the outermost satellite ring and existing satellites keep their orbit.
function childOrder(children: Task[]): Task[] {
  return [...children].sort((a, b) => (a.created_at ?? 0) - (b.created_at ?? 0) || a.id.localeCompare(b.id));
}

// → a single tile (radius 0). Geometry only; the caller assigns the cluster's slot.
function buildCluster(task: Task, children: Task[], slot: number, s: CanvasLayoutSettings): Cluster {
  const fp = taskFootprint(task, children, s);
  if (!children.length) {
    return { task, slot, radius: 0, locals: [{ id: task.id, task, cx: 0, cy: 0, w: fp.width, h: fp.height }] };
  }
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
}

// section ids not in the plan (sorted), then the neutral UNSECTIONED group last.
function orderGroups(keys: string[], plan: Plan): string[] {
  const present = new Set(keys);
  const flatOrder = flatSections(plan).map((sec) => sec.id).filter((id) => present.has(id));
  const seen = new Set(flatOrder);
  const extras = keys.filter((k) => k !== UNSECTIONED && !seen.has(k)).sort();
  const tail = present.has(UNSECTIONED) ? [UNSECTIONED] : [];
  return [...flatOrder, ...extras, ...tail];
}

export function layoutTaskCloud(
  plan: Plan,
  s: CanvasLayoutSettings,
  anchors: Map<string, Point>,
  sectionTitles?: ReadonlyMap<string, string>,
): TaskCloudLayout {
  const nodes: LaidOutNode[] = [];
  const edges: LaidOutEdge[] = [];
  const extraPoints: Point[] = [];

  const planTitles = new Map(flatSections(plan).map((sec) => [sec.id, sec.title]));
  const titleOf = (sid: string): string | null => {
    const t = (sectionTitles?.get(sid) ?? planTitles.get(sid))?.trim();
    return t || null;
  };

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

  const groupOf = (t: Task) => t.section_ids[0] ?? UNSECTIONED;
  const byGroup = new Map<string, Task[]>();
  for (const t of topLevel) {
    const g = byGroup.get(groupOf(t));
    if (g) g.push(t);
    else byGroup.set(groupOf(t), [t]);
  }
  const groupOrder = orderGroups([...byGroup.keys()], plan);
  const labelId = (sid: string) => `__zlabel__${sid}`;

  const blobs = groupOrder.map((sectionId) => {
    const tasks = byGroup.get(sectionId)!;
    const active = activeRanks(tasks);
    const creation = creationRanks(tasks);
    const clusters = tasks.map((task) =>
      buildCluster(task, childOrder(childrenByParent.get(task.id) ?? []), slotIndex(task, active, creation, ACTIVE_CORE_K), s),
    );
    const items: CloudItem[] = clusters.map((cl) => {
      const theta = foldedTheta(cl.slot);
      if (cl.radius === 0) {
        const p = cl.locals[0];
        return { id: cl.task.id, w: p.w, h: p.h, slot: cl.slot, theta };
      }
      const side = cl.radius * Math.SQRT2;
      return { id: cl.task.id, w: side, h: side, slot: cl.slot, theta };
    });
    // cards pack around the real pill — no truncation, no clip, no overlap.
    const title = sectionId === UNSECTIONED ? null : titleOf(sectionId);
    if (title) {
      const lb = zoneLabelBox(title);
      items.push({ id: labelId(sectionId), w: lb.width, h: lb.height, slot: Math.max(1, Math.floor(clusters.length / 3)), theta: 0 });
    }
    return { sectionId, clusters, pack: packCloud(items, TILE_GAP) };
  });

  const blobCenters = new Map<string, Point>();
  if (blobs.length === 1) blobCenters.set(blobs[0].sectionId, { x: 0, y: 0 });
  else if (blobs.length > 1) {
    const metricsById = new Map(blobs.map((b) => [b.sectionId, blobMetrics(b.pack)]));
    const blobItems: CloudItem[] = blobs.map((blob, slot) => {
      const m = metricsById.get(blob.sectionId)!;
      const side = Math.max(m.r * Math.SQRT2, 2);
      return { id: blob.sectionId, w: side, h: side, slot, theta: foldedTheta(slot) };
    });
    const blobPack = packCloud(blobItems, SECTION_GROUP_GAP);
    for (const it of blobPack.items) {
      const m = metricsById.get(it.id)!;
      blobCenters.set(it.id, { x: it.cx - m.cx, y: it.cy - m.cy });
    }
  }

  const boxesBySection = new Map<string, Box[]>();
  const labelAnchors = new Map<string, Point>();
  let radius = 0;
  for (const blob of blobs) {
    const gc = blobCenters.get(blob.sectionId) ?? { x: 0, y: 0 };
    const byId = new Map(blob.pack.items.map((it) => [it.id, it]));
    const lab = byId.get(labelId(blob.sectionId));
    if (lab) labelAnchors.set(blob.sectionId, { x: gc.x + lab.cx, y: gc.y + lab.cy });
    const boxes: Box[] = [];
    for (const cl of blob.clusters) {
      const cp = byId.get(cl.task.id)!;
      for (const loc of cl.locals) {
        const cx = gc.x + cp.cx + loc.cx;
        const cy = gc.y + cp.cy + loc.cy;
        const box: Box = { x: cx - loc.w / 2, y: cy - loc.h / 2, width: loc.w, height: loc.h };
        nodes.push({ id: loc.id, kind: "task", planId: plan.id, box, iteration: null, manual: false });
        boxes.push(box);
        radius = Math.max(radius, Math.hypot(cx, cy) + Math.hypot(loc.w, loc.h) / 2);
      }
      for (const linkSid of cl.task.section_ids) {
        const anchor = anchors.get(linkSid);
        if (anchor) edges.push({ id: `${linkSid}~${cl.task.id}`, kind: "section-task", fromPoint: anchor, to: cl.task.id });
      }
    }
    boxesBySection.set(blob.sectionId, boxes);
  }
  const groups: TaskCloudGroup[] = groupOrder
    .filter((sid) => (boxesBySection.get(sid)?.length ?? 0) > 0)
    .map((sid) => ({ sectionId: sid, boxes: boxesBySection.get(sid)!, labelAnchor: labelAnchors.get(sid) }));

  return { nodes, edges, extraPoints, radius, groups };
}
