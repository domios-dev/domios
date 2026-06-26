// Run: npx tsx src/components/Canvas/flyToTarget.test.ts

import { resolveFlyToNodeId } from "./flyToTarget";

type TestFn = () => void;
const tests: Array<[string, TestFn]> = [];
function test(name: string, fn: TestFn) {
  tests.push([name, fn]);
}
function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const board = new Set(["pl_a", "pl_a::spec", "pl_b", "ghost:pl_old"]);
const has = (id: string) => board.has(id);

test("spec chip lands on its plan's spec node, not the raw entity id", () => {
  assert(resolveFlyToNodeId("spec", "spec-xyz", "pl_a", has) === "pl_a::spec", "spec node from plan ctx");
});

test("spec chip falls back to the raw id when the spec node is absent", () => {
  assert(resolveFlyToNodeId("spec", "pl_b", "pl_missing", has) === "pl_b", "raw id fallback");
});

test("spec chip with no resolvable target returns null", () => {
  assert(resolveFlyToNodeId("spec", "nope", "pl_missing", has) === null, "null when nothing flyable");
});

test("lineage chip flies to the linked plan when present", () => {
  assert(resolveFlyToNodeId("follows", "pl_b", "pl_a", has) === "pl_b", "linked plan node");
});

test("lineage chip flies to the archived ghost when only the ghost is on the board", () => {
  assert(resolveFlyToNodeId("supersedes", "pl_old", "pl_a", has) === "ghost:pl_old", "ghost fallback");
});

test("lineage chip with an off-board target returns null", () => {
  assert(resolveFlyToNodeId("follows", "pl_gone", "pl_a", has) === null, "null off-board");
});

test("blank ids never resolve", () => {
  assert(resolveFlyToNodeId("follows", "  ", "pl_a", has) === null, "blank lineage id");
});

function main() {
  let passed = 0;
  for (const [name, fn] of tests) {
    try {
      fn();
      passed++;
      console.log(`  ✓ ${name}`);
    } catch (e) {
      console.error(`  ✗ ${name}`);
      console.error(`    ${e instanceof Error ? e.message : e}`);
      process.exit(1);
    }
  }
  console.log(`\n✓ ${passed}/${tests.length} flyToTarget tests passed`);
}

main();
