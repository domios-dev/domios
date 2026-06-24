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

// Fixed frames per mode (§18): the box equals exactly the designed height, so a
// node is never an empty shell (box matches content) and never overflows it.
ok("plan tile box == tileHeight", heightOf("tile", "plan") === S.tileHeight);
ok("plan card box == planCardHeight", heightOf("card", "plan") === S.planCardHeight);
ok("spec tile box == tileHeight", heightOf("tile", "spec-section") === S.tileHeight);
ok("spec card box == specCardHeight", heightOf("card", "spec-section") === S.specCardHeight);
ok("task tile box == tileHeight", heightOf("tile", "task") === S.tileHeight);
ok("task card box == taskCardHeight", heightOf("card", "task") === S.taskCardHeight);

// Document mode (content-fit) reserves MORE than the compact modes for big bodies.
ok("doc spec taller than card", heightOf("document", "spec-section") > heightOf("card", "spec-section"));
ok("doc task taller than card", heightOf("document", "task") > heightOf("card", "task"));
ok("card taller than tile (spec)", heightOf("card", "spec-section") > heightOf("tile", "spec-section"));

// No node overlaps another in any mode (containers excluded — they wrap children).
function noOverlap(m: CanvasViewMode): boolean {
  const real = docNodes(m).filter((n) => n.kind === "plan" || n.kind === "spec-section" || n.kind === "task");
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
