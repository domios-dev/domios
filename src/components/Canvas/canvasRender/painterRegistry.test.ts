// Run: npx tsx src/components/Canvas/canvasRender/painterRegistry.test.ts
//
// The canvas render seam is LIVE: a plugin registers a painter for its kind, drawScene
// dispatches through it, and hitTestScene resolves a click to the painter's region.

import { Window } from "happy-dom";
import type { LaidOutNode } from "../layout";
import type { ViewportTransform } from "../useCanvasViewport";

// The bitmap cache calls document.createElement("canvas"); give it a DOM (getContext
// returns null headlessly → the cache skips painting, which is fine: we assert the
// dispatch + hit-test, and exercise paint() directly).
Object.defineProperty(globalThis, "document", { value: new Window().document, configurable: true });
import type { HitRegion } from "./drawCard";
import { drawScene, hitTestScene, type SceneDrawInput } from "./drawScene";
import { NodeBitmapCache } from "./nodeBitmapCache";
import { __unregisterCanvasPainterForTest, getCanvasPainter, registerCanvasPainter } from "./painterRegistry";

let passed = 0;
let failed = 0;
function ok(label: string, cond: unknown) {
  if (cond) { console.log(`  ✓ ${label}`); passed++; }
  else { console.error(`  ✗ ${label}`); failed++; }
}

const node = (id: string, kind: string, x: number, y: number, w: number, h: number): LaidOutNode => ({
  id, kind, planId: "p", box: { x, y, width: w, height: h }, iteration: null, manual: false,
});

// A synthetic plugin painter: signature off the model.v, paint records + returns regions.
let paintCalls = 0;
let sigCalls = 0;
const WIDGET_REGIONS: HitRegion[] = [
  { x: 0, y: 0, w: 120, h: 80, action: { type: "node" } },
  { x: 10, y: 50, w: 60, h: 16, action: { type: "section-button", sectionId: "sec-x" } },
];
registerCanvasPainter("widget", {
  signature: (m) => { sigCalls++; return `widget:${(m as { v: number }).v}`; },
  paint: () => { paintCalls++; return WIDGET_REGIONS; },
});

ok("getCanvasPainter returns the registered painter", !!getCanvasPainter("widget"));
ok("signature reads the model", getCanvasPainter("widget")!.signature({ v: 7 }) === "widget:7");
ok("unknown kind has no painter", getCanvasPainter("nope") === undefined);

// A no-op 2D context proxy (headless: measureText returns a width, everything else no-ops).
const ctx = new Proxy({}, { get: (_t, p) => (p === "measureText" ? () => ({ width: 0 }) : () => {}) }) as unknown as CanvasRenderingContext2D;

const widget = node("w1", "widget", 100, 100, 120, 80);
const models = new Map<string, unknown>([["w1", { v: 7 }]]);
const resolveModel = (n: LaidOutNode) => models.get(n.id);
const baseInput: SceneDrawInput = {
  nodes: [widget],
  edges: [],
  nodeById: new Map([["w1", widget]]),
  zoneLabels: [],
  groups: [],
  presenceByNode: new Map(),
  resolveModel,
  signatureOf: (n) => getCanvasPainter(n.kind)?.signature(resolveModel(n)) ?? null,
  paintNode: (c, n, w, h) => getCanvasPainter(n.kind)?.paint(c, { node: n, model: resolveModel(n), w, h, palette: {} as never }) ?? [],
  cache: new NodeBitmapCache(),
  palette: {} as never,
  dpr: 1,
  width: 1000,
  height: 1000,
};
const t: ViewportTransform = { x: 0, y: 0, scale: 1 };

// drawScene dispatches: it consults the painter's signature for the visible widget node.
sigCalls = 0;
drawScene(ctx, baseInput, t);
ok("drawScene dispatched to the painter (signature consulted)", sigCalls > 0);

// paintNode invokes the painter's paint and returns its regions (the draw path).
paintCalls = 0;
const regions = baseInput.paintNode(ctx, widget, 120, 80);
ok("paintNode invokes the painter's paint", paintCalls === 1);
ok("paintNode returns the painter's hit regions", regions.length === 2);

// hitTestScene resolves a click on the painter's region to the widget node + action.
const hitInput: SceneDrawInput = {
  ...baseInput,
  cache: { get: () => ({ w: 120, h: 80, regions: WIDGET_REGIONS }) } as unknown as NodeBitmapCache,
};
const hit = hitTestScene(hitInput, t, { x: 100 + 30, y: 100 + 56 }); // inside the section-button region
ok("hitTestScene resolves the click to the widget node", hit?.node.id === "w1");
ok("hitTestScene resolves to the painter's region action", hit?.action.type === "section-button" && (hit.action as { sectionId: string }).sectionId === "sec-x");

__unregisterCanvasPainterForTest("widget");
ok("painter can be unregistered", getCanvasPainter("widget") === undefined);

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);
