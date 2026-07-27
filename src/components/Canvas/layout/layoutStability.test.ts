// Run: npx tsx src/components/Canvas/layout/layoutStability.test.ts

import type { Plan, Section, Task } from "../types";
import { layoutWorkspace, type Box } from "./index";

let passed = 0;
let failed = 0;

function ok(label: string, cond: boolean) {
  if (cond) { console.log(`  ✓ ${label}`); passed++; }
  else { console.error(`  ✗ ${label}`); failed++; }
}

const TOL = 0.5;

const canvas = () => ({ x: null, y: null, collapsed: false });

function section(id: string): Section {
  return { id, title: id, body: "", subsections: [], canvas: canvas() };
}

function task(id: string, opts: Partial<Task> = {}): Task {
  return {
    id,
    name: id,
    type: "implement",
    model: null,
    model_rationale: null,
    status: "pending",
    status_label: null,
    section_ids: [],
    body: "",
    intent: null,
    assigned_tab_id: null,
    peer: null,
    parent_task_id: null,
    canvas: canvas(),
    milestones: [],
    created_at: 0,
    ...opts,
  };
}

function plan(tasks: Task[], sections: Section[] = []): Plan {
  return {
    id: "pl_stab",
    title: "stab",
    sections,
    status: "active",
    owner: { tab_id: "o", session_id: "o", title: "o" },
    project_path: "/p",
    created_at: 0,
    updated_at: 1,
    milestone_kinds: [],
    loops: [],
    elements: [],
    tasks,
    canvas: { viewport: null },
    owner_history: [],
  };
}

function taskBoxes(layout: ReturnType<typeof layoutWorkspace>): Map<string, Box> {
  const m = new Map<string, Box>();
  for (const n of layout.frames[0].nodes.filter((x) => x.kind === "task")) {
    m.set(n.id, { ...n.box });
  }
  return m;
}

function boxDelta(a: Box, b: Box): number {
  return Math.hypot(a.x - b.x, a.y - b.y);
}

function maxDeltaForIds(before: Map<string, Box>, after: Map<string, Box>, ids: string[]): number {
  let max = 0;
  for (const id of ids) {
    const a = before.get(id);
    const b = after.get(id);
    if (!a || !b) continue;
    max = Math.max(max, boxDelta(a, b));
  }
  return max;
}

console.log("multi-section insert stability:");
{
  const sections = [section("s0"), section("s1"), section("s2")];
  const base: Task[] = [];
  for (let s = 0; s < 3; s++) {
    for (let i = 0; i < 8; i++) {
      base.push(task(`s${s}_t${i}`, { section_ids: [`s${s}`], created_at: s * 10 + i, status: "pending" }));
    }
  }
  const before = layoutWorkspace([plan(base, sections)]);
  const after = layoutWorkspace([
    plan([...base, task("fresh", { section_ids: ["s1"], created_at: 999, status: "pending" })], sections),
  ]);
  const b = taskBoxes(before);
  const a = taskBoxes(after);

  const s0 = base.filter((t) => t.section_ids[0] === "s0").map((t) => t.id);
  const s2 = base.filter((t) => t.section_ids[0] === "s2").map((t) => t.id);
  ok(`unrelated s0 tasks stable (max Δ ${maxDeltaForIds(b, a, s0).toFixed(1)}px)`, maxDeltaForIds(b, a, s0) <= TOL);
  ok(`unrelated s2 tasks stable (max Δ ${maxDeltaForIds(b, a, s2).toFixed(1)}px)`, maxDeltaForIds(b, a, s2) <= TOL);

  const planBefore = before.frames[0].nodes.find((n) => n.kind === "plan")!.box;
  const planAfter = after.frames[0].nodes.find((n) => n.kind === "plan")!.box;
  ok("plan node pinned on insert", boxDelta(planBefore, planAfter) <= TOL);
}

console.log("\nsingle-section append stability:");
{
  const sections = [section("s0")];
  const base = Array.from({ length: 12 }, (_, i) =>
    task(`t${i}`, { section_ids: ["s0"], created_at: i, status: "pending" }),
  );
  const before = layoutWorkspace([plan(base, sections)]);
  const after = layoutWorkspace([
    plan([...base, task("new", { section_ids: ["s0"], created_at: 99 })], sections),
  ]);
  const b = taskBoxes(before);
  const a = taskBoxes(after);
  const ids = base.map((t) => t.id);
  ok(`all prior tiles stable (max Δ ${maxDeltaForIds(b, a, ids).toFixed(1)}px)`, maxDeltaForIds(b, a, ids) <= TOL);
}

console.log("\ncross-section body update stability:");
{
  const sections = [section("s0"), section("s1")];
  const base = [
    ...Array.from({ length: 6 }, (_, i) => task(`s0_t${i}`, { section_ids: ["s0"], created_at: i })),
    ...Array.from({ length: 6 }, (_, i) => task(`s1_t${i}`, { section_ids: ["s1"], created_at: 10 + i })),
  ];
  const before = layoutWorkspace([plan(base, sections)]);
  const updated = base.map((t) => (t.id === "s1_t0" ? { ...t, body: "x".repeat(5000) } : t));
  const after = layoutWorkspace([plan(updated, sections)]);
  const b = taskBoxes(before);
  const a = taskBoxes(after);
  const s0 = base.filter((t) => t.section_ids[0] === "s0").map((t) => t.id);
  ok(`s0 stable when s1 body grows (max Δ ${maxDeltaForIds(b, a, s0).toFixed(1)}px)`, maxDeltaForIds(b, a, s0) <= TOL);
}

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) throw new Error(`${failed} test(s) failed`);
