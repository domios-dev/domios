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

test("AgentReady event forwards booting and session binding", () => {
  const launchGeneration = { epoch: 3, operation_id: "launch-3", pty_id: "pty-ready", binding_nonce: "binding-ready" };
  const action = tabBootStageEventAction({ tab_id: "ready", stage: "agent_ready", agent_booting: false, session_bound: true,
    launch_generation: launchGeneration });
  assert(action.type === "UPDATE_BOOT_STAGE", "action type");
  assert(action.agent_booting === false, "booting mapped");
  assert(action.session_bound === true, "binding mapped");
  assert(action.launchGeneration === launchGeneration, "launch identity mapped");
});

test("lazy first-turn event forwards its launch-scoped composer evidence", () => {
  const launch = {
    pty_id: "own-pty",
    binding_nonce: "own-nonce",
    reason: "composer ready",
    awaiting_binding_ack: true,
    first_turn_input_ready: true,
  };
  const action = tabBootStageEventAction({
    tab_id: "lazy",
    stage: "provider_ready",
    provisional_launch: launch,
  });
  assert(action.type === "UPDATE_BOOT_STAGE", "action type");
  assert(action.provisional_launch === launch, "provisional composer evidence mapped");
});

console.log(`bootStageEvent tests: ${failures === 0 ? "all passed" : `${failures} failed`}`);
if (failures > 0) throw new Error(`${failures} test(s) failed`);
