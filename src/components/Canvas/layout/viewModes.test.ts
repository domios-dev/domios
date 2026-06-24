// Run: npx tsx src/components/Canvas/layout/viewModes.test.ts

import type { Plan, Section, Task } from "../types";
import { DEFAULT_CANVAS_SETTINGS as S } from "../settings";
import type { CanvasViewMode } from "../cards/types";
import { layoutWorkspace, boxesOverlap, type NodeModeResolver } from "./index";

let passed = 0;
let failed = 0;
function ok(label: string, cond: boolean) {
  if (cond) { console.log(`  ✓ ${label}`); passed++; }
  else { console.error(`  ✗ ${label}`); failed++; }
}

const canvas = () => ({ x: null, y: null, collapsed: false });
const LONG = Array.from({ length: 40 }, (_, i) => `line ${i} with enough text to wrap several times over`).join("\n");

function section(id: string): Section {
  return { id, title: id, body: LONG, subsections: [], canvas: canvas() };
}
function task(id: string, sectionIds: string[]): Task {
  return {
    id, name: id, type: "implement", model: "claude:opus", model_rationale: "because",
    status: "in_progress", status_label: null, section_ids: sectionIds, body: LONG,
    intent: "do the thing", assigned_pane_id: null, peer: null, parent_task_id: null,
    canvas: canvas(), milestones: [],
  };
}
function plan(): Plan {
  return {
    id: "p1", title: "plan", sections: [section("s1"), section("s2")], status: "active",
    owner: { pane_id: "o", session_id: "o", title: "o" }, project_path: "/p",
    created_at: 0, updated_at: 1, milestone_kinds: [], loops: [], elements: [],
    tasks: [task("t1", ["s1"]), task("t2", ["s2"])], canvas: { viewport: null }, owner_history: [],
  };
}

const fixed = (m: CanvasViewMode): NodeModeResolver => () => m;
const docNodes = (m: CanvasViewMode) =>
  layoutWorkspace([plan()], S, fixed(m)).frames.flatMap((f) => f.nodes);

const heightOf = (m: CanvasViewMode, kind: string) =>
  docNodes(m).find((n) => n.kind === kind)!.box.height;

// FIXED plan-node frame per MANUAL mode (§22.1) — content-INDEPENDENT, the MODE
// (not zoom, not content) sets the size. Tile/Card/Document = fixed planТH/CH/DH.
ok("plan tile box == planTileH", heightOf("tile", "plan") === S.planTileH);
ok("plan card box == planCardH", heightOf("card", "plan") === S.planCardH);
ok("plan document box == planDocH", heightOf("document", "plan") === S.planDocH);
ok("plan document taller than card taller than tile", S.planDocH > S.planCardH && S.planCardH > S.planTileH);
const planWidthsFixed = (["tile", "card", "document"] as CanvasViewMode[]).every((m) => docNodes(m).find((n) => n.kind === "plan")!.box.width === S.planWidth);
ok("plan width fixed at planWidth across modes", planWidthsFixed);

// Task cells stay fixed per mode (tile/card); document is content-fit (§23.2 cells).
ok("task tile box == tileHeight", heightOf("tile", "task") === S.tileHeight);
ok("task card box == taskCardHeight", heightOf("card", "task") === S.taskCardHeight);
ok("doc task taller than card", heightOf("document", "task") > heightOf("card", "task"));

// Merged node: there are NO standalone spec-section nodes in any mode (§22.2).
for (const m of ["tile", "card", "document"] as CanvasViewMode[]) {
  ok(`no spec-section nodes in ${m} mode`, !docNodes(m).some((n) => n.kind === "spec-section"));
}

// No node overlaps another in any mode (containers excluded — they wrap children).
function noOverlap(m: CanvasViewMode): boolean {
  const real = docNodes(m).filter((n) => n.kind === "plan" || n.kind === "task");
  for (let i = 0; i < real.length; i++)
    for (let j = i + 1; j < real.length; j++)
      if (boxesOverlap(real[i].box, real[j].box)) return false;
  return true;
}
ok("no node overlap in document mode", noOverlap("document"));
ok("no node overlap in card mode", noOverlap("card"));
ok("no node overlap in tile mode", noOverlap("tile"));

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);
