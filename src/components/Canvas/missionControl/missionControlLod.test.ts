// Run: npx tsx src/components/Canvas/missionControl/missionControlLod.test.ts

import { CANVAS_MIN_SCALE, CANVAS_MAX_SCALE } from "../useCanvasViewport";
import {
  MC_LOD_FAR_MAX,
  MC_LOD_MID_MAX,
  resolveMissionControlLod,
} from "./missionControlLod";

type TestFn = () => void;
const tests: Array<[string, TestFn]> = [];
function test(name: string, fn: TestFn) {
  tests.push([name, fn]);
}
function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

test("far LOD at minimum zoom", () => {
  assert(resolveMissionControlLod(CANVAS_MIN_SCALE) === "far", "min scale");
  assert(resolveMissionControlLod(MC_LOD_FAR_MAX) === "far", "far threshold");
});

test("mid LOD between far and close thresholds", () => {
  assert(resolveMissionControlLod(MC_LOD_FAR_MAX + 0.05) === "mid", "above far");
  assert(resolveMissionControlLod(MC_LOD_MID_MAX) === "mid", "mid threshold");
});

test("close LOD above mid threshold", () => {
  assert(resolveMissionControlLod(MC_LOD_MID_MAX + 0.05) === "close", "above mid");
  assert(resolveMissionControlLod(CANVAS_MAX_SCALE) === "close", "max scale");
});

test("LOD clamps out-of-range scale inputs", () => {
  assert(resolveMissionControlLod(0.001) === "far", "below min");
  assert(resolveMissionControlLod(99) === "close", "above max");
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
console.log(`\n✓ ${passed}/${tests.length} missionControlLod tests passed`);
