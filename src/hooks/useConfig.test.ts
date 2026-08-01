import { defaultConfig } from "./useConfig";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const memory = defaultConfig().memory;
assert(memory?.extraction_model === "", "extraction model waits for the provider catalog");
assert(memory?.dreaming_model === "", "dreaming model waits for the provider catalog");
assert(memory?.worker_model === "", "worker model waits for the provider catalog");

const orchestration = defaultConfig().orchestration;
assert(orchestration.plan_reminders, "plan reminders default on");
assert(orchestration.nudge_interval_secs === 1800, "plan reminders default to 1800 seconds");
assert(!(["nudge_after", "turns"].join("_") in orchestration), "turn configuration must not be exposed");
assert(!(["idle", "poke"].join("_") in orchestration), "idle configuration must not be exposed");

console.log("✓ config defaults contain only scheduler-owned plan reminder cadence");
