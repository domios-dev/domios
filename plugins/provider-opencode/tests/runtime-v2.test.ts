import assert from "node:assert/strict";
import fs from "node:fs";
import type {JsonValue, Operation, RuntimeContext, RuntimeFrame, RuntimePlan, RuntimeSettings, RuntimeStep} from "@codeterm/plugin-sdk";
import {runtimePlan,runtimeResponse,runtimeReduce} from "../src/runtime";
import {v2History} from "../src/runtimeV2Projection";
import {v2AuthEvents, v2LoginEvents} from "../src/runtimeV2Auth";
import {binding} from "../src/runtimeState";
import {v2Model} from "../src/runtimeV2";

const fixture=JSON.parse(fs.readFileSync(new URL("../../../tests/fixtures/runtime-http-sse.json",import.meta.url),"utf8")) as {
  context:RuntimeContext;health:JsonValue;providers:JsonValue;integrations:JsonValue;session:JsonValue;receipt:JsonValue;history:{data:unknown[]};frames:RuntimeFrame[];interrupt:JsonValue;capability:JsonValue;
  streamAndInterrupt:{frames:RuntimeFrame[];interrupt:JsonValue};
  sameMessageFragments:{frames:RuntimeFrame[];history:{data:unknown[]}};
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
    const page:Operation={kind:"readHistory",threadId:resume.threadId,order:"desc"};
    plan=runtimePlan(ctx,page);
    assert.equal(call(plan).method,"v2.session.messages");
    const firstPage=response(ctx,page,plan,{data:fixture.history.data,cursor:{next:"opaque:next/+="}});
    if (firstPage.kind !== "complete" || firstPage.result.kind !== "historyPage") throw new Error("history page required");
    assert.ok(firstPage.result.items.length);
    const older={...page,cursor:firstPage.result.nextCursor}, olderPlan=runtimePlan(ctx,older);
    assert.deepEqual(call(olderPlan).params,{path:{sessionID:resume.threadId},query:{limit:"40",cursor:"opaque:next/+="}});
    assert.equal(response(ctx,older,olderPlan,{data:[]}).kind,"complete");
    const submit:Operation={kind:"submitTurn",threadId:resume.threadId,sendId:`continue-${epoch}`,input:ctx.claimedSends[0].input,settings:ctx.settings};
    ctx.claimedSends=[{sendId:submit.sendId,threadId:submit.threadId,input:submit.input}];
    plan=runtimePlan(ctx,submit);
    assert.equal(call(plan).method,"v2.session.prompt");
    assert.equal(call(plan).target?.threadId,resume.threadId);
    assert.throws(()=>runtimePlan(ctx,submit),{code:"provider_refused"});
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
  assert.ok(items.filter(e=>e.item.type === "assistant" || e.item.type === "thinking").every(e=>e.kind === "chatItem"));
  assert.equal(events.filter(e=>e.kind === "prompt").length,2);
  const history=v2History(fixture.history.data);
  for (const final of items.filter(e=>e.item.type !== "user")) if (final.item.toolStatus === "completed" || final.item.content === "Fixture tool turn complete.") assert.ok(history.some(row=>row.id === final.item.id));
  assert.equal(runtimeReduce(ctx,reduced.state,frames).events.length,0);
});
check("returns to working only when the active turn's final prompt settles",()=>{
  const {ctx,frames}=prepared("v2-prompt-turn-state");
  ctx.activeTurnId="active-turn";
  const frame=(type:string)=>frames.find(frame=>(frame.params as Record<string,JsonValue>).type === type)!;
  const permission=frame("permission.asked"), form=frame("form.created");
  const asked=runtimeReduce(ctx,null,[frame("session.execution.started"),permission,form]);
  assert.deepEqual(asked.events.filter(event=>event.kind === "turnState").map(event=>event.state),["working","needs_input","needs_input"]);

  const first=runtimeReduce(ctx,asked.state,[frame("permission.replied")]);
  assert.deepEqual(first.events.map(event=>event.kind),["promptResolved"]);
  const last=runtimeReduce(ctx,first.state,[frame("form.replied")]);
  assert.deepEqual(last.events.filter(event=>event.kind === "turnState"),[
    {kind:"turnState",threadId:ctx.threadId,turnId:"active-turn",state:"working"},
  ]);
  assert.equal(runtimeReduce(ctx,last.state,[frame("form.replied")]).events.length,0);

  const ended=runtimeReduce(ctx,asked.state,[frame("session.execution.succeeded"),frame("permission.replied"),frame("form.replied")]);
  assert.deepEqual(ended.events.filter(event=>event.kind === "turnState").map(event=>event.state),["idle"]);
});
check("settles an owned quota error once with a balance recovery notice",()=>{
  const {ctx,frames}=prepared("v2-provider-quota");
  ctx.activeTurnId="quota-turn";
  const started=copy(frames.find(frame=>(frame.params as Record<string,JsonValue>).type === "session.execution.started")!);
  const working=runtimeReduce(ctx,null,[started]);
  const quota=copy(started), params=quota.params as Record<string,JsonValue>;
  params.id="quota-error";
  params.type="session.error";
  const error={type:"provider.quota",status:402,body:{error:{type:"insufficient_balance_error",message:"Insufficient Balance"}}};
  params.data={sessionID:"foreign-session",error};
  const foreign=runtimeReduce(ctx,working.state,[quota]);
  assert.deepEqual(foreign.events,[]);
  params.data={sessionID:ctx.threadId!,error};
  const failed=runtimeReduce(ctx,foreign.state,[quota]);
  assert.deepEqual(failed.events,[
    {kind:"notice",threadId:ctx.threadId,message:"Provider balance or quota is exhausted — top up or switch model"},
    {kind:"turnState",threadId:ctx.threadId,turnId:"quota-turn",state:"idle",detail:"failed"},
  ]);
  params.id="execution-failed-after-quota";
  params.type="session.execution.failed";
  assert.deepEqual(runtimeReduce(ctx,failed.state,[quota]).events,[]);
});
check("recorded thinking, shell and text stay reconcilable across an active interrupt",()=>{
  const ctx=loaded("v2-stream-and-stop"), {frames,interrupt}=fixture.streamAndInterrupt;
  const type=(frame:RuntimeFrame)=>(frame.params as Record<string,JsonValue>).type;
  const firstEnd=frames.findIndex(frame=>type(frame) === "session.execution.succeeded");
  assert.ok(firstEnd > 0);
  ctx.activeTurnId="first";
  const first=runtimeReduce(ctx,null,frames.slice(0,firstEnd+1));
  const snapshots=first.events.filter(e=>e.kind === "chatItem").map(e=>e.item);
  const thinking=snapshots.findLast(item=>item.type === "thinking");
  const text=snapshots.findLast(item=>item.type === "assistant");
  const tool=snapshots.findLast(item=>item.type === "tool_call");
  assert.equal(thinking?.content,"Thinking before tool.");
  assert.equal(text?.content,"Fixture response after shell.");
  assert.equal(tool?.toolStatus,"completed");
  assert.equal(tool?.toolResult?.trim(),"fixture-stream");
  assert.deepEqual(snapshots.filter(item=>item.id === tool?.id).map(item=>item.toolStatus),["pending","running","running","completed"]);
  assert.ok(snapshots.filter(item=>item.id === tool?.id).every(item=>item.seq === tool?.seq));
  assert.deepEqual(first.events.filter(e=>e.kind === "chatItemAppend").map(e=>[e.itemId,e.offset,e.content]),[
    [thinking?.id,0,"Thinking before tool."],[text?.id,0,"Fixture response after shell."],
  ]);
  assert.ok(first.events.every(e=>e.kind !== "notice" && e.kind !== "chatItemPatch"));

  ctx.activeTurnId="second";
  ctx.activeTurns=[{threadId:ctx.threadId!,turnId:"second"}];
  const nextFrames=frames.slice(firstEnd+1);
  assert.equal(type(nextFrames[0]),"session.execution.started");
  const lateFrameTypes=["session.reasoning.delta","session.text.delta","session.tool.progress"];
  const late=lateFrameTypes.map(eventType=>{
    const frame=copy(frames.find(frame=>type(frame) === eventType)!);
    (frame.params as Record<string,JsonValue>).id=`late-${eventType}`;
    return frame;
  });
  const working=runtimeReduce(ctx,first.state,[nextFrames[0],...late]);
  assert.deepEqual(working.events,[{kind:"turnState",threadId:ctx.threadId,turnId:"second",state:"working"}]);

  // The host retains the transport and reducer state across a QuickJS reset.
  const reset={...ctx,tabId:"v2-stream-and-stop-reset"};
  assert.equal(binding(reset).protocol,undefined);
  const stop:Operation={kind:"interrupt",threadId:ctx.threadId!,activeTurnId:"second"};
  const plan=runtimePlan(reset,stop);
  assert.deepEqual(call(plan),{
    method:"session.abort",target:{threadId:ctx.threadId,turnId:"second"},
    params:{path:{sessionID:ctx.threadId}},
  });
  const manifest=JSON.parse(fs.readFileSync(new URL("../plugin.json",import.meta.url),"utf8"));
  const transport=manifest.capabilities.agentRuntime.httpSse;
  assert.deepEqual(transport.alternatives[0].routes[call(plan).method],{method:"POST",path:"/api/session/{sessionID}/interrupt"});
  assert.deepEqual(transport.routes[call(plan).method],{method:"POST",path:"/session/{sessionID}/abort"});
  assert.throws(()=>runtimePlan(reset,{...stop,threadId:"foreign"}),{code:"binding_mismatch"});
  assert.deepEqual(response(reset,stop,plan,interrupt),{kind:"complete",result:{kind:"done"},events:[]});
  assert.equal(response(reset,stop,plan,{interrupted:"true"}).kind,"refused");
  const completed=runtimeReduce(reset,working.state,nextFrames.slice(1));
  assert.deepEqual(completed.events,[{kind:"turnState",threadId:ctx.threadId,turnId:"second",state:"idle",detail:"interrupted"}]);
  assert.deepEqual(runtimeReduce(reset,completed.state,nextFrames.slice(1)).events,[]);
});
check("bookkeeping can reestablish a lost mapper and page history in ascending order",()=>{
  const ctx={...copy(fixture.context),tabId:"v2-bookkeeping",settings:{}};
  const threadId=ctx.threadId!;
  const resume:Operation={kind:"resumeThread",threadId,instructions:"opaque recovery",settings:ctx.settings};
  const health=runtimePlan(ctx,resume);
  assert.equal(call(health).method,"health");
  assert.equal(response(ctx,resume,health,{version:"2.3.0"}).kind,"refused");
  const get=continued(response(ctx,resume,health,fixture.health));
  assert.equal(call(get).method,"v2.session.get");
  const instructions=continued(response(ctx,resume,get,fixture.session));
  assert.equal(call(instructions).method,"v2.session.instructions");
  assert.equal(response(ctx,resume,instructions,null).kind,"complete");

  const read:Operation={kind:"readHistory",threadId,order:"asc"};
  const plan=runtimePlan(ctx,read);
  assert.deepEqual(call(plan).params,{path:{sessionID:threadId},query:{limit:"40",order:"asc"}});
  const page=response(ctx,read,plan,{data:[...fixture.history.data].reverse(),cursor:{next:"opaque-next"}});
  if(page.kind !== "complete" || page.result.kind !== "historyPage") throw new Error("history page required");
  assert.equal(page.result.order,"asc");
  assert.deepEqual(page.result.items,v2History(fixture.history.data));
  const following={...read,cursor:page.result.nextCursor};
  const second=runtimePlan(ctx,following);
  assert.deepEqual(call(second).params,{path:{sessionID:threadId},query:{limit:"40",cursor:"opaque-next"}});
  assert.equal(response(ctx,following,second,{data:[],cursor:{}}).kind,"complete");
});
check("bookkeeping serializes untracked text and bounds retained tool payloads",()=>{
  const ctx=loaded("v2-bookkeeping-state");
  delete ctx.activeTurnId;
  ctx.activeTurns=[];
  const frames=fixture.streamAndInterrupt.frames;
  const frame=(type:string)=>copy(frames.find(frame=>(frame.params as Record<string,JsonValue>).type === type)!);
  const vector=(frame:RuntimeFrame)=>frame.params as {data:Record<string,JsonValue>};
  const strictJson=(value:unknown)=>JSON.stringify(value,(_key,entry)=>{
    assert.notEqual(entry,undefined);
    return entry;
  });
  const source=frame("session.text.started");
  const text=runtimeReduce(ctx,null,[source]);
  assert.equal(text.events[0]?.kind,"chatItem");
  strictJson(text);

  ctx.activeTurnId="tool-turn";
  const called=frame("session.tool.called"), success=frame("session.tool.success");
  const large="工具".repeat(50_000);
  vector(called).data.input={data:large};
  vector(success).data.content=[{type:"text",text:large}];
  const running=runtimeReduce(ctx,text.state,[frame("session.tool.input.started"),called]);
  const row=running.events.filter(event=>event.kind === "chatItem").at(-1)?.item;
  assert.equal(row?.toolStatus,"running");
  assert.deepEqual(row?.toolInput,{data:large});
  assert.ok(Buffer.byteLength(strictJson(running.state)) <= 240*1024);
  const reduced=runtimeReduce(ctx,running.state,[success]);
  const completed=reduced.events.find(event=>event.kind === "chatItem")?.item;
  assert.equal(completed?.id,row?.id);
  assert.equal(completed?.toolStatus,"completed");
  assert.equal(completed?.toolResult,large);
  assert.ok(Buffer.byteLength(strictJson(reduced.state)) <= 240*1024);
  assert.ok(!strictJson(reduced.state).includes(large));
  assert.deepEqual(runtimeReduce(ctx,reduced.state,[success]).events,[]);
});
check("recorded resumed turn keeps reasoning and text with the same ordinal in separate rows",()=>{
  const ctx=loaded("v2-resumed-fragments"), capture=fixture.sameMessageFragments;
  ctx.activeTurnId="resumed-turn";
  let state:JsonValue=null;
  const events=capture.frames.flatMap(frame=>{
    const reduced=runtimeReduce(ctx,state,[frame]);
    state=reduced.state;
    return reduced.events;
  });
  const rows=events.filter(event=>event.kind === "chatItem").map(event=>event.item);
  const thinking=rows.findLast(row=>row.type === "thinking");
  const answer=rows.findLast(row=>row.type === "assistant");
  assert.equal(thinking?.content,"Fixture reasoning.");
  assert.equal(answer?.content,"Fixture final.");
  assert.notEqual(thinking?.id,answer?.id);
  assert.deepEqual(events.filter(event=>event.kind === "chatItemAppend").map(event=>[event.itemId,event.offset,event.content]),[
    [thinking?.id,0,"Fixture reasoning."],[answer?.id,0,"Fixture final."],
  ]);
  assert.deepEqual(events.at(-1),{kind:"turnState",threadId:ctx.threadId,turnId:"resumed-turn",state:"idle"});
  const history=v2History(capture.history.data).filter(row=>row.type === "assistant" || row.type === "thinking");
  assert.deepEqual(history.map(row=>[row.id,row.type,row.content]),[thinking,answer].map(row=>[row?.id,row?.type,row?.content]));
});
check("fresh default session accepts a second send after default effort confirmation",()=>{
  const ctx=initialized("v2-default-second-send","deepseek/deepseek-flash");
  ctx.threadId=undefined;
  const open:Operation={kind:"openThread",instructions:"opaque instructions",settings:ctx.settings};
  let plan=runtimePlan(ctx,open);
  assert.deepEqual((call(plan).params as Record<string,JsonValue>).body,{
    location:{directory:ctx.cwd},permissions:[{action:"*",resource:"*",effect:"ask"}],model:{providerID:"deepseek",id:"deepseek-flash"},
  });
  plan=continued(response(ctx,open,plan,fixture.session));
  assert.equal(response(ctx,open,plan,null).kind,"complete");
  ctx.threadId=fixture.context.threadId;
  const first:Operation={kind:"submitTurn",threadId:ctx.threadId!,sendId:"send-owned",input:ctx.claimedSends[0].input,settings:ctx.settings};
  plan=runtimePlan(ctx,first);
  const id=String(((call(plan).params as Record<string,JsonValue>).body as Record<string,JsonValue>).id);
  const frames=JSON.parse(JSON.stringify(fixture.frames).replaceAll("@message",id)) as RuntimeFrame[];
  for (const frame of frames) {
    const event=frame.params as Record<string,JsonValue>;
    if(event.type === "session.step.started") (event.data as Record<string,JsonValue>).model={providerID:"deepseek",id:"deepseek-flash",variant:"default"};
  }
  ctx.streamWindow={frameSequences:frames.map((_,i)=>101+i),turnRequests:[{sendId:first.sendId,turnId:id,afterSequence:100,written:true}]};
  assert.equal(response(ctx,first,plan,JSON.parse(JSON.stringify(fixture.receipt).replaceAll("@message",id))).kind,"complete");
  const reduced=runtimeReduce(ctx,null,frames);
  assert.ok(reduced.events.some(e=>e.kind === "turnState" && e.state === "idle"));
  for(const event of reduced.events) if(event.kind === "controlsConfirmed") ctx.settings={...ctx.settings,...event.settings};
  assert.deepEqual(ctx.settings,{model:"deepseek/deepseek-flash",effort:"default"});
  ctx.claimedSends=[{...ctx.claimedSends[0],sendId:"second-send"}];
  const second:Operation={...first,sendId:"second-send",settings:ctx.settings};
  const secondCall=call(runtimePlan(ctx,second)), secondBody=(secondCall.params as {body:{id:string}}).body;
  assert.equal(secondCall.method,"v2.session.prompt");
  assert.deepEqual(secondCall.params,{path:{sessionID:ctx.threadId},body:{id:secondBody.id,text:second.input.text,files:[]}});
  for(const effort of [undefined,"","default",null]) assert.deepEqual(v2Model(copy({model:ctx.settings.model,effort}) as RuntimeSettings),{providerID:"deepseek",id:"deepseek-flash"});
  for(const settings of [{model:ctx.settings.model,effort:"high"},{model:"another/model",effort:"default"}]) {
    assert.throws(()=>runtimePlan(ctx,{...second,sendId:"changed-settings",settings}),{code:"unsupported",message:"OpenCode model changes require restarting the same session"});
  }
  binding(ctx).settings={model:ctx.settings.model,effort:"high"};
  assert.throws(()=>runtimePlan(ctx,{...second,sendId:"clear-effort"}),{code:"unsupported"});
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
    assert.deepEqual(runtimeResponse(ctx,op,plan.state,{kind:"success",value:null}),{kind:"complete",result:{kind:"done"},events:[]});
    assert.equal(runtimeResponse(ctx,op,plan.state,{kind:"error",error:{code:404,message:"Request expired"}}).kind,"refused");
    assert.throws(()=>runtimePlan(ctx,{...op,requestId:"foreign"}));
    if (frame.method === "permission.asked") {
      const prompt=runtimeReduce(ctx,null,[frame]).events.find(event=>event.kind === "prompt");
      assert.deepEqual(prompt?.permission?.options,[{value:"once",decision:"allow_once"},{value:"reject",decision:"reject_once"}]);
      assert.deepEqual(prompt?.prompt.options.find(option=>option.value === "always"),{label:"Always allow",value:"always"});
      const manual=runtimePlan(ctx,{...op,answer:{kind:"choice",values:["always"]}});
      assert.deepEqual(manual.action,{kind:"reply",requestId:frame.requestId,result:{kind:"success",value:{decision:"always"}}});
    }
  }
});
check("five routed turns keep their stream identities and chronological order in resumed history",()=>{
  const ctx=loaded("v2-routed-history"), threadId=ctx.threadId!;
  const event=(frame:RuntimeFrame)=>(frame.params as Record<string,JsonValue>).type;
  const inbox=fixture.frames.find(frame=>event(frame) === "session.inbox.enqueued")!;
  const ended=fixture.frames.find(frame=>event(frame) === "session.text.ended")!;
  const wireHistory=fixture.history.data as {type:string;id:string;time:{created:number};text?:string;content?:unknown[]}[];
  const user=wireHistory.find(message=>message.type === "user")!;
  const assistant=wireHistory.find(message=>message.type === "assistant" && message.content?.length === 1)!;
  const history:unknown[]=[];
  const visible:{id:string;type:string;content:string}[]=[];
  let state:JsonValue=null;
  for (let index=0;index<5;index++) {
    const wireText=`<domios from="system" context="qa">Opaque prompt ${index}</domios>`;
    const sendId=`routed-${index}`, input={text:wireText,attachments:[]};
    ctx.claimedSends=[{sendId,threadId,input}];
    const submit:Operation={kind:"submitTurn",threadId,sendId,input,settings:ctx.settings};
    const planned=(call(runtimePlan(ctx,submit)).params as {body:{id:string}}).body;
    const accepted=copy(inbox), reply=copy(ended);
    const inboxEvent=accepted.params as {id:string;data:{inboxID:string;item:{payload:{text:string}}}};
    inboxEvent.id+=`-${index}`;
    inboxEvent.data.inboxID=planned.id;
    inboxEvent.data.item.payload.text=wireText;
    const replyEvent=reply.params as {id:string;data:{assistantMessageID:string;ordinal:number;text:string}};
    replyEvent.id+=`-${index}`;
    replyEvent.data.assistantMessageID=`reply-${index}`;
    replyEvent.data.ordinal=0;
    ctx.streamWindow={frameSequences:[3*index+1,3*index+2],turnRequests:[{sendId,turnId:planned.id,afterSequence:3*index,written:true}]};
    const reduced=runtimeReduce(ctx,state,[accepted,reply]);
    state=reduced.state;
    assert.ok(reduced.events.some(event=>event.kind === "sendEvidence" && event.sendId === sendId && event.itemId === planned.id));
    for (const event of reduced.events) {
      if (event.kind === "chatItem") {
        const {id,type,content}=event.item;
        visible.push({id,type,content});
      }
    }
    history.push(
      {...user,id:planned.id,text:wireText,time:{created:2*index}},
      {...assistant,id:replyEvent.data.assistantMessageID,content:[{type:"text",text:replyEvent.data.text}],time:{created:2*index+1}},
    );
  }
  const restarted={...ctx,epoch:ctx.epoch+2,bindingNonce:"history-restart",claimedSends:[],streamWindow:undefined};
  const initialize:Operation={kind:"initialize"};
  let plan=runtimePlan(restarted,initialize);
  for (const body of [fixture.health,fixture.providers]) plan=continued(response(restarted,initialize,plan,body));
  assert.equal(response(restarted,initialize,plan,fixture.integrations).kind,"complete");
  const resume:Operation={kind:"resumeThread",threadId,instructions:"opaque restart",settings:ctx.settings};
  plan=runtimePlan(restarted,resume);
  for (const body of [fixture.session,null]) plan=continued(response(restarted,resume,plan,body));
  assert.equal(response(restarted,resume,plan,null).kind,"complete");
  const read:Operation={kind:"readHistory",threadId,order:"desc"};
  plan=runtimePlan(restarted,read);
  assert.equal(call(plan).method,"v2.session.messages");
  const step=response(restarted,read,plan,{data:history.reverse()});
  if (step.kind !== "complete" || step.result.kind !== "historyPage") throw new Error("history page required");
  assert.equal(step.result.threadId,threadId);
  assert.equal(step.result.order,"desc");
  assert.deepEqual(step.events ?? [],[]);
  assert.equal(visible.length,10);
  assert.deepEqual(step.result.items.map(({id,type,content})=>({id,type,content})),visible);
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
