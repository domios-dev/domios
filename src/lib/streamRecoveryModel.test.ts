import assert from "node:assert/strict";
import { streamRecoveryAction } from "./streamRecoveryModel";

assert.equal(streamRecoveryAction("eof", false), "reconnect");
for (const reason of ["http", "error"] as const) assert.equal(streamRecoveryAction(reason, false), "wait");
assert.equal(streamRecoveryAction("abort", false), "stop");
for (const reason of ["eof", "http", "error", "abort"] as const) assert.equal(streamRecoveryAction(reason, true), "stop");
console.log("8 passed, 0 failed");
