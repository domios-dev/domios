import assert from "node:assert/strict";
import fs from "node:fs";
import vm from "node:vm";
import type { AgentRuntime, RuntimeContext, Operation } from "@codeterm/plugin-sdk";

const sandbox = vm.createContext({module:{exports:{}},exports:{}});
sandbox.exports = sandbox.module.exports;
vm.runInContext(fs.readFileSync(new URL("../plugin.js",import.meta.url),"utf8"),sandbox);
const hooks = sandbox.module.exports.default as AgentRuntime;
const fixture = JSON.parse(fs.readFileSync(new URL("./fixtures/runtime/tool-call.json",import.meta.url),"utf8"));
const context = {...fixture.context,activeTurnId:"stale-turn"} as RuntimeContext;
const operation: Operation = {kind:"resumeThread",threadId:context.threadId!,instructions:"",settings:{}};
for (const status of ["idle","active","notLoaded","systemError"]) {
  const result = hooks.runtimeResponse(context,operation,{stage:"verify",settings:{}},{kind:"success",value:{thread:{id:context.threadId!,source:"cli",status:{type:status}}}});
  assert.equal(result.kind,"complete");
  if (result.kind !== "complete") throw new Error("Expected verified resume");
  assert.deepEqual(JSON.parse(JSON.stringify(result.events ?? [])),status === "idle"
    ? [{kind:"turnState",threadId:context.threadId,state:"idle",turnId:"stale-turn"}] : []);
}
const foreign = hooks.runtimeResponse(context,operation,{stage:"verify",settings:{}},{kind:"success",value:{thread:{id:"foreign",source:"cli",status:{type:"idle"}}}});
assert.equal(foreign.kind,"refused");
console.log("runtime resume idle: 4 statuses and foreign-thread refusal passed");
