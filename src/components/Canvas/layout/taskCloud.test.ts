// Run: npx tsx src/components/Canvas/layout/taskCloud.test.ts

import type { Plan, Section, Task } from "../types";
import { DEFAULT_CANVAS_SETTINGS as S } from "../settings";
import { packCloud, layoutTaskCloud, ACTIVE_CORE_K, GOLDEN_ANGLE, TILE_GAP } from "./taskCloud";
import { layoutWorkspace, boxesOverlap } from "./index";

let passed = 0;
let failed = 0;
function ok(label: string, cond: boolean) {
  if (cond) { console.log(`  ✓ ${label}`); passed++; }
  else { console.error(`  ✗ ${label}`); failed++; }
}

const canvas = () => ({ x: null, y: null, collapsed: false });
function section(id: string): Section {
  return { id, title: id, body: "spec body " + id, subsections: [], canvas: canvas() };
}
function task(id: string, opts: Partial<Task> = {}): Task {
  return {
    id, name: id, type: "implement", model: null, model_rationale: null,
    status: "pending", status_label: null, section_ids: [], body: "",
    intent: null, assigned_pane_id: null, peer: null, parent_task_id: null,
    canvas: canvas(), milestones: [], created_at: 0, ...opts,
  };
}
function plan(tasks: Task[], sections: Section[] = []): Plan {
  return {
    id: "p", title: "p", sections, status: "active",
    owner: { pane_id: "o", session_id: "o", title: "o" }, project_path: "/p",
    created_at: 0, updated_at: 1, milestone_kinds: [], loops: [], elements: [],
    tasks, canvas: { viewport: null }, owner_history: [],
  };
}

// ── golden angle constant ──────────────────────────────────────────
ok("ψ ≈ 2.39996 rad (137.5°)", Math.abs(GOLDEN_ANGLE - 2.399963) < 1e-5);

// ── packCloud: non-overlap (circumscribed-circle guarantee) ─────────
{
  const items = Array.from({ length: 80 }, (_, i) => ({
    id: `t${i}`,
    w: 120 + (i % 5) * 24, // varied sizes — small tiles beside big ones
    h: 100 + (i % 7) * 20,
    slot: i,
  }));
  const pack = packCloud(items);
  ok("all 80 tiles packed", pack.items.length === 80);
  // Enclosing circles never overlap → inscribed rectangles never overlap.
  let circOverlap = false;
  for (let i = 0; i < pack.items.length; i++) {
    for (let j = i + 1; j < pack.items.length; j++) {
      const a = pack.items[i], b = pack.items[j];
      const ra = 0.5 * Math.hypot(a.w, a.h);
      const rb = 0.5 * Math.hypot(b.w, b.h);
      const d = Math.hypot(a.cx - b.cx, a.cy - b.cy);
      if (d < ra + rb - 1e-6) circOverlap = true;
    }
  }
  ok("no enclosing-circle overlap (→ no tile overlap)", !circOverlap);
  // §2.2 R grows ~√n: compact, far below a linear stack.
  ok(`disk radius bounded (${Math.round(pack.radius)} < 1600)`, pack.radius < 1600);
}

// ── center-fill: the disk is DENSE from the middle, not a hollow ring ─────
{
  const items = Array.from({ length: 40 }, (_, i) => ({ id: `t${i}`, w: 130, h: 110, slot: i }));
  const pack = packCloud(items);
  // The innermost tile (slot 0) sits AT the origin — old `cumArea+area/2` seeded
  // it ≈0.7ρ off-center, leaving a hole. Center-out fill puts tile 0 dead-center.
  const slot0 = pack.items[0];
  ok("innermost tile is at the disk center (no hole)", Math.hypot(slot0.cx, slot0.cy) < 1e-6);
  // Nothing orbits an empty middle: the nearest tile to the origin is the center tile.
  const minD = Math.min(...pack.items.map((it) => Math.hypot(it.cx, it.cy)));
  const rho = 0.5 * Math.hypot(130, 110) + TILE_GAP;
  ok(`center filled — nearest tile ≪ ρ (${minD.toFixed(1)} < ${(0.25 * rho).toFixed(1)})`, minD < 0.25 * rho);
}

// ── packCloud: determinism ──────────────────────────────────────────
{
  const items = Array.from({ length: 40 }, (_, i) => ({ id: `t${i}`, w: 130, h: 110, slot: i }));
  const a = packCloud(items);
  const b = packCloud([...items].reverse()); // input order must not matter — slot does
  const sameById = a.items.every((it) => {
    const m = b.items.find((x) => x.id === it.id)!;
    return m.cx === it.cx && m.cy === it.cy;
  });
  ok("pack is deterministic & input-order-independent (slot-keyed)", sameById && a.radius === b.radius);
}

// ── append-stability (§31): adding a newest task moves no existing tile ──
{
  const base = Array.from({ length: 12 }, (_, i) => ({ id: `t${i}`, w: 140, h: 120, slot: ACTIVE_CORE_K + i }));
  const before = packCloud(base);
  const after = packCloud([...base, { id: "new", w: 140, h: 120, slot: ACTIVE_CORE_K + 12 }]);
  const stable = before.items.every((it) => {
    const m = after.items.find((x) => x.id === it.id)!;
    return m.cx === it.cx && m.cy === it.cy;
  });
  ok("appending the outermost slot leaves every prior tile byte-identical", stable);
}

