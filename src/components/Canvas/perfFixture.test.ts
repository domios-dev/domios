// Run: npx tsx src/components/Canvas/perfFixture.test.ts

import { buildDoneHeavyPerfFixturePlans } from "./perfFixture";
import { layoutWorkspace } from "./layout";
import { TaskStatus } from "./types";

let passed = 0;
let failed = 0;
function ok(label: string, cond: unknown) {
  if (cond) { console.log(`  ✓ ${label}`); passed++; }
  else { console.error(`  ✗ ${label}`); failed++; }
}

const [plan] = buildDoneHeavyPerfFixturePlans();
const active = plan.tasks.filter((t) => t.status === TaskStatus.InProgress);
const done = plan.tasks.filter((t) => t.status === TaskStatus.Done);

ok("done-heavy plan has 210 tasks", plan.tasks.length === 210);
ok("10 active tasks", active.length === 10);
ok("200 done tasks", done.length === 200);

const layout = layoutWorkspace([plan]);
const LEGACY_WIDTH = 6228;
const LEGACY_AREA = 61_000_000;
const area = layout.bounds.width * layout.bounds.height;
ok("done-heavy bounds width denser than legacy baseline", layout.bounds.width < LEGACY_WIDTH);
ok("done-heavy bounds area denser than legacy baseline", area < LEGACY_AREA);

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);
