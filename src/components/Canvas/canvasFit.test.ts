// Run: npx tsx src/components/Canvas/canvasFit.test.ts

import { buildDoneHeavyPerfFixturePlans } from "./perfFixture";
import { layoutWorkspace, unionBox } from "./layout";
import { computeFitTransform, fitZoomFloor, viewportShowsBounds } from "./useCanvasViewport";

let passed = 0;
let failed = 0;
function ok(label: string, cond: unknown) {
  if (cond) { console.log(`  ✓ ${label}`); passed++; }
  else { console.error(`  ✗ ${label}`); failed++; }
}

const viewport = { width: 1280, height: 800 };
const [plan] = buildDoneHeavyPerfFixturePlans();
const layout = layoutWorkspace([plan]);
const frame = layout.frames[0];
const taskBoxes = frame.nodes.filter((n) => n.kind === "task").map((n) => n.box);
const content = unionBox([frame.box, ...taskBoxes, ...frame.zones.map((z) => z.box).filter(Boolean) as never[]]);

const fit = computeFitTransform(content, viewport.width, viewport.height);
ok("fit transform computed", !!fit);
if (fit) {
  ok("fit scale within 10-400% band", fit.scale >= 0.1 && fit.scale <= 4);
  ok("fit camera shows all content", viewportShowsBounds(fit, content, viewport));

  const at47 = { ...fit, scale: 0.47 };
  ok("47% zoom still frames content when centered on fit", viewportShowsBounds(at47, content, viewport));

  const floor = fitZoomFloor(fit.scale);
  ok("zoom floor below 47% when fit is tighter", floor <= 0.47 || fit.scale >= 0.47);
  ok("cannot zoom out past fit-margin floor", 0.47 >= floor - 1e-6 || fit.scale > 0.47);
}

const planOnly = computeFitTransform(frame.nodes.find((n) => n.kind === "plan")!.box, viewport.width, viewport.height);
if (fit && planOnly) {
  ok("full-frame fit is not wider than plan-only (regression: empty-at-47%)", fit.scale <= planOnly.scale + 1e-6);
}

function transformAtScale(fitT: NonNullable<typeof fit>, scale: number) {
  const cx = viewport.width / 2;
  const cy = viewport.height / 2;
  const wx = (cx - fitT.x) / fitT.scale;
  const wy = (cy - fitT.y) / fitT.scale;
  return { scale, x: cx - wx * scale, y: cy - wy * scale };
}

if (fit) {
  for (const [label, scale] of [["fit", fit.scale], ["100%", 1], ["400%", 4]] as const) {
    const t = label === "fit" ? fit : transformAtScale(fit, scale);
    ok(`${label} camera shows content`, viewportShowsBounds(t, content, viewport));
    ok(`${label} scale within 10–400% band`, t.scale >= 0.1 && t.scale <= 4);
  }
}

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);
