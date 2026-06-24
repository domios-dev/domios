// Run: npx tsx src/components/Canvas/viewMode.test.ts

import { resolveGlobalMode } from "./viewMode";

type TestFn = () => void;
const tests: Array<[string, TestFn]> = [];
function test(name: string, fn: TestFn) {
  tests.push([name, fn]);
}
function assert(cond: unknown, msg: string): asserts cond {
  if (!cond) throw new Error(msg);
}

test("resolveGlobalMode: the manual setting is returned verbatim (zoom never switches — §22.1)", () => {
  assert(resolveGlobalMode("document", true) === "document", "document stays document");
  assert(resolveGlobalMode("card", true) === "card", "card stays card");
  assert(resolveGlobalMode("tile", true) === "tile", "tile stays tile");
});

test("resolveGlobalMode: text-off forces Tile (deliberate declutter, never empty box)", () => {
  assert(resolveGlobalMode("document", false) === "tile", "text-off overrides document");
  assert(resolveGlobalMode("card", false) === "tile", "text-off overrides card");
  assert(resolveGlobalMode("tile", false) === "tile", "tile + text-off stays tile");
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
