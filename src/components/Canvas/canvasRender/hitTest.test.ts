// Run: npx tsx src/components/Canvas/canvasRender/hitTest.test.ts

import type { LaidOutNode } from "../layout";
import type { ViewportTransform } from "../useCanvasViewport";
import { TaskStatus } from "../types";
import type { HitRegion } from "./drawCard";
import { hitTestScene, type SceneDrawInput } from "./drawScene";
import { buildSectionGroups, resolveSceneLod } from "./sceneLod";

let passed = 0;
let failed = 0;
function ok(label: string, cond: unknown) {
  if (cond) { console.log(`  ✓ ${label}`); passed++; }
  else { console.error(`  ✗ ${label}`); failed++; }
}

const node = (id: string, kind: string, x: number, y: number, w: number, h: number): LaidOutNode => ({
  id, kind, planId: "plan-1", box: { x, y, width: w, height: h }, iteration: null, manual: false,
});

// A spec node (0..200 wide, 0..160 tall) with: a section row band + a "→ tasks"
// button inside that band + an expand box top-right + the full-node fallback.
const specRegions: HitRegion[] = [
  { x: 0, y: 0, w: 200, h: 160, action: { type: "node" } },
  { x: 178, y: 8, w: 18, h: 18, action: { type: "expand" } },
  { x: 0, y: 60, w: 200, h: 19, action: { type: "section", sectionId: "sec-1" } },
  { x: 150, y: 61, w: 44, h: 15, action: { type: "section-button", sectionId: "sec-1" } },
];

// Cache stub: identity-mapped logical size == screen size (transform scale 1, no offset).
const cache = {
  get: (id: string) => (id === "spec-1" ? { w: 200, h: 160, regions: specRegions } : undefined),
} as unknown as SceneDrawInput["cache"];

const nodes: LaidOutNode[] = [
  node("spec-1", "spec", 0, 0, 200, 160),
  node("task-1", "task", 40, 40, 60, 40), // overlaps the spec — smaller, should win on body
];

const input = {
  nodes,
  cache,
  edges: [], nodeById: new Map(), zoneLabels: [], groups: [], presenceByNode: new Map(),
  getContent: () => null, palette: {} as never, dpr: 1, width: 1000, height: 1000,
} as unknown as SceneDrawInput;

const t: ViewportTransform = { x: 0, y: 0, scale: 1 };
const at = (x: number, y: number) => hitTestScene(input, t, { x, y });

ok("empty space → null", at(500, 500) === null);
ok("smallest node wins (task over spec)", at(60, 55)?.node.id === "task-1");
ok("task body → node action", at(60, 55)?.action.type === "node");
ok("expand box → expand", (() => { const h = at(186, 16); return h?.node.id === "spec-1" && h.action.type === "expand"; })());
{
  const h = at(170, 68); // inside the section row AND the button rect → button wins
  ok("button beats row on overlap", h?.action.type === "section-button" && (h.action as { sectionId: string }).sectionId === "sec-1");
}
{
  const h = at(120, 68); // section row, left of the button and clear of the task overlap
  ok("section row → section action", h?.action.type === "section" && (h.action as { sectionId: string }).sectionId === "sec-1");
}
ok("spec header (no region) → node", at(20, 4)?.action.type === "node");

{
  const taskById = new Map([
    ["task-done", { id: "task-done", section_ids: ["sec-1"], status: TaskStatus.Done } as never],
    ["task-live", { id: "task-live", section_ids: ["sec-1"], status: TaskStatus.InProgress } as never],
  ]);
  const lodNodes = [
    node("task-done", "task", 300, 300, 80, 50),
    node("task-live", "task", 420, 300, 80, 50),
  ];
  const groups = buildSectionGroups(lodNodes, taskById, () => "Section", new Map());
  const lodInput = {
    ...input,
    nodes: lodNodes,
    lodGroups: groups,
    taskById,
  } as unknown as SceneDrawInput;
  const readable = resolveSceneLod(1, groups, taskById);
  const marker = readable.markers[0];
  ok("done-stack marker exists", !!marker);
  if (marker) {
    const cx = marker.box.x * t.scale + t.x + (marker.box.width * t.scale) / 2;
    const cy = marker.box.y * t.scale + t.y + (marker.box.height * t.scale) / 2;
    const h = hitTestScene(lodInput, t, { x: cx, y: cy });
    ok("done-stack hit-test", h?.action.type === "done-stack");
  }

  const cluster = resolveSceneLod(0.3, groups, taskById);
  const chip = cluster.markers[0];
  ok("cluster chip at mid zoom", chip?.kind === "section-cluster");
  if (chip) {
    const clusterT: ViewportTransform = { x: 0, y: 0, scale: 0.3 };
    const cx = chip.box.x * clusterT.scale + clusterT.x + (chip.box.width * clusterT.scale) / 2;
    const cy = chip.box.y * clusterT.scale + clusterT.y + (chip.box.height * clusterT.scale) / 2;
    const h = hitTestScene(lodInput, clusterT, { x: cx, y: cy });
    ok("cluster chip hit-test", h?.action.type === "section-cluster");
  }
}

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);
