// Run: npx tsx src/components/Canvas/planViewMode.test.ts

import {
  DEFAULT_PLAN_VIEW_MODE,
  isPlanViewMode,
  planViewModeLabel,
  PLAN_VIEW_MODE_LABELS,
  PLAN_VIEW_MODES,
  resolvePlanViewMode,
} from "./planViewMode";

type TestFn = () => void;
const tests: Array<[string, TestFn]> = [];
function test(name: string, fn: TestFn) {
  tests.push([name, fn]);
}
function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

test("PLAN_VIEW_MODES exposes exactly Spec Plan Board in order", () => {
  assert(PLAN_VIEW_MODES.join(",") === "spec,plan,board", `modes=${PLAN_VIEW_MODES.join(",")}`);
  assert(!PLAN_VIEW_MODES.includes("list" as never), "List is parked");
  assert(!PLAN_VIEW_MODES.includes("canvas" as never), "Canvas is parked");
});

test("DEFAULT_PLAN_VIEW_MODE is board", () => {
  assert(DEFAULT_PLAN_VIEW_MODE === "board", `default=${DEFAULT_PLAN_VIEW_MODE}`);
});

test("isPlanViewMode accepts only known modes", () => {
  assert(isPlanViewMode("canvas"), "canvas");
  assert(isPlanViewMode("list"), "list");
  assert(isPlanViewMode("spec"), "spec");
  assert(isPlanViewMode("plan"), "plan");
  assert(isPlanViewMode("board"), "board");
  assert(!isPlanViewMode("grid"), "grid rejected");
  assert(!isPlanViewMode(""), "empty rejected");
});

test("planViewModeLabel returns exact UI labels", () => {
  assert(planViewModeLabel("canvas") === "Canvas", "canvas label");
  assert(planViewModeLabel("list") === "List", "list label");
  assert(planViewModeLabel("board") === "Board", "board label");
  assert(planViewModeLabel("spec") === "Spec", "spec label");
  assert(planViewModeLabel("plan") === "Plan", "plan label");
  assert(PLAN_VIEW_MODE_LABELS.list === "List", "record label");
});

test("resolvePlanViewMode falls back to board", () => {
  assert(resolvePlanViewMode("spec") === "spec", "spec preserved");
  assert(resolvePlanViewMode("plan") === "plan", "plan preserved");
  assert(resolvePlanViewMode("list") === "list", "list preserved");
  assert(resolvePlanViewMode("bogus") === "board", "bogus -> board");
  assert(resolvePlanViewMode(null) === "board", "null -> board");
  assert(resolvePlanViewMode(undefined) === "board", "undefined -> board");
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
console.log(`\n✓ ${passed}/${tests.length} planViewMode tests passed`);
