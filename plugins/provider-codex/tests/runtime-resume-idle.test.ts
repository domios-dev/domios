import assert from "node:assert/strict";
import fs from "node:fs";
import vm from "node:vm";
import type { AgentRuntime, RuntimeContext, Operation, RpcReply } from "@codeterm/plugin-sdk";

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

const missing = JSON.parse(fs.readFileSync(new URL("./fixtures/runtime-missing-rollout.json",import.meta.url),"utf8")) as {threadId:string;reply:Extract<RpcReply,{kind:"error"}>};
const rpcSchema = JSON.parse(fs.readFileSync(new URL("./fixtures/runtime-rpc-error-schema-0.164.0.json",import.meta.url),"utf8"));
assert.deepEqual(rpcSchema.properties.code,{format:"int64",type:"integer"});
assert.equal(rpcSchema.properties.data,true);
const missingContext = {...context,threadId:missing.threadId};
const resume: Operation = {...operation,threadId:missing.threadId};
const classified = hooks.runtimeResponse(missingContext,resume,{stage:"thread/resume"},missing.reply);
assert.equal(classified.kind,"refused");
if (classified.kind !== "refused") throw new Error("Expected missing conversation");
assert.equal(classified.reason.code,"thread_not_found");
for (const error of [
  {...missing.reply.error,code:-32603},
  {...missing.reply.error,data:{opaque:"unrecognized-typed-data"}},
  {...missing.reply.error,message:"opaque provider diagnostic"},
  {...missing.reply.error,message:"no rollout found for thread id foreign-thread"},
]) {
  const step = hooks.runtimeResponse(missingContext,resume,{stage:"thread/resume"},{kind:"error",error});
  assert.equal(step.kind,"refused");
  if (step.kind !== "refused") throw new Error("Expected refusal");
  assert.equal(step.reason.code,"provider_refused");
}
const outsideResume = hooks.runtimeResponse(missingContext,resume,{stage:"verify"},missing.reply);
assert.equal(outsideResume.kind,"refused");
if (outsideResume.kind !== "refused") throw new Error("Expected refusal");
assert.equal(outsideResume.reason.code,"provider_refused");
console.log("missing rollout: exact own-thread error and 5 negative boundaries passed");
