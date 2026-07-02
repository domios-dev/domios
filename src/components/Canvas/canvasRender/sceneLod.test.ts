// Run: npx tsx src/components/Canvas/canvasRender/sceneLod.test.ts

import type { Box } from "../layout";
import { TaskStatus } from "../types";
import {
  buildSectionGroups,
  CANVAS_MIN_SCALE,
  lodMode,
  OVERVIEW_LOD_SCALE,
  resolveSceneLod,
} from "./sceneLod";

let passed = 0;
let failed = 0;
function ok(label: string, cond: unknown) {
  if (cond) { console.log(`  ✓ ${label}`); passed++; }
  else { console.error(`  ✗ ${label}`); failed++; }
}

const box = (x: number, y: number, w: number, h: number): Box => ({ x, y, width: w, height: h });

const taskById = new Map([
  ["t-active", { id: "t-active", section_ids: ["sec-a"], status: TaskStatus.InProgress, name: "active" } as never],
  ["t-done-1", { id: "t-done-1", section_ids: ["sec-a"], status: TaskStatus.Done, name: "done1" } as never],
  ["t-done-2", { id: "t-done-2", section_ids: ["sec-a"], status: TaskStatus.Done, name: "done2" } as never],
]);

const nodes = [
  { id: "t-active", kind: "task", planId: "p1", box: box(0, 0, 120, 80) },
  { id: "t-done-1", kind: "task", planId: "p1", box: box(140, 0, 120, 80) },
  { id: "t-done-2", kind: "task", planId: "p1", box: box(280, 0, 120, 80) },
];

const groups = buildSectionGroups(nodes, taskById, (_p, sid) => (sid === "sec-a" ? "Alpha" : sid), new Map());

ok("zoom floor matches overview threshold", CANVAS_MIN_SCALE === OVERVIEW_LOD_SCALE);
ok("overview mode at zoom floor", lodMode(OVERVIEW_LOD_SCALE) === "overview");
ok("readable mode above floor", lodMode(OVERVIEW_LOD_SCALE + 0.01) === "readable");

const overview = resolveSceneLod(0.4, groups, taskById);
ok("overview hides all tasks", overview.hiddenNodeIds.size === 3);
ok("overview emits one section cluster", overview.markers.length === 1 && overview.markers[0].kind === "section-cluster");
ok("cluster title preserved", overview.markers[0]?.title === "Alpha");

const readable = resolveSceneLod(0.8, groups, taskById);
ok("readable keeps active task visible", !readable.hiddenNodeIds.has("t-active"));
ok("readable hides done tasks", readable.hiddenNodeIds.has("t-done-1") && readable.hiddenNodeIds.has("t-done-2"));
ok("readable emits done-stack", readable.markers.length === 1 && readable.markers[0].kind === "done-stack");
ok("done-stack count", (readable.markers[0]?.counts.done ?? 0) === 2);

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);
