import assert from "node:assert/strict";
import fs from "node:fs";
import type {JsonValue, Operation, RuntimeContext, RuntimeFrame, RuntimePlan, RuntimeStep} from "@codeterm/plugin-sdk";
import {runtimePlan,runtimeResponse,runtimeReduce} from "../src/runtime";
import {v2History} from "../src/runtimeV2Projection";
import {v2AuthEvents, v2LoginEvents} from "../src/runtimeV2Auth";
import {binding} from "../src/runtimeState";

const fixture=JSON.parse(fs.readFileSync(new URL("../../../tests/fixtures/runtime-http-sse.json",import.meta.url),"utf8")) as {
  context:RuntimeContext;health:JsonValue;providers:JsonValue;integrations:JsonValue;session:JsonValue;receipt:JsonValue;history:{data:unknown[]};frames:RuntimeFrame[];interrupt:JsonValue;capability:JsonValue;
  credentialFrame:RuntimeFrame;credentialCases:{name:string;createdOffset:number;nowOffset:number;expected:number;directory?:string;integration?:string;newEpoch?:boolean}[];
};
const copy=<T>(value:T):T=>JSON.parse(JSON.stringify(value));
const call=(plan:RuntimePlan)=>{assert.equal(plan.action.kind,"request");if(plan.action.kind !== "request") throw new Error("request required");return plan.action.call;};
const response=(ctx:RuntimeContext,op:Operation,plan:RuntimePlan,body:unknown)=>runtimeResponse(ctx,op,plan.state,{kind:"success",value:{body,headers:{}} as JsonValue});
const continued=(step:RuntimeStep)=>{assert.equal(step.kind,"continue");if(step.kind !== "continue") throw new Error("continuation required");return step.plan;};
function initialized(tab:string, model="fixture/fixture") {
  const ctx={...copy(fixture.context),tabId:tab,settings:{model}}, op:Operation={kind:"initialize"};
  let plan=runtimePlan(ctx,op);
  plan=continued(response(ctx,op,plan,fixture.health));assert.equal(call(plan).method,"v2.provider.list");
  plan=continued(response(ctx,op,plan,fixture.providers));assert.equal(call(plan).method,"v2.integration.list");
  const step=response(ctx,op,plan,fixture.integrations);assert.equal(step.kind,"complete");
  return ctx;
}
function loaded(tab:string) {
  const ctx=initialized(tab), op:Operation={kind:"resumeThread",threadId:ctx.threadId!,instructions:"opaque instructions",settings:ctx.settings};
  let plan=runtimePlan(ctx,op);
  plan=continued(response(ctx,op,plan,fixture.session));assert.equal(call(plan).method,"v2.session.model");
  plan=continued(response(ctx,op,plan,null));assert.equal(call(plan).method,"v2.session.instructions");
  assert.equal(response(ctx,op,plan,null).kind,"complete");return ctx;
}
function prepared(tab:string) {
  const ctx=loaded(tab), op:Operation={kind:"submitTurn",threadId:ctx.threadId!,sendId:"send-owned",input:copy(ctx.claimedSends[0].input),settings:ctx.settings};
  const plan=runtimePlan(ctx,op), id=String((call(plan).params as Record<string,JsonValue>).body && ((call(plan).params as Record<string,JsonValue>).body as Record<string,JsonValue>).id);
  const frames=JSON.parse(JSON.stringify(fixture.frames).replaceAll("@message",id)) as RuntimeFrame[];
  ctx.streamWindow={frameSequences:frames.map((_,i)=>101+i),turnRequests:[{sendId:op.sendId,turnId:id,afterSequence:100,written:true}]};
  const receipt=JSON.parse(JSON.stringify(fixture.receipt).replaceAll("@message",id)) as JsonValue;
  return {ctx,op,plan,id,frames,receipt};
}
let count=0;
function check(name:string, fn:()=>void){fn();count++;console.log(`  ✓ OpenCode v2 ${name}`);}
check("selects capabilities from authenticated shape, independent of version text",()=>{
  const manifest=JSON.parse(fs.readFileSync(new URL("../plugin.json",import.meta.url),"utf8"));
  assert.deepEqual(fixture.capability,manifest.capabilities.agentRuntime);
  const ctx={...copy(fixture.context),tabId:"v2-capability"},op:Operation={kind:"initialize"};
  const plan=runtimePlan(ctx,op);
  assert.equal(call(continued(response(ctx,op,plan,{...fixture.health as object,version:"opaque-build"}))).method,"v2.provider.list");
  const unknown={...ctx,tabId:"unknown-capability"};
  assert.equal(response(unknown,op,runtimePlan(unknown,op),{version:"2.3.0"}).kind,"refused");
  const legacy={...ctx,tabId:"v1-capability"};
  assert.equal(call(continued(response(legacy,op,runtimePlan(legacy,op),{version:"opaque-v1",healthy:true}))).method,"provider.list");
});
check("model selection on resume updates the same session before loading it",()=>{
  const manifest=JSON.parse(fs.readFileSync(new URL("../plugin.json",import.meta.url),"utf8"));
  assert.equal(manifest.commands.modelSwitch,"restart_resume");
  assert.ok(manifest.defaultModel);
  const ctx=initialized("v2-model-selection","opencode/selected"), op:Operation={kind:"resumeThread",threadId:ctx.threadId!,instructions:"opaque model selection",settings:ctx.settings};
  let plan=runtimePlan(ctx,op);
  assert.equal(call(plan).method,"v2.session.get");
  plan=continued(response(ctx,op,plan,fixture.session));
  assert.equal(call(plan).method,"v2.session.model");
  assert.deepEqual(call(plan).params,{path:{sessionID:ctx.threadId},body:{model:{providerID:"opencode",id:"selected"}}});
  plan=continued(response(ctx,op,plan,null));
  const step=response(ctx,op,plan,null);
  assert.equal(step.kind,"complete");
  if(step.kind === "complete") assert.deepEqual(step.result,{kind:"threadLoaded",threadId:ctx.threadId,settings:ctx.settings});
});
check("events before and during restarted initialization preserve the v2 mapper",()=>{
  for (const epoch of [2,3]) {
    const ctx={...copy(fixture.context),tabId:"v2-restarted-events",epoch,bindingNonce:`restart-${epoch}`,settings:{model:"opencode/selected"}};
    const initialize:Operation={kind:"initialize"};
    let plan=runtimePlan(ctx,initialize), raw:JsonValue=null;
    for (const body of [fixture.health,fixture.providers,fixture.integrations]) {
      const reduction=runtimeReduce(ctx,raw,fixture.frames.slice(0,1));
      if (body === fixture.health) assert.deepEqual(reduction,{state:null,events:[]});
      raw=reduction.state;
      const step=response(ctx,initialize,plan,body);
      if (step.kind === "continue") plan=step.plan;
      else assert.equal(step.kind,"complete");
    }
    const resume:Operation={kind:"resumeThread",threadId:ctx.threadId!,instructions:"opaque restart",settings:ctx.settings};
    plan=runtimePlan(ctx,resume);
    for (const [method,body] of [["v2.session.get",fixture.session],["v2.session.model",null],["v2.session.instructions",null]] as const) {
      assert.equal(call(plan).method,method);
      raw=runtimeReduce(ctx,raw,fixture.frames.slice(0,1)).state;
      const step=response(ctx,resume,plan,body);
      if (step.kind === "continue") plan=step.plan;
      else {assert.equal(step.kind,"complete");if(step.kind === "complete") assert.deepEqual(step.result,{kind:"threadLoaded",threadId:ctx.threadId,settings:ctx.settings});}
    }
  }
});
check("recorded stream maps delivery, tool lifecycle, text, forms and usage",()=>{
  const {ctx,frames}=prepared("v2-stream"), reduced=runtimeReduce(ctx,null,frames), events=reduced.events;
  assert.equal(events.filter(e=>e.kind === "sendEvidence").length,1);
  assert.equal(events.filter(e=>e.kind === "usage").length,2);
  assert.equal(events.filter(e=>e.kind === "turnState" && e.state === "idle").length,1);
  const items=events.filter(e=>e.kind === "chatItem" || e.kind === "chatItemPatch");
  assert.equal(items.findLast(e=>e.item.toolName === "bash")?.item.toolStatus,"completed");
  assert.match(items.findLast(e=>e.item.toolName === "bash")?.item.toolResult ?? "",/fixture-tool/);
  assert.equal(items.findLast(e=>e.item.type === "assistant")?.item.content,"Fixture tool turn complete.");
  assert.equal(events.filter(e=>e.kind === "prompt").length,2);
  const history=v2History(fixture.history.data);
  for (const final of items.filter(e=>e.item.type !== "user")) if (final.item.toolStatus === "completed" || final.item.content === "Fixture tool turn complete.") assert.ok(history.some(row=>row.id === final.item.id));
  assert.equal(runtimeReduce(ctx,reduced.state,frames).events.length,0);
});
check("durable receipts acknowledge input while late stream events prove delivery",()=>{
  const {ctx,op,plan,receipt,frames}=prepared("v2-receipt"), step=response(ctx,op,plan,receipt);
  assert.equal(step.kind,"complete");if(step.kind !== "complete") throw new Error("complete required");
  assert.equal(step.events?.filter(e=>e.kind === "sendEvidence").length,undefined);
  assert.equal(runtimeReduce(ctx,null,frames).events.filter(e=>e.kind === "sendEvidence").length,1);
  assert.throws(()=>runtimePlan(ctx,op));
  const bad=prepared("v2-receipt-foreign");
  assert.equal(response(bad.ctx,bad.op,bad.plan,JSON.parse(JSON.stringify(bad.receipt).replaceAll("ses_owned","ses_foreign"))).kind,"refused");
});
check("foreign, prewrite, stale launch and unwritten streams cannot prove delivery",()=>{
  for (const kind of ["foreign","prewrite","unwritten","wrong-directory"]) {
    const {ctx,frames}=prepared(`v2-${kind}`);
    if(kind === "foreign") for(const f of frames) f.params=JSON.parse(JSON.stringify(f.params).replaceAll("ses_owned","ses_foreign"));
    if(kind === "wrong-directory") for(const f of frames) f.params=JSON.parse(JSON.stringify(f.params).replaceAll("/fixture","/foreign"));
    if(kind === "prewrite") ctx.streamWindow!.frameSequences=frames.map((_,i)=>i);
    if(kind === "unwritten") ctx.streamWindow!.turnRequests![0].written=false;
    assert.equal(runtimeReduce(ctx,null,frames).events.filter(e=>e.kind === "sendEvidence").length,0);
  }
});
check("permission and form responses retain exact request identities",()=>{
  const ctx=loaded("v2-answers");
  for(const frame of fixture.frames.filter(f=>f.kind === "request")) {
    if(frame.kind !== "request") throw new Error("request required");
    ctx.pendingRequest={requestId:frame.requestId,method:frame.method,params:frame.params};
    const op:Operation={kind:"answer",requestId:frame.requestId,answer:{kind:"choice",values:[frame.method === "permission.asked" ? "once" : "0:1"]}};
    const plan=runtimePlan(ctx,op);assert.equal(plan.action.kind,"reply");
    if(plan.action.kind === "reply") assert.deepEqual(plan.action.result,{kind:"success",value:frame.method === "permission.asked" ? {decision:"once"} : {answer:{choice:"beta"}}});
    assert.throws(()=>runtimePlan(ctx,{...op,requestId:"foreign"}));
  }
});
check("opaque history cursors and idle interrupt acknowledgements stay honest",()=>{
  const ctx=loaded("v2-history"), op:Operation={kind:"readHistory",threadId:ctx.threadId!,order:"desc",cursor:"opaque:page/+="}, plan=runtimePlan(ctx,op);
  assert.deepEqual((call(plan).params as Record<string,JsonValue>).query,{limit:"40",cursor:"opaque:page/+="});
  const step=response(ctx,op,plan,{...fixture.history,cursor:{next:"opaque-next"}});assert.equal(step.kind,"complete");
  if(step.kind === "complete" && step.result.kind === "historyPage") assert.equal(step.result.nextCursor,"opaque-next");
  const stop:Operation={kind:"interrupt",threadId:ctx.threadId!,activeTurnId:"turn"};
  assert.equal((response(ctx,stop,runtimePlan(ctx,stop),fixture.interrupt) as Extract<RuntimeStep,{kind:"complete"}>).events?.length,0);
  assert.throws(()=>runtimePlan(ctx,{kind:"submitTurn",threadId:ctx.threadId!,sendId:"different-model",input:ctx.claimedSends[0].input,settings:{model:"another/model"}}));
});
check("recorded OAuth methods preserve hidden defaults and exact host challenge",()=>{
  const ctx=initialized("v2-oauth","opencode/example"), op:Operation={kind:"control",threadId:ctx.threadId,action:"login",value:{action:"start",methodId:"v2:opencode:device",loginId:"host-login"}};
  let plan=runtimePlan(ctx,op);plan=continued(response(ctx,op,plan,fixture.integrations));
  assert.equal(call(plan).method,"v2.integration.authorize");
  assert.deepEqual((call(plan).params as Record<string,JsonValue>).body,{methodID:"device",answer:{server:"https://opencode.ai/console"}});
  const step=response(ctx,op,plan,{data:{attemptID:"attempt-owned",url:"https://example.invalid/sign-in",instructions:"Opaque instructions",mode:"auto",time:{created:100,expires:2000000000000}}});
  assert.equal(step.kind,"complete");
  const complete={...op,value:{action:"complete",methodId:"v2:opencode:device",loginId:"host-login"}} as Operation;
  const status=runtimePlan(ctx,complete);assert.equal(call(status).method,"v2.integration.status");
  const pending=response(ctx,complete,status,{data:{status:"pending"}});
  assert.equal(pending.kind,"complete");
  if(pending.kind === "complete" && step.kind === "complete") assert.deepEqual(pending.events,step.events);
  assert.throws(()=>runtimePlan(ctx,{...op,value:{action:"complete",methodId:"v2:opencode:device",loginId:"foreign"}}));
  assert.ok(!JSON.stringify(status).includes("https://example.invalid"));
  const expired=copy(fixture.integrations) as {data:Record<string,unknown>[]};
  expired.data.find(entry=>entry.id === "opencode")!.connections=[{type:"credential",id:"expired-fixture",label:"Expired",method:"oauth",status:{status:"needs_auth",message:"Sign in again"}}];
  assert.equal(v2AuthEvents(ctx,expired)[1].kind,"auth");
  assert.equal((v2AuthEvents(ctx,expired)[1] as Extract<ReturnType<typeof v2AuthEvents>[number],{kind:"auth"}>).state,"signedOut");
});
check("recorded credential events complete only a fresh active matching challenge",()=>{
  for(const test of fixture.credentialCases) {
    const ctx=initialized(`credential-${test.name}`), frame=copy(fixture.credentialFrame), event=frame.params as Record<string,JsonValue>, created=Number(event.created), expires=created+100;
    binding(ctx).login={loginId:"host-credential-challenge",methodId:"v2:fixture:device",integration:"fixture",attempt:"attempt-owned",created:created+test.createdOffset,expires,challenge:{status:"pending",method:"browser",loginId:"host-credential-challenge",url:"https://example.invalid/sign-in",expiresAtMs:expires}};
    if(test.directory) event.location={directory:test.directory};
    if(test.integration) (event.data as Record<string,JsonValue>).integrationID=test.integration;
    if(test.newEpoch) ctx.epoch++;
    const events=v2LoginEvents(ctx,[frame],created+test.nowOffset);
    assert.equal(events.length,test.expected,test.name);
    if(test.expected) {
      assert.deepEqual(events,[{kind:"authLogin",login:{status:"completed",loginId:"host-credential-challenge"}}]);
      assert.equal(v2LoginEvents(ctx,[frame],created+test.nowOffset).length,0);
    }
  }
});
console.log(`${count} OpenCode v2 runtime cases passed`);
