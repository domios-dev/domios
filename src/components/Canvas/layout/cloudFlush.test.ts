// Run: npx tsx src/components/Canvas/layout/cloudFlush.test.ts
//
// Guards the #1 fix: the task cloud's plan-facing edge sits FLUSH against the plan
// (nearest tile's left edge == planRight + columnGapX), with zero plan overlap, and
// the flush column spans roughly the plan height. Any future packer change that
// regresses the flush shape (e.g. reverting to a radius-offset circular disk) fails here.

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
  const flush = tasks.filter((t) => t.box.x - planRight <= S.columnGapX + 12);
  const ys = flush.map((t) => t.box.y + t.box.height / 2).sort((a, b) => a - b);
  const span = ys.length ? ys[ys.length - 1] - ys[0] : 0;

  ok(`${n}: nearest task flush at planRight + columnGapX (gap=${gap}=${S.columnGapX})`, gap === S.columnGapX);
  ok(`${n}: zero plan overlap`, overlap === 0);
  ok(`${n}: flush column spans >= half the plan height`, span >= plan.box.height * 0.5);
}

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) throw new Error(`${failed} test(s) failed`);
