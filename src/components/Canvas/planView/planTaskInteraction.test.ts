// Run: npx tsx src/components/Canvas/planView/planTaskInteraction.test.ts

import {
  DRAG_CLICK_THRESHOLD_PX,
  hasTextSelection,
  markPointerDown,
  markPointerMove,
  shouldSuppressPlanTaskActivate,
  type PlanTaskPointerState,
} from "./planTaskInteraction";

type TestFn = () => void | Promise<void>;
const tests: Array<[string, TestFn]> = [];
function test(name: string, fn: TestFn) {
  tests.push([name, fn]);
}
function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

test("markPointerMove sets dragged past threshold", () => {
  const state: PlanTaskPointerState = { x: 0, y: 0, dragged: false };
  markPointerDown(state, 10, 10);
  markPointerMove(state, 10 + DRAG_CLICK_THRESHOLD_PX + 1, 10);
  assert(state.dragged, "dragged after move");
  assert(shouldSuppressPlanTaskActivate(state), "suppress on drag");
});

test("shouldSuppressPlanTaskActivate respects text selection", () => {
  const state: PlanTaskPointerState = { x: 0, y: 0, dragged: false };
  const sel = {
    isCollapsed: false,
    toString: () => "selected text",
  };
  Object.defineProperty(globalThis, "window", {
    value: { getSelection: () => sel },
    configurable: true,
  });
  assert(hasTextSelection(), "selection detected");
  assert(shouldSuppressPlanTaskActivate(state), "suppress when selecting");
});

test("click allowed when no drag and no selection", () => {
  Object.defineProperty(globalThis, "window", {
    value: { getSelection: () => ({ isCollapsed: true, toString: () => "" }) },
    configurable: true,
  });
  const state: PlanTaskPointerState = { x: 0, y: 0, dragged: false };
  assert(!shouldSuppressPlanTaskActivate(state), "plain click proceeds");
});

let passed = 0;
(async () => {
  for (const [name, fn] of tests) {
    try {
      await fn();
      passed++;
      console.log(`  ✓ ${name}`);
    } catch (e) {
      console.error(`  ✗ ${name}`);
      console.error(e);
      process.exit(1);
    }
  }
  console.log(`\n✓ ${passed}/${tests.length} planTaskInteraction tests passed`);
})().catch((e) => {
  console.error(e);
  process.exit(1);
});
