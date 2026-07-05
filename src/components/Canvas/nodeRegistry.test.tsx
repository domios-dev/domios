
import { registerCanvasNode } from "./nodeRegistry";
import { __unregisterCanvasPainterForTest, getCanvasPainter, isCanvasNodeExpandable } from "./canvasRender/painterRegistry";
import { __resetNodeFootprintsForTest, footprintFor } from "./layout/nodeFootprints";
import { DEFAULT_CANVAS_SETTINGS } from "./settings";

let passed = 0;
let failed = 0;
function ok(label: string, cond: unknown) {
  if (cond) { console.log(`  ✓ ${label}`); passed++; }
  else { console.error(`  ✗ ${label}`); failed++; }
}

const painter = { signature: () => "burndown:1", paint: () => [] };
registerCanvasNode({ kind: "burndown", painter, footprint: () => ({ width: 333, height: 144 }) });

ok("registerCanvasNode forwards the painter to the canvas registry", getCanvasPainter("burndown") === painter);
{
  const fp = footprintFor("burndown", { id: "b", kind: "burndown", canvas: { x: 0, y: 0, collapsed: false }, content: "", links: [] }, DEFAULT_CANVAS_SETTINGS);
  ok(`registerCanvasNode forwards the footprint (${fp.width}×${fp.height})`, fp.width === 333 && fp.height === 144);
}
ok("a kind with no painter is undefined (plugins opt in)", getCanvasPainter("never-registered") === undefined);

// capabilities.expandable flows to the registry so drawCard can honor an opt-out.
ok("a node is expandable by default (View pill shows)", isCanvasNodeExpandable("burndown") === true);
registerCanvasNode({ kind: "static-widget", painter, capabilities: { expandable: false } });
ok("a plugin can opt out of expand (no View pill)", isCanvasNodeExpandable("static-widget") === false);

__unregisterCanvasPainterForTest("burndown");
__unregisterCanvasPainterForTest("static-widget");
__resetNodeFootprintsForTest();

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);
