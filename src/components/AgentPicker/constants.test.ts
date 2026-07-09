// Run: npx tsx src/components/AgentPicker/constants.test.ts

import { FIRST_RUN_GA_PANE_MARKER, isFirstRunGeneralPane } from "./constants";

const tests: Array<[string, () => void]> = [];
function test(name: string, fn: () => void) {
  tests.push([name, fn]);
}

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

test("isFirstRunGeneralPane matches marker", () => {
  assert(isFirstRunGeneralPane({ description: FIRST_RUN_GA_PANE_MARKER }), "marker hit");
  assert(!isFirstRunGeneralPane({ description: "other" }), "other miss");
  assert(!isFirstRunGeneralPane(undefined), "undefined miss");
});

let failed = 0;
for (const [name, fn] of tests) {
  try {
    fn();
    console.log(`ok ${name}`);
  } catch (e) {
    failed += 1;
    console.error(`FAIL ${name}:`, e);
  }
}
process.exit(failed > 0 ? 1 : 0);
