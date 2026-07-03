// Run: npx tsx src/components/Canvas/canvasSubMode.test.ts

import {
  DEFAULT_CANVAS_SUB_MODE,
  isCanvasSubMode,
  resolveCanvasSubMode,
} from "./canvasSubMode";

type TestFn = () => void;
const tests: Array<[string, TestFn]> = [];
function test(name: string, fn: TestFn) {
  tests.push([name, fn]);
}
function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

test("default canvas sub-mode is mission-control", () => {
  assert(DEFAULT_CANVAS_SUB_MODE === "mission-control", "default");
  assert(resolveCanvasSubMode(null) === "mission-control", "fallback");
});

test("isCanvasSubMode accepts known modes only", () => {
  assert(isCanvasSubMode("mission-control"), "mission-control");
  assert(isCanvasSubMode("section-graph"), "section-graph");
  assert(!isCanvasSubMode("graph"), "unknown");
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
console.log(`\n✓ ${passed}/${tests.length} canvasSubMode tests passed`);
