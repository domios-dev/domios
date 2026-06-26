// Run: npx tsx src/components/Canvas/richOverlays.test.ts

import { largeCanvasLayout, largeCanvasPlan } from "./layout/largeCanvasFixtures";
import { MAX_ACTIVE_RICH_OVERLAYS, buildOverlayNodes } from "./richOverlays";
import { buildCanvasScene, projectWorldBox } from "./scene";

let passed = 0;
let failed = 0;

function ok(label: string, cond: boolean) {
  if (cond) {
    console.log(`  ✓ ${label}`);
    passed++;
  } else {
    console.error(`  ✗ ${label}`);
    failed++;
  }
}

function sameJson(a: unknown, b: unknown): boolean {
  return JSON.stringify(a) === JSON.stringify(b);
}

console.log("rich overlay budget:");
{
  const plan = largeCanvasPlan(1000);
  const layout = largeCanvasLayout(1000);
  const transform = { x: 80, y: 40, scale: 0.9 };
  const scene = buildCanvasScene({ layout, plans: [plan], cameraScale: transform.scale });
  const selectedIds = plan.tasks.map((task) => task.id);
  const overlays = buildOverlayNodes({
    scene,
    transform,
    focusedId: selectedIds[0],
    selectedIds,
  });
  ok("large fixture active overlays are bounded", overlays.length === MAX_ACTIVE_RICH_OVERLAYS);
  ok("focused overlay has markdown, Mermaid, and copy capability", overlays[0]?.capabilities.markdown && overlays[0]?.capabilities.mermaid && overlays[0]?.capabilities.copy);
  ok("focused overlay records that rich rendering preserves world box", overlays[0]?.preservesWorldBox === true);

  const source = scene.nodes.find((node) => node.id === overlays[0]?.id)!;
  ok("screen rect is exact world box projected through camera", sameJson(overlays[0]?.screenRect, projectWorldBox(source.worldBox, transform)));
  ok("world box is not resized for rich overlay", sameJson(overlays[0]?.worldBox, source.worldBox));
}

if (failed) {
  console.error(`\n${failed} failed, ${passed} passed`);
  process.exit(1);
}
console.log(`\n${passed} passed`);
