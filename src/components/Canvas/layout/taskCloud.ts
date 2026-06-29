import type { Plan, Task, TaskStatus } from "../types";
import type { CanvasLayoutSettings } from "../settings";
import { flatSections } from "../sectionRows";
import { zoneLabelBox, convexHull } from "./sectionZones";
import { taskFootprint } from "./taskFootprint";
import type { Box, Point } from "./geometry";
import type { LaidOutEdge, LaidOutNode } from "./types";

// Tasks with no section pack into one neutral, unlabeled group.
export const UNSECTIONED = "__unsectioned__";

// Min breathing space between cards of DIFFERENT sections after compaction — tight
// enough that adjacent blobs nestle into one cohesive cloud, not scattered islands.
export const SECTION_GROUP_GAP = 14;

// Gravity-compaction: after a safe circle placement, pull each blob toward the cloud
// centroid until its real card hull is about to touch a neighbour's. This nestles the
// actual masses (corners included) instead of leaving the air a circle pack wastes.
const COMPACT_ITERS = 28;

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
// Subtasks are satellites — smaller than their parent, but UX-3: still big enough to
// READ (a 2-line title + the glyph row). The floor fits that schematic face; the
// scale keeps them clearly subordinate to a parent's volume-driven mass.
export const SUBTASK_SCALE = 0.66; // satellite footprint vs. its own card footprint
export const SUBTASK_MIN_W = 140;
export const SUBTASK_MIN_H = 84;
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

// Right-opening fold: all tiles on +x so a blob's plan-facing side is a flat edge and
// its active core (small radius) sits nearest the plan — §35 active-left within a blob.
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

// `tight`: seed every item at the origin so the outward collision-resolve packs it
// flush against its neighbours (tangent), independent of size disparity. The default
// areal seed assumes uniform tiles — with one dominant circle it over-seeds the rest,
// flinging small members far out. Tiles use the default; section blobs use `tight`.
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

// Per section: the member task boxes (cloud-local coords) the zones layer hugs, plus
// the reserved label-tile centre (cloud-local) where the region name pill sits.
export interface TaskCloudGroup {
  sectionId: string;
  boxes: Box[];
  labelAnchor?: Point;
}

// Greedy tight circle pack: largest first at the origin; each next circle placed
// tangent to a placed one at the candidate angle NEAREST the origin that overlaps
// nothing → a compact, mutually-tangent cluster. Deterministic, O(n²·angles), small n.
function packTangentCompact(circles: { id: string; r: number }[], gap: number): Map<string, Point> {
  const ordered = [...circles].sort((a, b) => b.r - a.r || a.id.localeCompare(b.id));
  const placed: { x: number; y: number; r: number }[] = [];
  const out = new Map<string, Point>();
  const ANG = 64;
  for (const c of ordered) {
    if (!placed.length) {
      placed.push({ x: 0, y: 0, r: c.r });
      out.set(c.id, { x: 0, y: 0 });
      continue;
    }
    let best: { x: number; y: number; d: number } | null = null;
    for (const p of placed) {
      const ring = p.r + c.r + gap;
      for (let k = 0; k < ANG; k++) {
        const a = (k / ANG) * Math.PI * 2;
        const x = p.x + Math.cos(a) * ring;
        const y = p.y + Math.sin(a) * ring;
        let okPos = true;
        for (const q of placed) {
          if (Math.hypot(x - q.x, y - q.y) < q.r + c.r + gap - 1e-6) { okPos = false; break; }
        }
        if (!okPos) continue;
        const d = Math.hypot(x, y);
        if (!best || d < best.d) best = { x, y, d };
      }
    }
    const pos = best ?? { x: 0, y: 0 };
    placed.push({ x: pos.x, y: pos.y, r: c.r });
    out.set(c.id, { x: pos.x, y: pos.y });
  }
  return out;
}

