// Run: npx tsx src/components/Canvas/layout/largeCanvasFixtures.test.ts

import {
  LARGE_CANVAS_WORLD_SIZES,
  largeCanvasFingerprint,
  largeCanvasLayoutStats,
  largeCanvasPlan,
} from "./largeCanvasFixtures";

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

console.log("large canvas fixtures - determinism:");
for (const size of LARGE_CANVAS_WORLD_SIZES) {
  const runs = Array.from({ length: 5 }, () => largeCanvasFingerprint(size));
  ok(`${size} tasks: 5 runs identical`, runs.every((fp) => fp === runs[0]));
}

console.log("\nlarge canvas fixtures - task counts:");
for (const size of LARGE_CANVAS_WORLD_SIZES) {
  const plan = largeCanvasPlan(size);
  ok(`${size} tasks in plan JSON`, plan.tasks.length === size);
  const stats = largeCanvasLayoutStats(size);
  ok(`${size} task nodes laid out`, stats.taskCount === size);
  ok(`${size} core nodes have zero overlap`, stats.coreOverlapCount === 0);
  ok(`${size} bounds width > 0`, stats.bounds.width > 0);
  ok(`${size} bounds height > 0`, stats.bounds.height > 0);
}

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) throw new Error(`${failed} test(s) failed`);
