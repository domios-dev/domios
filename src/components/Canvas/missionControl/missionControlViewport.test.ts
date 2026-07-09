// Run: npx tsx src/components/Canvas/missionControl/missionControlViewport.test.ts

import { MC_PAN_IGNORE } from "./missionControlViewport";

type TestFn = () => void;
const tests: Array<[string, TestFn]> = [];
function test(name: string, fn: TestFn) {
  tests.push([name, fn]);
}
function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

test("pan ignore selector covers interactive mission-control nodes", () => {
  assert(MC_PAN_IGNORE.includes("[data-mc-interactive]"), "task cards");
  assert(MC_PAN_IGNORE.includes("button"), "buttons");
  assert(MC_PAN_IGNORE.includes(".mc-live-toolbar"), "toolbar");
});

let passed = 0;
for (const [name, fn] of tests) {
  try {
    fn();
    passed++;
    console.log(`  ✓ ${name}`);
  } catch (e) {
    console.error(`  ✗ ${name}`);
    console.error(e);
    process.exit(1);
  }
}
console.log(`\n✓ ${passed}/${tests.length} missionControlViewport tests passed`);
