import assert from "node:assert/strict";
import { shouldWakeOnOpen } from "./wakeOnOpen";

assert.equal(shouldWakeOnOpen({ frozen: true }, true, false), true);
assert.equal(shouldWakeOnOpen({ frozen: true }, false, false), false);
assert.equal(shouldWakeOnOpen({ frozen: true }, true, true), false);
assert.equal(shouldWakeOnOpen({ frozen: false }, true, false), false);
assert.equal(shouldWakeOnOpen({}, true, false), false);
assert.equal(shouldWakeOnOpen(undefined, true, false), false);
const missing = { backing: { kind: "pty" as const }, pty_id: null, frozen: false };
assert.equal(shouldWakeOnOpen(missing, true, false), true);
assert.equal(shouldWakeOnOpen(missing, false, false), false);
assert.equal(shouldWakeOnOpen(missing, true, true), false);
assert.equal(shouldWakeOnOpen({ ...missing, protected: true }, true, false), false);
assert.equal(shouldWakeOnOpen({ ...missing, boot_stage: "spawn_failed" }, true, false), false);
assert.equal(shouldWakeOnOpen({ ...missing, pty_id: "owned" }, true, false), false);
assert.equal(shouldWakeOnOpen({ ...missing, backing: { kind: "chat_backend", plugin_id: "fixture", session_id: "own" } }, true, false), false);
assert.equal(shouldWakeOnOpen({ ...missing, close_restore: { command: null, stopped: true, record_id: null, exact_session: true } }, true, false), false);
console.log("wake-on-open decisions passed");