// ── two-zone sunflower: in_progress lands in the active core (inner) ──
{
  const tasks: Task[] = [];
  // many old done/pending tasks (history spiral) + a couple of active ones.
  for (let i = 0; i < 30; i++) tasks.push(task(`h${i}`, { status: i % 2 ? "done" : "pending", created_at: i }));
  tasks.push(task("act1", { status: "in_progress", created_at: 100 }));
  tasks.push(task("act2", { status: "in_progress", created_at: 101 }));
  const cloud = layoutTaskCloud(plan(tasks), S, new Map());
  const center = (id: string) => {
    const n = cloud.nodes.find((x) => x.id === id)!;
    return { x: n.box.x, y: n.box.y };
  };
  // active tiles sit nearer the disk center (0,0) than the median history tile.
  const distOf = (id: string) => Math.hypot(center(id).x, center(id).y);
  const histDists = tasks.filter((t) => t.id.startsWith("h")).map((t) => distOf(t.id)).sort((a, b) => a - b);
  const medianHist = histDists[Math.floor(histDists.length / 2)];
  ok("active task 1 inside the median history radius", distOf("act1") < medianHist);
  ok("active task 2 inside the median history radius", distOf("act2") < medianHist);
}

// ── §35 comet fold: active pulls LEFT (plan side), done sinks RIGHT/outer ──
{
  const tasks: Task[] = [];
  for (let i = 0; i < 14; i++) tasks.push(task(`act${i}`, { status: i < 3 ? "in_progress" : "pending", created_at: i }));
  for (let i = 0; i < 14; i++) tasks.push(task(`done${i}`, { status: "done", created_at: 100 + i }));
  const cloud = layoutTaskCloud(plan(tasks), S, new Map());
  const cx = (id: string) => {
    const n = cloud.nodes.find((x) => x.id === id)!;
    return n.box.x + n.box.width / 2; // local x: <0 = toward the plan
  };
  const actXs = tasks.filter((t) => t.id.startsWith("act")).map((t) => cx(t.id));
  const doneXs = tasks.filter((t) => t.id.startsWith("done")).map((t) => cx(t.id));
  const mean = (xs: number[]) => xs.reduce((a, b) => a + b, 0) / xs.length;
  ok(`active mean-x left of done mean-x (${Math.round(mean(actXs))} < ${Math.round(mean(doneXs))})`, mean(actXs) < mean(doneXs));
  // Flush-to-plan pack: the cloud's flat-left edge sits against the plan, so every tile is
  // right of it (x≥0). The comet-fold legibility invariant is now relative — active work
  // clusters nearer the plan (smaller x) than done — so the active centre of mass stays in
  // the plan-near half of the cloud, and active never extends as far out as the done centre.
  const allXs = [...actXs, ...doneXs];
  const midX = (Math.min(...allXs) + Math.max(...allXs)) / 2;
  ok("active centre of mass is in the plan-near half", mean(actXs) < midX);
}

// ── §35 fold is append-stable: a NEW task moves no existing tile ──
{
  // Core already full (K in_progress) so the fresh task lands on the outer history
  // spiral (slot = K + creationRank), not an open core slot → no ripple.
  const base: Task[] = [];
  for (let i = 0; i < ACTIVE_CORE_K; i++) base.push(task(`a${i}`, { status: "in_progress", created_at: i }));
  for (let i = 0; i < 6; i++) base.push(task(`d${i}`, { status: "done", created_at: 100 + i }));
  const before = layoutTaskCloud(plan(base), S, new Map());
  const after = layoutTaskCloud(plan([...base, task("fresh", { status: "in_progress", created_at: 999 })]), S, new Map());
  const stable = before.nodes.every((n) => {
    const m = after.nodes.find((x) => x.id === n.id)!;
    return m.box.x === n.box.x && m.box.y === n.box.y;
  });
  ok("appending a fresh task (core full) leaves every prior tile byte-identical", stable);
}

