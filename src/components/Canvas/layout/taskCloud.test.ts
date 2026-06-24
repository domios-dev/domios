// Run: npx tsx src/components/Canvas/layout/taskCloud.test.ts

import type { Plan, Section, Task } from "../types";
import { DEFAULT_CANVAS_SETTINGS as S } from "../settings";
import { packCloud, layoutTaskCloud, ACTIVE_CORE_K, GOLDEN_ANGLE } from "./taskCloud";
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

// ── full layout: disk is right of the plan, no node overlap ──────────
{
  const sections = Array.from({ length: 8 }, (_, i) => section(`s${i}`));
  const tasks = Array.from({ length: 38 }, (_, i) =>
    task(`t${i}`, { section_ids: [`s${i % 8}`], created_at: i, status: i < 4 ? "in_progress" : "pending" }),
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

  // the disk reads as round: width ≈ height of the task cluster bbox.
  const xs = taskNodes.flatMap((n) => [n.box.x, n.box.x + n.box.width]);
  const ys = taskNodes.flatMap((n) => [n.box.y, n.box.y + n.box.height]);
  const w = Math.max(...xs) - Math.min(...xs);
  const h = Math.max(...ys) - Math.min(...ys);
  ok(`task cluster is roughly round (aspect ${(w / h).toFixed(2)} in [0.6,1.7])`, w / h > 0.6 && w / h < 1.7);
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
