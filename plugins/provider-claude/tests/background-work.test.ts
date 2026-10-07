import assert from "node:assert/strict";
import {parseBackgroundWork} from "../src/backgroundWork";
assert.deepEqual(parseBackgroundWork({toolUseResult:{backgroundTaskId:"shell"}}),[{id:"shell",state:"running"}]);
assert.deepEqual(parseBackgroundWork({toolUseResult:{agentId:"agent",status:"async_launched"}}),[{id:"agent",state:"running"}]);
assert.deepEqual(parseBackgroundWork({message:{content:"<task-notification><task-id>shell</task-id><status>completed</status></task-notification>"}}),[{id:"shell",state:"completed"}]);
assert.deepEqual(parseBackgroundWork({attachment:{prompt:"<task-notification><task-id>agent</task-id><status>killed</status></task-notification>"}}),[{id:"agent",state:"killed"}]);
assert.deepEqual(parseBackgroundWork({toolUseResult:{agentId:"agent",status:"opaque"}}),[]);
assert.deepEqual(parseBackgroundWork({toolUseResult:{backgroundTaskId:"../other"}}),[]);
console.log("6 passed, 0 failed");
