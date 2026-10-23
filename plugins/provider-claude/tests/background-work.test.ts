import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import {parseBackgroundWork} from "../src/backgroundWork";
assert.deepEqual(parseBackgroundWork({toolUseResult:{backgroundTaskId:"shell"}}),[{id:"shell",state:"running"}]);
assert.deepEqual(parseBackgroundWork({toolUseResult:{agentId:"agent",status:"async_launched"}}),[{id:"agent",state:"running"}]);
assert.deepEqual(parseBackgroundWork({message:{content:"<task-notification><task-id>shell</task-id><status>completed</status></task-notification>"}}),[{id:"shell",state:"completed"}]);
assert.deepEqual(parseBackgroundWork({attachment:{prompt:"<task-notification><task-id>agent</task-id><status>killed</status></task-notification>"}}),[{id:"agent",state:"killed"}]);
assert.deepEqual(parseBackgroundWork({toolUseResult:{agentId:"agent",status:"opaque"}}),[]);
assert.deepEqual(parseBackgroundWork({toolUseResult:{backgroundTaskId:"../other"}}),[]);
for (const row of [{type:"summary"}, {type:"system",subtype:"compact_boundary"}, {type:"user",isCompactSummary:true}, {type:"system",subtype:"init"}, {type:"system",subtype:"resume"}]) {
  assert.deepEqual(parseBackgroundWork(row), [{id:"",state:"session_boundary"}]);
}
assert.deepEqual(parseBackgroundWork({type:"user",subtype:"compact_boundary"}), []);
const rows = readFileSync(new URL("./fixtures/background-work.jsonl", import.meta.url), "utf8").trim().split("\n").map(line => JSON.parse(line) as unknown);
assert.deepEqual(rows.flatMap(parseBackgroundWork), [
  {id:"dropped",state:"running"}, {id:"",state:"session_boundary"},
  {id:"finished",state:"running"}, {id:"finished",state:"completed"}, {id:"live",state:"running"},
]);
console.log("13 passed, 0 failed");
