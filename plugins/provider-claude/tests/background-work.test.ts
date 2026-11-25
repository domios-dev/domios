import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import vm from "node:vm";
import type { AgentProvider } from "@codeterm/plugin-sdk";

const sandbox = { module: { exports: {} as { default: AgentProvider } }, exports: {}, host: {} };
vm.runInNewContext(readFileSync(new URL("../plugin.js", import.meta.url), "utf8"), sandbox);
const parse = sandbox.module.exports.default.parseBackgroundWork!;
const parseBackgroundWork = (value: unknown): ReturnType<typeof parse> => JSON.parse(JSON.stringify(parse(value)));
assert.deepEqual(parseBackgroundWork({toolUseResult:{backgroundTaskId:"shell"}}),[{id:"shell",state:"running",kind:"shell"}]);
assert.deepEqual(parseBackgroundWork({toolUseResult:{agentId:"agent",status:"async_launched"}}),[{id:"agent",state:"running",kind:"subagent"}]);
assert.deepEqual(parseBackgroundWork({type:"user",isMeta:true,message:{content:"<task-notification><task-id>shell</task-id><status>completed</status></task-notification>"}}),[{id:"shell",state:"completed"}]);
assert.deepEqual(parseBackgroundWork({type:"attachment",attachment:{type:"queued_command",commandMode:"task-notification",prompt:"<task-notification><task-id>agent</task-id><status>killed</status></task-notification>"}}),[{id:"agent",state:"killed"}]);
assert.deepEqual(parseBackgroundWork({toolUseResult:{agentId:"agent",status:"opaque"}}),[]);
assert.deepEqual(parseBackgroundWork({toolUseResult:{backgroundTaskId:"../other"}}),[]);
for (const row of [{type:"summary"}, {type:"system",subtype:"compact_boundary"}, {type:"user",isCompactSummary:true}, {type:"system",subtype:"init"}, {type:"system",subtype:"resume"}]) {
  assert.deepEqual(parseBackgroundWork(row), [{id:"",state:"session_boundary"}]);
}
assert.deepEqual(parseBackgroundWork({type:"user",subtype:"compact_boundary"}), []);
const rows = readFileSync(new URL("./fixtures/background-work.jsonl", import.meta.url), "utf8").trim().split("\n").map(line => JSON.parse(line) as unknown);
assert.deepEqual(rows.flatMap(parseBackgroundWork), [
  {id:"dropped",state:"running",kind:"shell"}, {id:"",state:"session_boundary"},
  {id:"finished",state:"running",kind:"shell"}, {id:"finished",state:"failed"}, {id:"live",state:"running",kind:"shell"},
  {id:"opaque-stopped",state:"running",kind:"shell",output_path:String.raw`C:\Fixture\Temp\claude\opaque-project\opaque-own\tasks\opaque-stopped.output`}, {id:"opaque-stopped",state:"killed",kind:"shell"},
]);
const stop = rows[rows.length - 1] as Record<string, unknown>;
const result = stop.toolUseResult as Record<string, unknown>;
for (const value of [
  {...stop, type:"assistant"},
  {...stop, toolUseResult:{...result,task_type:"opaque"}},
  {...stop, toolUseResult:{...result,task_id:"../foreign"}},
  {...stop, toolUseResult:{...result,command:null}},
  {...stop, message:{content:[{type:"tool_result",is_error:true}]}},
  {type:"user", toolUseResult:{message:"opaque-b",task_id:"opaque-stopped"}},
]) assert.deepEqual(parseBackgroundWork(value), []);
const launch = rows.at(-2) as Record<string, unknown>;
assert.deepEqual(parseBackgroundWork({ ...launch, sessionId: "foreign" }), [{ id: "opaque-stopped", state: "running", kind: "shell" }]);
assert.deepEqual(parseBackgroundWork({ type: "user", message: { content: "<task-notification><task-id>live</task-id><status>completed</status></task-notification>" } }), []);
console.log("21 passed, 0 failed");
