import { defaultConfig, mergeConfigEvent } from "./useConfig";

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

const authenticated = defaultConfig();
authenticated.api.token = "runtime-secret";
const redacted = structuredClone(authenticated);
redacted.api.token = null;
assert(
  mergeConfigEvent(authenticated, redacted).api.token === "runtime-secret",
  "a redacted config event cannot erase the desktop runtime token",
);

const rotated = structuredClone(authenticated);
rotated.api.token = "rotated-secret";
assert(
  mergeConfigEvent(authenticated, rotated).api.token === "rotated-secret",
  "an explicit token rotation replaces the previous runtime token",
);

console.log("✓ config defaults and runtime token event merging are stable");
