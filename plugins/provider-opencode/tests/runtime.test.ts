import assert from "node:assert/strict";
import fs from "node:fs";
import type {JsonValue, Operation, RuntimeContext, RuntimeFrame, RuntimePlan, RuntimeStep} from "@codeterm/plugin-sdk";
import {runtimePlan, runtimeResponse, runtimeReduce} from "../src/runtime";
import {authEvents, methods} from "../src/runtimeAuth";
import {messageIdentity} from "../src/runtimeProtocol";

const fixture=JSON.parse(fs.readFileSync(new URL("./fixtures/runtime-contract.json",import.meta.url),"utf8")) as {
  context:RuntimeContext;frames:RuntimeFrame[];expectedKinds:string[];expectedUsage:unknown;providers:unknown;auth:unknown;question:RuntimeFrame;permission:RuntimeFrame;
};
const legacyErrors=JSON.parse(fs.readFileSync(new URL("./fixtures/legacy-turn-errors.json",import.meta.url),"utf8")) as Array<{
  version:string;event:{type:string;properties:{sessionID:string;error:{name:string;data:{message:string;isRetryable:boolean}}}};
}>;
const copy=<T>(value:T):T=>JSON.parse(JSON.stringify(value));
let count=0;
function check(name:string, run:()=>void) {run();count++;console.log(`  ✓ OpenCode runtime ${name}`);}
function call(plan:RuntimePlan) {assert.equal(plan.action.kind,"request");if (plan.action.kind !== "request") throw new Error("request required");return plan.action.call;}
function response(ctx:RuntimeContext, op:Operation, state:JsonValue, body:unknown, headers={}) {return runtimeResponse(ctx,op,state,{kind:"success",value:{body,headers} as JsonValue});}
function continuation(step:RuntimeStep) {assert.equal(step.kind,"continue");if (step.kind !== "continue") throw new Error("continuation required");return step.plan;}
function loaded(tab:string) {
  const ctx={...copy(fixture.context),tabId:tab};
  const initialize:Operation={kind:"initialize"};
  let handshake=runtimePlan(ctx,initialize);
  for (const body of [{healthy:true,version:"fixture"},fixture.providers,fixture.auth]) {
    const step=response(ctx,initialize,handshake.state,body);
    if (step.kind === "continue") handshake=step.plan;
    else assert.equal(step.kind,"complete");
  }
  const op:Operation={kind:"resumeThread",threadId:ctx.threadId!,instructions:"opaque system instructions",settings:ctx.settings};
  const plan=runtimePlan(ctx,op);
  const step=response(ctx,op,plan.state,{id:ctx.threadId,directory:ctx.cwd});
  assert.equal(step.kind,"complete");return ctx;
}
function prepared(tab:string, text="opaque prompt") {
  const ctx=loaded(tab), input={text,attachments:[]};
  ctx.claimedSends[0].input=input;
  const op:Operation={kind:"submitTurn",threadId:ctx.threadId!,sendId:"send-owned",input,settings:ctx.settings};
  const plan=runtimePlan(ctx,op), wire=call(plan), message=String((wire.params as Record<string,JsonValue>).body && ((wire.params as Record<string,JsonValue>).body as Record<string,JsonValue>).messageID);
  const frames=JSON.parse(JSON.stringify(fixture.frames).replaceAll("@message",message)
    .replaceAll(JSON.stringify("opaque prompt"),()=>JSON.stringify(text))) as RuntimeFrame[];
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
check("preserves exact large echoed parts through assistant output and idle",()=>{
  const text="opaque λ\n".repeat(32_000);
  assert.ok(Buffer.byteLength(text) > 256*1024);
  const {ctx,frames,message}=prepared("fixture-large-echo",text);
  const {events}=runtimeReduce(ctx,null,frames);
  const evidence=events.filter(event=>event.kind === "sendEvidence");
  assert.deepEqual(evidence,[{kind:"sendEvidence",threadId:ctx.threadId,sendId:"send-owned",turnId:message,itemId:message}]);
  const user=events.find(event=>event.kind === "chatItem" && event.item.type === "user");
  assert.ok(user?.kind === "chatItem");
  assert.equal(user.item.content,text);
  assert.ok(events.some(event=>event.kind === "chatItem" && event.item.type === "assistant"));
  assert.equal(events.filter(event=>event.kind === "turnState" && event.state === "idle").length,1);
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
check("legacy upstream failures reach the provider-error alert and settle the exact turn once",()=>{
  assert.equal(legacyErrors.length,2);
  for (const {version,event} of legacyErrors) {
    const {ctx,message:turnId}=prepared(`upstream-error-${version}`);
    const upstream=event.properties.error.data.message;
    const error:RuntimeFrame={kind:"notification",method:"http/event",params:event};
    const foreign:RuntimeFrame={...error,params:{...event,properties:{...event.properties,sessionID:"ses_foreign"}}};
    const idle:RuntimeFrame={kind:"notification",method:"http/event",params:{type:"session.idle",properties:{sessionID:ctx.threadId!}}};
    const failure=runtimeReduce(ctx,null,[foreign,error,idle,error]);
    assert.deepEqual(failure.events,[
      {kind:"notice",threadId:ctx.threadId,message:upstream},
      {kind:"chatItem",threadId:ctx.threadId,item:{id:`${turnId}:error`,seq:1,type:"system",systemSubtype:"api_error",toolFlowId:turnId,content:upstream}},
      {kind:"turnState",threadId:ctx.threadId,turnId,state:"idle",detail:"failed"},
    ]);
  }
});
check("legacy interruption settles without a provider-error alert",()=>{
  const {ctx,message:turnId}=prepared("legacy-interrupted");
  const frame:RuntimeFrame={kind:"notification",method:"http/event",params:{type:"session.error",properties:{sessionID:ctx.threadId!,error:{name:"MessageAbortedError"}}}};
  assert.deepEqual(runtimeReduce(ctx,null,[frame]).events,[
    {kind:"turnState",threadId:ctx.threadId,turnId,state:"idle",detail:"interrupted"},
  ]);
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
check("legacy startup requests a compact provider list and keeps selected provider auth",()=>{
  const ctx={...copy(fixture.context),tabId:"large-provider-list",settings:{model:"opencode/beam-free"}};
  const op:Operation={kind:"initialize"};
  const health=runtimePlan(ctx,op);
  const catalogue=continuation(response(ctx,op,health.state,{healthy:true,version:"1.17.16"}));
  const wire=call(catalogue);
  assert.equal(wire.method,"provider.list");
  assert.deepEqual(wire.params,{responseFields:{all:["id","name"],connected:[]}});
  const raw={all:[{id:"opencode",name:"OpenCode",models:{"beam-free":{description:"x".repeat(5*1024*1024)}}}],connected:["opencode"]};
  assert.ok(Buffer.byteLength(JSON.stringify(raw)) > 4*1024*1024);
  const projected={all:raw.all.map(({id,name})=>({id,name})),connected:raw.connected};
  const auth=continuation(response(ctx,op,catalogue.state,projected));
  assert.equal(call(auth).method,"provider.auth");
  assert.ok(Buffer.byteLength(JSON.stringify(auth.state)) < 1024);
  const initialized=response(ctx,op,auth.state,{});
  assert.equal(initialized.kind,"complete");
  if (initialized.kind === "complete") {
    assert.equal(initialized.result.kind,"initialized");
    assert.deepEqual(initialized.events?.find(event=>event.kind === "auth"),{kind:"auth",state:"signedIn"});
  }
  assert.deepEqual(runtimeResponse(ctx,op,catalogue.state,{
    kind:"error",error:{code:-32010,message:"HTTP response exceeded its byte limit",data:{kind:"response_too_large"}},
  }),{kind:"refused",reason:{code:"size_limit",message:"HTTP response exceeded its byte limit"},beforeAcceptance:false});
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
  const cursor=Buffer.from(JSON.stringify({id:"msg_old",time:10})).toString("base64url");
  const step=response(ctx,op,plan.state,body,{"x-next-cursor":cursor});
  assert.equal(step.kind,"complete");if (step.kind === "complete" && step.result.kind === "historyPage") {assert.equal(step.result.nextCursor,cursor);assert.equal(step.result.items[0].id,"msg_old");}
  const older={...op,cursor}, olderPlan=runtimePlan(ctx,older);
  assert.deepEqual(olderPlan.action,{kind:"request",call:{method:"session.messages",params:{path:{sessionID:ctx.threadId},query:{limit:"40",before:cursor}}}});
  assert.equal(response(ctx,older,olderPlan.state,body,{"x-next-cursor":cursor}).kind,"refused");
  assert.throws(()=>response(ctx,op,plan.state,JSON.parse(JSON.stringify(body).replaceAll("ses_owned","ses_foreign"))));
});
console.log(`${count} OpenCode runtime cases passed`);
