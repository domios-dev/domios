import assert from "node:assert/strict";
import fs from "node:fs";
import vm from "node:vm";
import type { AgentRuntime, RuntimeContext, Operation, RpcReply, RuntimePlan } from "@codeterm/plugin-sdk";

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

{
  const frameLimit = 16 * 1024 * 1024;
  const hookLimit = 4 * 1024 * 1024;
  const images = Array.from({length:9}, (_, index) => ({item:{
    id:`image-${index}`,type:"imageGeneration",status:"completed",savedPath:`/work/image-${index}.png`,
    result:Buffer.alloc(1_770_843,index).toString("base64"),
  }}));
  assert.ok(Buffer.byteLength(JSON.stringify({id:1,result:{data:images}})) > frameLimit);
  const thread = {id:operation.threadId,source:"cli",status:{type:"idle"},turns:[]};
  const call = (plan:RuntimePlan) => {
    if (plan.action.kind !== "request") throw new Error("Expected RPC request");
    return plan.action.call;
  };
  const resumePlan = hooks.runtimePlan(context,operation);
  assert.equal(call(resumePlan).method,"thread/resume");
  assert.equal((call(resumePlan).params as {excludeTurns:boolean}).excludeTurns,true);
  const verify = hooks.runtimeResponse(context,operation,resumePlan.state,{kind:"success",value:{thread}});
  if (verify.kind !== "continue") throw new Error("Expected resume identity check");
  assert.equal(call(verify.plan).method,"thread/read");
  assert.equal((call(verify.plan).params as {includeTurns:boolean}).includeTurns,false);
  const loaded = hooks.runtimeResponse(context,operation,verify.plan.state,{kind:"success",value:{thread}});
  assert.equal(loaded.kind === "complete" && loaded.result.kind === "threadLoaded" && loaded.result.threadId,thread.id);

  let history:Extract<Operation,{kind:"readHistory"}> = {kind:"readHistory",threadId:thread.id,order:"desc"};
  const turnsPlan = hooks.runtimePlan(context,history);
  assert.equal(call(turnsPlan).method,"thread/turns/list");
  assert.equal((call(turnsPlan).params as {itemsView:string}).itemsView,"notLoaded");
  const turns = hooks.runtimeResponse(context,history,turnsPlan.state,{kind:"success",value:{data:[{id:"image-turn",status:"completed"}]}});
  if (turns.kind !== "continue") throw new Error("Expected paged item read");
  let itemsPlan = turns.plan;
  const limits:number[] = [], retained:string[] = [];
  for (let attempts=0; attempts<images.length+5; attempts++) {
    assert.equal(call(itemsPlan).method,"thread/items/list");
    const {limit,cursor} = call(itemsPlan).params as {limit:number;cursor:string|null};
    const start = Number(cursor ?? 0);
    const data = images.slice(start,start+limit);
    const nextCursor = start+data.length < images.length ? String(start+data.length) : null;
    const bytes = Buffer.byteLength(JSON.stringify({id:1,result:{data,nextCursor}}));
    if (bytes > hookLimit) {
      assert.equal(start,0);
      limits.push(limit);
      const retry = hooks.runtimeResponse(context,history,itemsPlan.state,{kind:"error",error:{
        code:-32097,message:"History page exceeds byte budget",data:{historyPageLimit:limit/2},
      }});
      if (retry.kind !== "continue") throw new Error("Expected smaller history page");
      itemsPlan = retry.plan;
      continue;
    }
    assert.equal(limit,1);
    assert.equal(Buffer.from(data[0].item.result,"base64").length,1_770_843);
    const projected = data.map(({item})=>({item:{...item,result:`[image omitted: ${item.result.length} bytes]`}}));
    const page = hooks.runtimeResponse(context,history,itemsPlan.state,{kind:"success",value:{data:projected,nextCursor}});
    if (page.kind !== "complete" || page.result.kind !== "historyPage") throw new Error("Expected history page");
    const image = page.result.items[0].toolRichResult as {path?:string} | undefined;
    assert.equal(image?.path,data[0].item.savedPath);
    retained.push(page.result.items[0].id);
    if (!nextCursor) break;
    history = {...history,cursor:page.result.nextCursor};
    itemsPlan = hooks.runtimePlan(context,history);
  }
  assert.deepEqual(limits,[16,8,4,2]);
  assert.deepEqual(retained,images.map(({item})=>item.id));

  const submit:Operation = {kind:"submitTurn",threadId:thread.id,sendId:"after-resume",input:{text:"Continue",attachments:[]},settings:{}};
  const turnPlan = hooks.runtimePlan({...context,activeTurnId:undefined},submit);
  assert.equal(call(turnPlan).method,"turn/start");
  const accepted = hooks.runtimeResponse(context,submit,turnPlan.state,{kind:"success",value:{turn:{id:"next-turn"}}});
  assert.equal(accepted.kind === "complete" && accepted.result.kind === "turnAccepted" && accepted.result.turnId,"next-turn");
  console.log("runtime resume: nine image items over 16 MiB retained by adaptive paging, then a turn accepted");
}
