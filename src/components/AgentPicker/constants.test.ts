// Run: npx tsx src/components/AgentPicker/constants.test.ts

import { FIRST_RUN_GA_TAB_MARKER, isFirstRunGeneralTab } from "./constants";

const tests: Array<[string, () => void]> = [];
function test(name: string, fn: () => void) {
  tests.push([name, fn]);
}

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

test("isFirstRunGeneralTab matches marker", () => {
  assert(isFirstRunGeneralTab({ description: FIRST_RUN_GA_TAB_MARKER }), "marker hit");
  assert(!isFirstRunGeneralTab({ description: "other" }), "other miss");
  assert(!isFirstRunGeneralTab(undefined), "undefined miss");
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
