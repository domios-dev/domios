// Run: npx tsx src/components/Canvas/layout/cloudFlush.test.ts
//
// Zone-first v3: the task shelf sits columnGapX past the plan; uniform grid cells
// start zonePad inside the zone hull (nearest tile == planRight + columnGapX + zonePad).
// Zero plan overlap and a substantial vertical field must hold at every scale.

import { largeCanvasLayout } from "./largeCanvasFixtures";
import { DEFAULT_CANVAS_SETTINGS as S } from "../settings";

let passed = 0;
let failed = 0;
function ok(label: string, cond: boolean) {
  if (cond) { console.log(`  ✓ ${label}`); passed++; }
  else { console.error(`  ✗ ${label}`); failed++; }
}

type Box = { x: number; y: number; width: number; height: number };
const intersect = (a: Box, b: Box) =>
  a.x < b.x + b.width && a.x + a.width > b.x && a.y < b.y + b.height && a.y + a.height > b.y;

console.log("task cloud flush-to-plan shape:");
for (const n of [100, 500, 1000] as const) {
  const layout = largeCanvasLayout(n);
  const frame = layout.frames[0];
  const plan = frame.nodes.find((x) => x.kind === "plan")!;
  const tasks = frame.nodes.filter((x) => x.kind === "task");
  const planRight = plan.box.x + plan.box.width;
  const nearest = Math.min(...tasks.map((t) => t.box.x));
  const gap = Math.round(nearest - planRight);
  const overlap = tasks.filter((t) => intersect(t.box, plan.box)).length;
  // Section-grouping (§zones) splits the field into per-section blobs, so the plan's
  // flush CONTACT is the nearest section blob, not one full-height wall. The real
  // invariants stay: exact flush gap, zero overlap, and a SUBSTANTIAL field — the
  // overall task field still spans >= half the plan height vertically.
  const cys = tasks.map((t) => t.box.y + t.box.height / 2).sort((a, b) => a - b);
  const fieldSpan = cys.length ? cys[cys.length - 1] - cys[0] : 0;

  const flushGap = S.columnGapX + S.zonePad;
  ok(`${n}: nearest task flush at planRight + columnGapX + zonePad (gap=${gap}=${flushGap})`, gap === flushGap);
  ok(`${n}: zero plan overlap`, overlap === 0);
  ok(`${n}: task field spans >= half the plan height`, fieldSpan >= plan.box.height * 0.5);
}

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) throw new Error(`${failed} test(s) failed`);
