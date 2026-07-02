// Run: npx tsx src/components/Canvas/canvasRender/sceneLod.test.ts

import type { Box } from "../layout";
import { TaskStatus } from "../types";
import { CANVAS_MIN_SCALE } from "../useCanvasViewport";
import {
  buildSectionGroups,
  CLUSTER_LOD_SCALE,
  lodMode,
  resolveSceneLod,
  SUMMARY_LOD_SCALE,
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

ok("zoom floor is 10%", CANVAS_MIN_SCALE === 0.1);
ok("summary mode at zoom floor", lodMode(CANVAS_MIN_SCALE) === "summary");
ok("summary mode at summary threshold", lodMode(SUMMARY_LOD_SCALE) === "summary");
ok("cluster mode above summary", lodMode(SUMMARY_LOD_SCALE + 0.01) === "cluster");
ok("cluster mode at cluster threshold", lodMode(CLUSTER_LOD_SCALE) === "cluster");
ok("readable mode above cluster", lodMode(CLUSTER_LOD_SCALE + 0.01) === "readable");

const summary = resolveSceneLod(0.12, groups, taskById);
ok("summary hides all tasks", summary.hiddenNodeIds.size === 3);
ok("summary emits compact marker", summary.markers.length === 1 && summary.markers[0].kind === "section-summary");
ok("summary title preserved", summary.markers[0]?.title === "Alpha");

const cluster = resolveSceneLod(0.3, groups, taskById);
ok("cluster hides all tasks", cluster.hiddenNodeIds.size === 3);
ok("cluster emits section chip", cluster.markers.length === 1 && cluster.markers[0].kind === "section-cluster");

const readable = resolveSceneLod(0.8, groups, taskById);
ok("readable keeps active task visible", !readable.hiddenNodeIds.has("t-active"));
ok("readable hides done tasks", readable.hiddenNodeIds.has("t-done-1") && readable.hiddenNodeIds.has("t-done-2"));
ok("readable emits done-stack", readable.markers.length === 1 && readable.markers[0].kind === "done-stack");
ok("done-stack count", (readable.markers[0]?.counts.done ?? 0) === 2);

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);