// A blob's MASS center + the radius of its content measured from THAT center. The
// blob-local origin (0,0) is the plan-side tip, not the mass center (right-opening
// fold), so packing on the origin-extent flung dense cores apart. Packing on the
// centroid pulls the actual masses tangent — one cohesive cloud.
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

// Convex SAT: do two convex polygons overlap? Used to keep blobs from nestling so far
// that their (gap-padded) card hulls touch. Returns true on any overlap.
function polysOverlap(a: Point[], b: Point[]): boolean {
  for (const poly of [a, b]) {
    for (let i = 0; i < poly.length; i++) {
      const p = poly[i];
      const q = poly[(i + 1) % poly.length];
      const ax = -(q.y - p.y);
      const ay = q.x - p.x;
      let aMin = Infinity, aMax = -Infinity, bMin = Infinity, bMax = -Infinity;
      for (const v of a) { const d = v.x * ax + v.y * ay; aMin = Math.min(aMin, d); aMax = Math.max(aMax, d); }
      for (const v of b) { const d = v.x * ax + v.y * ay; bMin = Math.min(bMin, d); bMax = Math.max(bMax, d); }
      if (aMax < bMin || bMax < aMin) return false; // separating axis found
    }
  }
  return true;
}

function translatePoly(poly: Point[], dx: number, dy: number): Point[] {
  return poly.map((p) => ({ x: p.x + dx, y: p.y + dy }));
}

