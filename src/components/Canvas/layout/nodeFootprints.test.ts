import { layoutPlanIntra } from "./intraPlan";
import { plan } from "./fixtures";
import { DEFAULT_CANVAS_SETTINGS as S } from "../settings";
import { footprintFor, registerNodeFootprint, __resetNodeFootprintsForTest } from "./nodeFootprints";
import { registerCanvasNode } from "../nodeRegistry";
import type { CanvasElement, Plan } from "../types";

let failures = 0;
function ok(msg: string, cond: boolean): void {
  console.log(`${cond ? "  ✓" : "  ✗"} ${msg}`);
  if (!cond) failures++;
}

function planWith(el: CanvasElement): Plan {
  return plan("fp", "o", 1, [], [], { elements: [el] });
}

const ghEl: CanvasElement = { id: "gh1", kind: "github-pr", canvas: { x: 100, y: 100, collapsed: false }, content: "PR #1", links: [] };

__resetNodeFootprintsForTest();

// 1. Unregistered kind → default element box.
{
  const fp = footprintFor("github-pr", ghEl, S);
  ok(`unregistered footprint = default box (${fp.width}×${fp.height})`, fp.width === S.taskWidth && fp.height === S.taskMinHeight);
  const node = layoutPlanIntra(planWith(ghEl), S).nodes.find((n) => n.id === "gh1");
  ok("layout places the element at the default box", !!node && node.box.width === S.taskWidth && node.box.height === S.taskMinHeight);
}

// 2. Registered footprint → honored by footprintFor AND by the layout.
{
  registerNodeFootprint("github-pr", () => ({ width: 412, height: 222 }));
  const fp = footprintFor("github-pr", ghEl, S);
  ok(`registered footprint honored by footprintFor (${fp.width}×${fp.height})`, fp.width === 412 && fp.height === 222);
  const node = layoutPlanIntra(planWith(ghEl), S).nodes.find((n) => n.id === "gh1");
  ok("layout sizes the element to the plugin footprint", !!node && node.box.width === 412 && node.box.height === 222);
  // A different, unregistered kind still gets the default — dispatch is per-kind.
  const other = footprintFor("sticky-note", ghEl, S);
  ok("a different unregistered kind still defaults", other.width === S.taskWidth && other.height === S.taskMinHeight);
}
__resetNodeFootprintsForTest();

// 3. registerCanvasNode forwards the descriptor's footprint into the registry.
{
  registerCanvasNode({ kind: "burndown", painter: { signature: () => "x", paint: () => [] }, footprint: () => ({ width: 333, height: 144 }) });
  const fp = footprintFor("burndown", ghEl, S);
  ok(`registerCanvasNode forwards footprint (${fp.width}×${fp.height})`, fp.width === 333 && fp.height === 144);
}
__resetNodeFootprintsForTest();

console.log(failures === 0 ? "\n✓ nodeFootprints seam tests passed" : `\n✗ ${failures} failed`);
if (failures > 0) process.exit(1);
