import assert from "node:assert/strict";
import fs from "node:fs";
import type {JsonValue, Operation, RuntimeContext, RuntimeFrame, RuntimePlan, RuntimeStep} from "@codeterm/plugin-sdk";
import {runtimePlan, runtimeResponse, runtimeReduce} from "../src/runtime";
import {authEvents, methods} from "../src/runtimeAuth";
import {messageIdentity} from "../src/runtimeProtocol";

const fixture=JSON.parse(fs.readFileSync(new URL("./fixtures/runtime-contract.json",import.meta.url),"utf8")) as {
  context:RuntimeContext;frames:RuntimeFrame[];expectedKinds:string[];expectedUsage:unknown;providers:unknown;auth:unknown;question:RuntimeFrame;permission:RuntimeFrame;
};
const copy=<T>(value:T):T=>JSON.parse(JSON.stringify(value));
let count=0;
function check(name:string, run:()=>void) {run();count++;console.log(`  ✓ OpenCode runtime ${name}`);}
function call(plan:RuntimePlan) {assert.equal(plan.action.kind,"request");if (plan.action.kind !== "request") throw new Error("request required");return plan.action.call;}
function response(ctx:RuntimeContext, op:Operation, state:JsonValue, body:unknown, headers={}) {return runtimeResponse(ctx,op,state,{kind:"success",value:{body,headers} as JsonValue});}
function continuation(step:RuntimeStep) {assert.equal(step.kind,"continue");if (step.kind !== "continue") throw new Error("continuation required");return step.plan;}
function loaded(tab:string) {
  const ctx={...copy(fixture.context),tabId:tab};
  const op:Operation={kind:"resumeThread",threadId:ctx.threadId!,instructions:"opaque system instructions",settings:ctx.settings};
  const plan=runtimePlan(ctx,op);
  const step=response(ctx,op,plan.state,{id:ctx.threadId,directory:ctx.cwd});
  assert.equal(step.kind,"complete");return ctx;
}
function prepared(tab:string) {
  const ctx=loaded(tab), op:Operation={kind:"submitTurn",threadId:ctx.threadId!,sendId:"send-owned",input:{text:"opaque prompt",attachments:[]},settings:ctx.settings};
  const plan=runtimePlan(ctx,op), wire=call(plan), message=String((wire.params as Record<string,JsonValue>).body && ((wire.params as Record<string,JsonValue>).body as Record<string,JsonValue>).messageID);
  const frames=JSON.parse(JSON.stringify(fixture.frames).replaceAll("@message",message)) as RuntimeFrame[];
  ctx.streamWindow={frameSequences:frames.map((_,index)=>101+index),turnRequests:[{sendId:"send-owned",turnId:message,afterSequence:100,written:true}]};
  return {ctx,op,plan,wire,message,frames};
}
check("shared fixture projects exact user evidence, stream, tool lifecycle and usage",()=>{
  const {ctx,frames,message}=prepared("fixture-stream");
  const reduced=runtimeReduce(ctx,null,frames);
  assert.deepEqual(reduced.events.map(event=>event.kind),fixture.expectedKinds);
  const evidence=reduced.events.find(event=>event.kind === "sendEvidence");
  assert.deepEqual(evidence,{kind:"sendEvidence",threadId:"ses_owned",sendId:"send-owned",turnId:message,itemId:message});
  assert.deepEqual(reduced.events.find(event=>event.kind === "usage")?.usage,fixture.expectedUsage);
  const tool=reduced.events.filter(event=>event.kind === "chatItem" || event.kind === "chatItemPatch").findLast(event=>event.item.id === "prt_tool");
  assert.equal(tool?.item.toolStatus,"completed");assert.equal(tool?.item.toolResult,"opaque");
  assert.equal(reduced.events.filter(event=>event.kind === "turnState" && event.state === "idle").length,1);
  JSON.stringify(reduced,(_key,value)=>{assert.notEqual(value,undefined);return value;});
  const again=runtimeReduce(ctx,reduced.state,frames);
  assert.equal(again.events.filter(event=>event.kind === "sendEvidence").length,0);
});
check("unknown, foreign and prewrite user events never confirm sends",()=>{
  for (const kind of ["foreign","prewrite","unwritten"]) {
    const {ctx,frames}=prepared(`fixture-${kind}`);
    if (kind === "foreign") for (const frame of frames) frame.params=JSON.parse(JSON.stringify(frame.params).replaceAll("ses_owned","ses_foreign"));
    if (kind === "prewrite") ctx.streamWindow!.frameSequences=frames.map((_,index)=>index+1);
    if (kind === "unwritten") ctx.streamWindow!.turnRequests![0].written=false;
    const events=runtimeReduce(ctx,null,frames).events;
    assert.equal(events.filter(event=>event.kind === "sendEvidence").length,0);
    assert.equal(events.filter(event=>event.kind === "turnState" && event.state === "working").length,0);
  }
});
check("prompt identity never authorizes replay and acknowledgment is not delivery evidence",()=>{
  const {ctx,op,plan,message}=prepared("fixture-no-replay");
  assert.equal(call(plan).method,"session.prompt");
  assert.equal(call(plan).target?.turnId,message);
  const step=response(ctx,op,plan.state,null);
  assert.equal(step.kind,"complete");
  assert.equal(step.kind === "complete" && step.events?.some(event=>event.kind === "sendEvidence"),undefined);
  assert.throws(()=>runtimePlan(ctx,op));
  assert.equal(messageIdentity("send",123,1),messageIdentity("send",123,1));
});
check("discovered auth methods preserve supported inputs and explicit unsupported fields",()=>{
  const ctx=loaded("fixture-auth-methods"), discovered=methods(ctx,fixture.providers,fixture.auth);
  assert.equal(discovered[0].input,"none");assert.equal(discovered[0].available,true);
  assert.equal(discovered[1].available,false);assert.ok(discovered[1].unavailableReason);
  assert.equal(discovered.at(-1)?.input,"apiKey");
  assert.equal(authEvents(ctx,fixture.providers,fixture.auth)[0].kind,"authentication");
});
check("API key stays out of continuation state and completes only after connected verification",()=>{
  const ctx=loaded("fixture-api-key"), op:Operation={kind:"control",threadId:ctx.threadId,action:"login",value:{action:"start",methodId:"api:paid",loginId:"host-key-challenge",apiKey:"opaque-key-fixture"}};
  let plan=runtimePlan(ctx,op);
  plan=continuation(response(ctx,op,plan.state,fixture.providers));
  plan=continuation(response(ctx,op,plan.state,fixture.auth));
  assert.equal(call(plan).method,"provider.key");assert.ok(!JSON.stringify(plan.state).includes("opaque-key-fixture"));
  plan=continuation(response(ctx,op,plan.state,true));
  assert.equal(call(plan).method,"provider.list");
  const step=response(ctx,op,plan.state,{...(fixture.providers as object),connected:["paid"]});
  assert.equal(step.kind,"complete");
  if (step.kind === "complete") assert.deepEqual(step.events,[{kind:"authLogin",login:{status:"completed",loginId:"host-key-challenge"}}]);
});
check("OAuth publishes its host challenge before a separate code callback",()=>{
  const ctx=loaded("fixture-oauth"), op:Operation={kind:"control",threadId:ctx.threadId,action:"login",value:{action:"start",methodId:"oauth:paid:0:Browser",loginId:"host-browser-challenge"}};
  let plan=runtimePlan(ctx,op);
  plan=continuation(response(ctx,op,plan.state,fixture.providers));
  plan=continuation(response(ctx,op,plan.state,fixture.auth));
  assert.equal(call(plan).method,"provider.authorize");
  const step=response(ctx,op,plan.state,{url:"https://example.invalid/login",method:"code",instructions:"Opaque challenge instructions"});
  assert.equal(step.kind,"complete");
  if (step.kind === "complete") assert.deepEqual(step.events,[{kind:"authLogin",login:{status:"pending",method:"browser",loginId:"host-browser-challenge",url:"https://example.invalid/login",codeInput:true,instructions:"Opaque challenge instructions"}}]);
  const callback=runtimePlan(ctx,{...op,value:{action:"code",methodId:"oauth:paid:0:Browser",loginId:"host-browser-challenge",code:"opaque-code-fixture"}});
  assert.equal(call(callback).method,"provider.callback");assert.ok(!JSON.stringify(callback.state).includes("opaque-code-fixture"));
  assert.throws(()=>runtimePlan(ctx,{...op,value:{action:"start",methodId:"api:foreign",loginId:"foreign"}}));
});
check("permission and question answers use only their exact owned request",()=>{
  const ctx=loaded("fixture-requests");
  for (const frame of [fixture.question,fixture.permission]) {
    assert.equal(frame.kind,"request");if (frame.kind !== "request") throw new Error("request fixture required");
    const reduced=runtimeReduce(ctx,null,[frame]);assert.equal(reduced.events[0].kind,"prompt");
    ctx.pendingRequest={requestId:frame.requestId,method:frame.method,params:frame.params};
    const op:Operation={kind:"answer",requestId:frame.requestId,answer:{kind:"choice",values:[frame.method === "question.asked" ? "0:1" : "once"]}};
    const plan=runtimePlan(ctx,op);assert.equal(plan.action.kind,"reply");
    if (plan.action.kind === "reply") assert.deepEqual(plan.action.result,{kind:"success",value:frame.method === "question.asked" ? {answers:[["Beta"]]} : {reply:"once"}});
    assert.throws(()=>runtimePlan(ctx,{...op,requestId:"foreign"}));
  }
});
check("resume and history refuse foreign ownership while preserving page cursor",()=>{
  const ctx=loaded("fixture-history"), resume:Operation={kind:"resumeThread",threadId:ctx.threadId!,instructions:"",settings:ctx.settings};
  assert.throws(()=>response(ctx,resume,runtimePlan(ctx,resume).state,{id:"ses_foreign",directory:ctx.cwd}));
  const op:Operation={kind:"readHistory",threadId:ctx.threadId!,order:"desc"}, plan=runtimePlan(ctx,op);
  const body=[{info:{id:"msg_old",sessionID:ctx.threadId,role:"user",time:{created:10}},parts:[{id:"prt_old",messageID:"msg_old",sessionID:ctx.threadId,type:"text",text:"prior user turn"}]}];
  const step=response(ctx,op,plan.state,body,{"x-next-cursor":"msg_older"});
  assert.equal(step.kind,"complete");if (step.kind === "complete" && step.result.kind === "historyPage") {assert.equal(step.result.nextCursor,"msg_older");assert.equal(step.result.items[0].id,"msg_old");}
  assert.throws(()=>response(ctx,op,plan.state,JSON.parse(JSON.stringify(body).replaceAll("ses_owned","ses_foreign"))));
});
console.log(`${count} OpenCode runtime cases passed`);
