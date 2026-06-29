// Run: npx tsx src/components/Canvas/settings.test.ts

import { DEFAULT_CANVAS_SETTINGS as S } from "./settings";

let passed = 0;
let failed = 0;
function ok(label: string, cond: boolean) {
  if (cond) { console.log(`  ✓ ${label}`); passed++; }
  else { console.error(`  ✗ ${label}`); failed++; }
}

// worlds-v3 §1 hard size invariant: a task's enclosing circle is always smaller
// than the smallest big node — ½·√(taskMaxW² + taskMaxH²) < min(specW,specH,planW,planH)/2.
{
  const taskCircleR = 0.5 * Math.hypot(S.taskMaxW, S.taskMaxH);
  const smallestBigHalf = Math.min(S.specNodeW, S.specNodeH, S.planWidth, S.planHeight) / 2;
  ok(`task circle radius (${taskCircleR.toFixed(1)}) < smallest big node half (${smallestBigHalf})`, taskCircleR < smallestBigHalf);
}

// §1 size hierarchy: plan is biggest in BOTH axes; a task can never reach big-node size.
ok("plan wider than spec", S.planWidth > S.specNodeW);
ok("plan taller than spec", S.planHeight > S.specNodeH);
ok("task ceiling below spec width", S.taskMaxW < S.specNodeW);
ok("task ceiling below spec height", S.taskMaxH < S.specNodeH);

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) throw new Error(`${failed} test(s) failed`);