// ── subtask satellites: solid smaller tiles swarming around the parent ──
{
  const parent = task("P", { body: "x".repeat(400), created_at: 0 });
  const kids = Array.from({ length: 6 }, (_, i) => task(`c${i}`, { parent_task_id: "P", created_at: i + 1 }));
  const cloud = layoutTaskCloud(plan([parent, ...kids]), S, new Map());
  const taskNodes = cloud.nodes.filter((n) => n.kind === "task");
  // Every subtask is its OWN node (solid, packed) — not hidden inside the parent.
  ok("parent + 6 subtasks = 7 task nodes", taskNodes.length === 7);
  ok("all subtasks present as nodes", kids.every((k) => taskNodes.some((n) => n.id === k.id)));

  const pn = cloud.nodes.find((n) => n.id === "P")!;
  const cn = cloud.nodes.find((n) => n.id === "c0")!;
  const pArea = pn.box.width * pn.box.height;
  const cArea = cn.box.width * cn.box.height;
  ok(`satellite is smaller than its parent (${Math.round(cArea)} < ${Math.round(pArea)})`, cArea < pArea * 0.7);

  // Satellites swarm AROUND the parent: each child center sits close to the parent
  // center (within the local cluster), never flung across the disk.
  const pc = { x: pn.box.x + pn.box.width / 2, y: pn.box.y + pn.box.height / 2 };
  const orbit = (id: string) => {
    const n = cloud.nodes.find((x) => x.id === id)!;
    return Math.hypot(n.box.x + n.box.width / 2 - pc.x, n.box.y + n.box.height / 2 - pc.y);
  };
  const maxOrbit = Math.max(...kids.map((k) => orbit(k.id)));
  ok(`subtasks orbit the parent tightly (${Math.round(maxOrbit)} < 360)`, maxOrbit < 360);

  // No two tiles overlap inside the cluster.
  let overlap = false;
  for (let i = 0; i < taskNodes.length; i++)
    for (let j = i + 1; j < taskNodes.length; j++)
      if (boxesOverlap(taskNodes[i].box, taskNodes[j].box)) overlap = true;
  ok("no overlap among parent + satellites", !overlap);
}

// ── sibling clusters never collide (each parent keeps its own swarm) ────
{
  const tasks: Task[] = [];
  for (let p = 0; p < 5; p++) {
    tasks.push(task(`P${p}`, { created_at: p * 100 }));
    for (let c = 0; c < 4; c++) tasks.push(task(`P${p}_c${c}`, { parent_task_id: `P${p}`, created_at: p * 100 + c + 1 }));
  }
  const cloud = layoutTaskCloud(plan(tasks), S, new Map());
  const taskNodes = cloud.nodes.filter((n) => n.kind === "task");
  ok("5 parents × 4 subtasks all placed (25 nodes)", taskNodes.length === 25);
  let overlap = false;
  for (let i = 0; i < taskNodes.length; i++)
    for (let j = i + 1; j < taskNodes.length; j++)
      if (boxesOverlap(taskNodes[i].box, taskNodes[j].box)) overlap = true;
  ok("no overlap across sibling clusters' satellites", !overlap);
}

// ── full layout: disk is right of the plan, no node overlap ──────────
{
  const sections = Array.from({ length: 8 }, (_, i) => section(`s${i}`));
  // Balanced mix: active folds plan-near, done sinks outward → a right-opening fan (§35,
  // flush-to-plan pack — flat-left edge against the plan, not a centred round disk).
  const tasks = Array.from({ length: 38 }, (_, i) =>
    task(`t${i}`, { section_ids: [`s${i % 8}`], created_at: i, status: i < 4 ? "in_progress" : i % 2 ? "done" : "pending" }),
  );
  const lo = layoutWorkspace([plan(tasks, sections)]);
  const frame = lo.frames[0];
  const taskNodes = frame.nodes.filter((n) => n.kind === "task");
  ok("all 38 tasks placed", taskNodes.length === 38);

  const planNode = frame.nodes.find((n) => n.kind === "plan")!;
  const planRight = planNode.box.x + planNode.box.width;
  ok("every task tile sits right of the plan node", taskNodes.every((t) => t.box.x >= planRight - 0.5));

  // no two task tiles overlap (circumscribed-circle pack guarantees it).
  let overlap = false;
  for (let i = 0; i < taskNodes.length; i++)
    for (let j = i + 1; j < taskNodes.length; j++)
      if (boxesOverlap(taskNodes[i].box, taskNodes[j].box)) overlap = true;
  ok("no task-tile overlap in the packed disk", !overlap);

  // the flush-to-plan fan reads as a balanced bloom (flat-left, right-opening): neither a
  // wide sliver nor a tall spike. The pack is intentionally not a centred round disk.
  const xs = taskNodes.flatMap((n) => [n.box.x, n.box.x + n.box.width]);
  const ys = taskNodes.flatMap((n) => [n.box.y, n.box.y + n.box.height]);
  const w = Math.max(...xs) - Math.min(...xs);
  const h = Math.max(...ys) - Math.min(...ys);
  ok(`task fan is balanced (aspect ${(w / h).toFixed(2)} in [0.5,1.8])`, w / h > 0.5 && w / h < 1.8);
}

// ── full layout determinism ──────────────────────────────────────────
{
  const sections = [section("s0")];
  const mk = () => plan(Array.from({ length: 20 }, (_, i) => task(`t${i}`, { section_ids: ["s0"], created_at: i })), sections);
  const a = layoutWorkspace([mk()]).frames[0].nodes.filter((n) => n.kind === "task");
  const b = layoutWorkspace([mk()]).frames[0].nodes.filter((n) => n.kind === "task");
  const same = a.every((n, i) => n.id === b[i].id && n.box.x === b[i].box.x && n.box.y === b[i].box.y);
  ok("packed cloud layout is deterministic end-to-end", same);
}

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) throw new Error(`${failed} test(s) failed`);