export interface TaskCloudLayout {
  nodes: LaidOutNode[];
  edges: LaidOutEdge[];
  extraPoints: Point[];
  radius: number;
  groups: TaskCloudGroup[];
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

// One cluster = a parent tile + its subtask satellites (nested sub-cloud). Childless
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

// Deterministic group order: sections in the plan's flat order first, then any
// section ids not in the plan (sorted), then the neutral UNSECTIONED group last.
function orderGroups(keys: string[], plan: Plan): string[] {
  const present = new Set(keys);
  const flatOrder = flatSections(plan).map((sec) => sec.id).filter((id) => present.has(id));
  const seen = new Set(flatOrder);
  const extras = keys.filter((k) => k !== UNSECTIONED && !seen.has(k)).sort();
  const tail = present.has(UNSECTIONED) ? [UNSECTIONED] : [];
  return [...flatOrder, ...extras, ...tail];
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
  sectionTitles?: ReadonlyMap<string, string>,
): TaskCloudLayout {
  const nodes: LaidOutNode[] = [];
  const edges: LaidOutEdge[] = [];
  const extraPoints: Point[] = [];

  // Section title for label-tile sizing: the layout plan's sections may be spec-overlaid
  // (ids won't match tasks), so prefer the passed map (from the original plans), then
  // fall back to this plan's own sections (the test/fixture path).
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

  // COHESIVE SECTION BLOBS: each section's tasks pack as their OWN tight right-opening
  // sub-cloud (active core flush to the plan-side tip → §35 holds within every blob),
  // with a reserved LABEL tile at the blob body so cards collision-resolve AROUND the
  // name (it never overlaps a card / never clips). The blobs are then nestled into one
  // cohesive mass — each a coherent shape the zone hull can visibly hug (a drawn
  // territory, not a thin fan smear). A no-section plan = one blob = the original pack.
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
    // Reserve a label tile sized to the FULL title (≤2 lines) in the blob body so
    // cards pack around the real pill — no truncation, no clip, no overlap.
    const title = sectionId === UNSECTIONED ? null : titleOf(sectionId);
    if (title) {
      const lb = zoneLabelBox(title);
      items.push({ id: labelId(sectionId), w: lb.width, h: lb.height, slot: Math.max(1, Math.floor(clusters.length / 3)), theta: 0 });
    }
    return { sectionId, clusters, pack: packCloud(items, TILE_GAP) };
  });

  // Nestle the blobs into ONE cohesive mass. A circle pack alone leaves the air in
  // every blob's corners → separated islands; instead we place blobs on safe centroid
  // circles, then GRAVITY-COMPACT: pull each toward the cloud centroid until its real
  // (gap-padded) card hull is about to touch a neighbour's. The masses interlock; cards
  // never collide. A single blob sits at the origin so a no-section plan reproduces the
  // original one-fan pack byte-for-byte (§35 + stability).
  const blobCenters = new Map<string, Point>();
  if (blobs.length === 1) blobCenters.set(blobs[0].sectionId, { x: 0, y: 0 });
  else if (blobs.length > 1) {
    const PAD = SECTION_GROUP_GAP / 2; // each blob's hull inflated by this → cards stay ≥SECTION_GROUP_GAP apart
    const metrics = new Map(blobs.map((b) => [b.sectionId, blobMetrics(b.pack)]));
    // Per-blob card hull, centred on the blob's centroid and inflated by PAD.
    const hullCentered = new Map<string, Point[]>();
    for (const blob of blobs) {
      const m = metrics.get(blob.sectionId)!;
      const byId = new Map(blob.pack.items.map((it) => [it.id, it]));
      const corners: Point[] = [];
      for (const cl of blob.clusters) {
        const cp = byId.get(cl.task.id)!;
        for (const loc of cl.locals) {
          const x0 = cp.cx + loc.cx - loc.w / 2 - PAD - m.cx;
          const y0 = cp.cy + loc.cy - loc.h / 2 - PAD - m.cy;
          const x1 = cp.cx + loc.cx + loc.w / 2 + PAD - m.cx;
          const y1 = cp.cy + loc.cy + loc.h / 2 + PAD - m.cy;
          corners.push({ x: x0, y: y0 }, { x: x1, y: y0 }, { x: x1, y: y1 }, { x: x0, y: y1 });
        }
      }
      hullCentered.set(blob.sectionId, convexHull(corners));
    }
    // Safe start: tangent circles enclosing each inflated hull (r ⊇ hull) — no overlap.
    const placed = packTangentCompact(
      blobs.map((b) => ({ id: b.sectionId, r: Math.max(1, metrics.get(b.sectionId)!.r + PAD) })),
      SECTION_GROUP_GAP,
    );
    const order = blobs.map((b) => b.sectionId);
    const P = new Map(order.map((id) => [id, placed.get(id) ?? { x: 0, y: 0 }]));
    const worldHull = (id: string) => translatePoly(hullCentered.get(id)!, P.get(id)!.x, P.get(id)!.y);
    for (let iter = 0; iter < COMPACT_ITERS; iter++) {
      let gx = 0, gy = 0;
      for (const id of order) { gx += P.get(id)!.x; gy += P.get(id)!.y; }
      gx /= order.length; gy /= order.length;
      for (const id of order) {
        const p = P.get(id)!;
        const dx = gx - p.x, dy = gy - p.y;
        const dist = Math.hypot(dx, dy);
        if (dist < 1e-3) continue;
        const ux = dx / dist, uy = dy / dist;
        const others = order.filter((o) => o !== id).map((o) => worldHull(o));
        const hl = hullCentered.get(id)!;
        const collides = (t: number) => {
          const moved = translatePoly(hl, p.x + ux * t, p.y + uy * t);
          return others.some((o) => polysOverlap(moved, o));
        };
        let lo = 0, hi = dist;
        if (!collides(hi)) lo = hi;
        else for (let k = 0; k < 18; k++) { const mid = (lo + hi) / 2; if (collides(mid)) hi = mid; else lo = mid; }
        P.set(id, { x: p.x + ux * lo, y: p.y + uy * lo });
      }
    }
    // gc = world-centroid − local-centroid, so each blob's centroid lands at P.
    for (const b of blobs) {
      const m = metrics.get(b.sectionId)!;
      const p = P.get(b.sectionId)!;
      blobCenters.set(b.sectionId, { x: p.x - m.cx, y: p.y - m.cy });
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
