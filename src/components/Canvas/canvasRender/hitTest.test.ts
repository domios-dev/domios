// Run: npx tsx src/components/Canvas/canvasRender/hitTest.test.ts

import type { LaidOutNode } from "../layout";
import type { ViewportTransform } from "../useCanvasViewport";
import type { HitRegion } from "./drawCard";
import { hitTestScene, type SceneDrawInput } from "./drawScene";

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

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);
