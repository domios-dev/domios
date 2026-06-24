// Run: npx tsx src/components/Canvas/viewMode.test.ts

import { autoViewMode, resolveGlobalMode } from "./viewMode";

type TestFn = () => void;
const tests: Array<[string, TestFn]> = [];
function test(name: string, fn: TestFn) {
  tests.push([name, fn]);
}
function assert(cond: unknown, msg: string): asserts cond {
  if (!cond) throw new Error(msg);
}

test("autoViewMode: far → tile, mid → card, near → document", () => {
  assert(autoViewMode(0.2) === "tile", "far is tile");
  assert(autoViewMode(0.44) === "tile", "just under far band is tile");
  assert(autoViewMode(0.6) === "card", "mid is card");
  assert(autoViewMode(0.84) === "card", "just under document band is card");
  assert(autoViewMode(1) === "document", "100% is document (reading view)");
  assert(autoViewMode(2.5) === "document", "near is document");
});

test("resolveGlobalMode: explicit setting wins over zoom", () => {
  assert(resolveGlobalMode("tile", 2.5, true) === "tile", "locked tile stays tile when zoomed in");
  assert(resolveGlobalMode("document", 0.1, true) === "document", "locked document stays at far zoom");
  assert(resolveGlobalMode("card", 2.5, true) === "card", "locked card stays");
});

test("resolveGlobalMode: auto follows zoom band", () => {
  assert(resolveGlobalMode("auto", 0.3, true) === "tile", "auto far → tile");
  assert(resolveGlobalMode("auto", 0.6, true) === "card", "auto mid → card");
  assert(resolveGlobalMode("auto", 1.5, true) === "document", "auto near → document");
});

test("resolveGlobalMode: text-off forces Tile (deliberate declutter, never empty box)", () => {
  assert(resolveGlobalMode("document", 2.5, false) === "tile", "text-off overrides document");
  assert(resolveGlobalMode("auto", 1.5, false) === "tile", "text-off overrides auto");
});

let failed = 0;
for (const [name, fn] of tests) {
  try {
    fn();
    console.log(`  ✓ ${name}`);
  } catch (e) {
    failed++;
    console.error(`  ✗ ${name}\n    ${e instanceof Error ? e.message : e}`);
  }
}
console.log(`\n${tests.length - failed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);
