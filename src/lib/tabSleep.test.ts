import assert from "node:assert/strict";
import { asleepLabel, hasSleepingTranscript, sleepThresholdMinutes, tabSleepSettings } from "./tabSleep";

assert.deepEqual(tabSleepSettings({}), { enabled: true, idle_minutes: 60 });
assert.deepEqual(tabSleepSettings({ tab_sleep: { enabled: false, idle_minutes: 120 } }), { enabled: false, idle_minutes: 120 });
for (const value of ["", " ", "0", "-1", "1.5", "NaN", "Infinity", "9007199254740992"]) {
  assert.equal(sleepThresholdMinutes(value), null, value);
}
assert.equal(sleepThresholdMinutes("90"), 90);
assert.equal(asleepLabel({}, 10), "Asleep");
assert.equal(asleepLabel({ slept_at_ms: 1000 }, 7_201_000), "Asleep, idle 2h");
assert.equal(asleepLabel({ slept_at_ms: 1000 }, 0), "Asleep, idle 0h");
assert.equal(hasSleepingTranscript({ frozen: true, agent_session_id: "saved" }), false);
assert.equal(hasSleepingTranscript({ slept_at_ms: 0, agent_session_id: "saved" }), true);
assert.equal(hasSleepingTranscript({ slept_at_ms: 0 }), false);
assert.equal(hasSleepingTranscript({ slept_at_ms: 0, backing: { kind: "chat_backend", plugin_id: "fixture", session_id: "saved" } }), true);
console.log("tab sleep model: defaults, disabled persistence, threshold validation, clock, persisted session passed");
