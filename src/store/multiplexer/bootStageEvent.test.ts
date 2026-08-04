// Run: npx tsx src/store/multiplexer/bootStageEvent.test.ts

import { tabBootStageEventAction } from "./index";

let failures = 0;
function test(name: string, fn: () => void) {
  try {
    fn();
    console.log(`  ✓ ${name}`);
  } catch (e) {
    failures++;
    console.error(`  ✗ ${name}: ${e instanceof Error ? e.message : String(e)}`);
  }
}

function assert(cond: unknown, msg: string): asserts cond {
  if (!cond) throw new Error(msg);
}

test("tab-boot-stage event maps to reducer action with reason", () => {
  const action = tabBootStageEventAction({
    tab_id: "p-1",
    stage: "spawn_failed",
    reason: "provider missing",
  });
  assert(action.type === "UPDATE_BOOT_STAGE", "action type");
  assert(action.tabId === "p-1", "Tab id mapped");
  assert(action.stage === "spawn_failed", "stage mapped");
  assert(action.reason === "provider missing", "reason mapped");
});

console.log(`bootStageEvent tests: ${failures === 0 ? "all passed" : `${failures} failed`}`);
if (failures > 0) throw new Error(`${failures} test(s) failed`);
