import assert from "node:assert/strict";
import fs from "node:fs";
import vm from "node:vm";
import { fileURLToPath } from "node:url";
import type { AgentRuntime, RuntimeContext, Operation, RuntimeAction, RpcReply, RuntimeFrame, RuntimeStep, AgentEvent, JsonValue, RuntimePlan, StdioAgentRuntimeCapability } from "@codeterm/plugin-sdk";
import { notificationMethods } from "../src/runtimeProtocol";

interface Fixture {
  schemaVersion: 1;
  context: RuntimeContext;
  operations: {operation:Operation; context?:RuntimeContext}[];
  rpc: (
    | {direction:"outbound"; operation:number; action:RuntimeAction}
    | {direction:"inbound"; operation:number; reply:RpcReply}
    | {direction:"inbound"; frame:RuntimeFrame; context?:RuntimeContext}
  )[];
  expected: {results:{operation:number;step:RuntimeStep}[];events:AgentEvent[]};
}
const root = new URL("./fixtures/runtime/",import.meta.url);
const descriptor = JSON.parse(fs.readFileSync(new URL("./runtime-manifest.json",import.meta.url),"utf8")) as {agentRuntime:StdioAgentRuntimeCapability};
const sandbox = vm.createContext({module:{exports:{}},exports:{},host:new Proxy({}, {get:()=>{throw new Error("Runtime hooks accessed host");}})});
sandbox.exports = sandbox.module.exports;
vm.runInContext(fs.readFileSync(new URL("../plugin.js",import.meta.url),"utf8"),sandbox);
const hooks = sandbox.module.exports.default as AgentRuntime;
const plain = <T>(value:T): T => JSON.parse(JSON.stringify(value));
let count = 0;
function check(name:string, run:()=>void) {run();count++;console.log(`  ✓ runtime ${name}`);}
function allowed(op:Operation, action:RuntimeAction) {
  if (action.kind === "questionAnswer") {assert.equal(op.kind,"answerQuestion");return;}
  if (action.kind === "reply") {assert.equal(op.kind,"answer"); return;}
  const call = action.kind === "request" ? action.call : action.notification;
  assert.ok(descriptor.agentRuntime.requestMethods[op.kind]?.includes(call.method),`${op.kind}: ${call.method}`);
}
function replay(fixture:Fixture) {
  const plans = new Map<number,RuntimePlan>();
  const ready = new Set<number>();
  const counts = new Map<number,number>();
  let binding:string | undefined;
  const results:Fixture["expected"]["results"] = [];
  const events:AgentEvent[] = [];
  let state:JsonValue = null;
  for (const record of fixture.rpc) {
    if ("frame" in record) {
      const context=record.context ?? fixture.context;
      const authority=JSON.stringify([context.epoch,context.tabId,context.bindingNonce,context.threadId ?? null]);
      if(binding !== authority) {state=null;binding=authority;}
      const reduced = hooks.runtimeReduce(context,state,[record.frame]);
      state = reduced.state; events.push(...reduced.events); continue;
    }
    const entry = fixture.operations[record.operation];
    assert.ok(entry,"Operation index exists");
    const ctx = entry.context ?? fixture.context;
    if (record.direction === "outbound") {
      let plan = plans.get(record.operation);
      if (!plan) {
        assert.ok(!results.some(result=>result.operation === record.operation),"Terminal operation must not replay");
        plan = hooks.runtimePlan(ctx,entry.operation);
      }
      else assert.ok(ready.has(record.operation),"Next call follows a continuation");
      allowed(entry.operation,plan.action);
      assert.deepEqual(plain(plan.action),record.action);
      if (plan.action.kind === "questionAnswer") plans.delete(record.operation);
      else plans.set(record.operation,plan);
      ready.delete(record.operation); continue;
    }
    const plan = plans.get(record.operation);
    assert.ok(plan,"Reply follows outbound action");
    const count=(counts.get(record.operation) ?? 0)+1;
    counts.set(record.operation,count);assert.ok(count <= 32,"Operation step bound");
    const step = hooks.runtimeResponse(ctx,entry.operation,plan.state,record.reply);
    if (step.kind === "continue") {
      if (entry.operation.kind === "submitTurn") {assert.equal(step.beforeAcceptance,true);assert.equal(record.reply.kind,"error");}
      plans.set(record.operation,step.plan);ready.add(record.operation);
    } else {
      if (step.kind === "complete") {
        events.push(...(step.events ?? []));
        if (step.notification) allowed(entry.operation,{kind:"notification",notification:step.notification});
      }
      results.push({operation:record.operation,step:plain(step)});plans.delete(record.operation);
    }
  }
  assert.equal(plans.size,0,"Every operation finishes");
  assert.deepEqual(results,fixture.expected.results);
  assert.deepEqual(plain(events),fixture.expected.events);
  return {events,state};
}
const names = fs.readdirSync(fileURLToPath(root)).filter(n => n.endsWith(".json")).sort();
assert.equal(names.length,8);
for (const name of names) check(`replay ${name}`,()=>replay(JSON.parse(fs.readFileSync(new URL(name,root),"utf8"))));
for (const name of fs.readdirSync(new URL("captured/",root)).filter(n=>n.endsWith(".json") && n!=="metrics.json").sort())
  check(`captured traffic ${name}`,()=>replay(JSON.parse(fs.readFileSync(new URL(`captured/${name}`,root),"utf8"))));
const sample = JSON.parse(fs.readFileSync(new URL("multi-turn.json",root),"utf8")) as Fixture;
const ctx = sample.context;
const submit = sample.operations[0].operation;
check("manifest inbound notification roster",()=>assert.deepEqual(descriptor.agentRuntime.notificationMethods,[...notificationMethods]));
check("no live activation before switch-over",()=> {
  const manifest = JSON.parse(fs.readFileSync(new URL("../plugin.json",import.meta.url),"utf8"));
  assert.equal(manifest.capabilities.agentRuntime,undefined);
});
check("exact steer refusal only",()=> {
  const steer = {...submit,activeTurnId:"active"} as Operation;
  const context = {...ctx,activeTurnId:"active"};
  const plan = hooks.runtimePlan(context,steer);
  for (const error of [{code:-32600,message:"temporary error"},{code:-32600,message:"no active turn to steer",data:{retry:true}},{code:-32000,message:"no active turn to steer"}])
    assert.equal(hooks.runtimeResponse(context,steer,plan.state,{kind:"error",error}).kind,"refused");
  assert.equal(hooks.runtimeResponse(context,steer,plan.state,{kind:"success",value:{}}).kind,"complete");
});
check("wrong thread and active turn refuse",()=> {
  assert.throws(()=>hooks.runtimePlan(ctx,{kind:"interrupt",threadId:"foreign",activeTurnId:"active"}));
  assert.throws(()=>hooks.runtimePlan(ctx,{kind:"interrupt",threadId:ctx.threadId!,activeTurnId:"wrong"}));
});
check("numeric and string approval ids are distinct",()=> {
  assert.throws(()=>hooks.runtimePlan({...ctx,pendingRequest:{requestId:7,method:"item/commandExecution/requestApproval",params:{}}},
    {kind:"answer",requestId:"7",answer:{kind:"cancel"}}));
});
check("unsupported controls and modes refuse",()=> {
  assert.throws(()=>hooks.runtimePlan(ctx,{kind:"control",action:"globalConfig",value:{}}));
  assert.throws(()=>hooks.runtimePlan(ctx,{kind:"openThread",instructions:"",settings:{mode:"unknown"}}));
});
check("fresh thread materializes before loaded",()=> {
  const operation:Operation = {kind:"openThread",instructions:"Own instructions",settings:ctx.settings};
  const plan = hooks.runtimePlan(ctx,operation);
  assert.equal(plan.action.kind,"request");
  const step = hooks.runtimeResponse(ctx,operation,plan.state,{kind:"success",value:{thread:{id:"fresh"}}});
  assert.equal(step.kind,"continue");
  if (step.kind !== "continue") return;
  assert.deepEqual(plain(step.plan.action),{kind:"request",call:{method:"thread/inject_items",params:{threadId:"fresh",items:[{type:"message",role:"developer",content:[{type:"input_text",text:"Domios thread binding marker."}]}]}}});
  assert.equal(hooks.runtimeResponse(ctx,operation,step.plan.state,{kind:"error",error:{code:-32000,message:"failed"}}).kind,"refused");
});
check("resume never accepts another thread",()=> {
  const operation:Operation = {kind:"resumeThread",threadId:ctx.threadId!,settings:ctx.settings};
  const plan = hooks.runtimePlan(ctx,operation);
  for (const thread of [{id:"foreign"},{id:ctx.threadId,parentThreadId:"parent"},{id:ctx.threadId,source:{subAgent:{}}}])
    assert.equal(hooks.runtimeResponse(ctx,operation,plan.state,{kind:"success",value:{thread}}).kind,"refused");
});
check("delivery cannot use old text, foreign thread or unmatched client",()=> {
  const frame:RuntimeFrame = {kind:"notification",method:"item/completed",params:{threadId:ctx.threadId!,turnId:"turn-a",item:{id:"user",type:"userMessage",clientId:"send-a",content:[{type:"text",text:"Wrong text"}]}}};
  assert.equal(hooks.runtimeReduce(ctx,null,[frame]).events.some(e=>e.kind === "sendEvidence"),false);
  const empty = {...ctx,claimedSends:[]};
  const genuine = sample.rpc.find(r=>"frame" in r && r.frame.method === "item/completed")!;
  assert.ok("frame" in genuine);
  assert.equal(hooks.runtimeReduce(empty,null,[genuine.frame]).events.some(e=>e.kind === "sendEvidence"),false);
  assert.deepEqual(plain(hooks.runtimeReduce({...ctx,threadId:"foreign"},null,[genuine.frame]).events),[]);
});
check("claim evidence is emitted once",()=> {
  const entry = sample.rpc.find(r=>"frame" in r && r.frame.method === "item/completed")!;
  assert.ok("frame" in entry);
  const reduction = hooks.runtimeReduce(ctx,null,[entry.frame,entry.frame]);
  assert.equal(reduction.events.filter(e=>e.kind === "sendEvidence").length,1);
});
check("delta batches preserve exact ordering",()=> {
  const frames:RuntimeFrame[] = ["a","b","c"].map(delta=>({kind:"notification",method:"item/agentMessage/delta",params:{threadId:ctx.threadId!,turnId:"turn-a",itemId:"stream",delta}}));
  const batch = hooks.runtimeReduce(ctx,null,frames);
  let state:JsonValue = null;const events:AgentEvent[]=[];
  for (const frame of frames) {const step=hooks.runtimeReduce(ctx,state,[frame]);state=step.state;events.push(...step.events);}
  assert.deepEqual(plain(batch),plain({state,events}));
  assert.equal(batch.events.at(-1)?.kind,"chatItem");
  const last=batch.events.at(-1);if(last?.kind === "chatItem") assert.equal(last.item.content,"abc");
});
check("old completed turn cannot settle newer turn",()=> {
  const frames:RuntimeFrame[] = [{kind:"notification",method:"turn/started",params:{threadId:ctx.threadId!,turn:{id:"new"}}},{kind:"notification",method:"turn/completed",params:{threadId:ctx.threadId!,turn:{id:"old"}}}];
  assert.equal(hooks.runtimeReduce(ctx,null,frames).events.filter(e=>e.kind === "turnState").length,1);
});
check("compaction context restore event deduplicates",()=> {
  const frame:RuntimeFrame={kind:"notification",method:"item/completed",params:{threadId:ctx.threadId!,turnId:"turn-a",item:{id:"compact",type:"contextCompaction"}}};
  assert.equal(hooks.runtimeReduce(ctx,null,[frame,frame]).events.filter(e=>e.kind === "contextCompacted").length,1);
});
check("fixture oracle detects corrupted expected content",()=> {
  const corrupted=structuredClone(sample);corrupted.expected.events[1]={kind:"turnState",threadId:ctx.threadId!,state:"needs_input"};
  assert.throws(()=>replay(corrupted));
});
console.log(`runtime hooks: ${count} checks passed (8 shared scenarios)`);
check("reasoning summary and content streams stay separate",()=> {
  const frames:RuntimeFrame[]=[
    {kind:"notification",method:"item/reasoning/textDelta",params:{threadId:ctx.threadId!,turnId:"turn-a",itemId:"r",contentIndex:0,delta:"private content"}},
    {kind:"notification",method:"item/reasoning/summaryTextDelta",params:{threadId:ctx.threadId!,turnId:"turn-a",itemId:"r",summaryIndex:0,delta:"summary"}},
    {kind:"notification",method:"item/reasoning/summaryTextDelta",params:{threadId:ctx.threadId!,turnId:"turn-a",itemId:"r",summaryIndex:1,delta:"second"}},
  ];
  const result=hooks.runtimeReduce(ctx,null,frames);const last=result.events.at(-1);
  assert.ok(last?.kind === "chatItem");assert.equal(last.item.content,"summary\nsecond");
});
check("rate limits merge sparse updates without token counts",()=> {
  const frames:RuntimeFrame[]=[
    {kind:"notification",method:"account/rateLimits/updated",params:{rateLimits:{primary:{usedPercent:20,resetsAt:100,windowDurationMins:300},secondary:{usedPercent:60}}}},
    {kind:"notification",method:"account/rateLimits/updated",params:{rateLimits:{limitId:"codex",rateLimitReachedType:"weekly"}}},
    {kind:"notification",method:"account/rateLimits/updated",params:{rateLimits:{limitId:"model-specific",primary:{usedPercent:90}}}},
  ];
  const result=hooks.runtimeReduce(ctx,null,frames);
  assert.deepEqual(plain(result.events),[
    {kind:"rateLimits",rateLimits:[{window:"primary",usedPercent:20,resetsAt:100000,windowMinutes:300},{window:"secondary",usedPercent:60}]},
    {kind:"rateLimits",rateLimits:[{window:"primary",usedPercent:20,resetsAt:100000,windowMinutes:300},{window:"secondary",usedPercent:60}]},
  ]);
});
check("async question retains native values and formats ordinary input",()=> {
  const frame:RuntimeFrame={kind:"notification",method:"item/completed",params:{threadId:ctx.threadId!,turnId:"turn-a",item:{type:"agentMessage",id:"async",delivery:"async",text:"Choose",questions:[{id:"q",title:"Pick",options:[{id:"native-value",label:"Display label"},"Exact string"]}]}}};
  const result=hooks.runtimeReduce(ctx,null,[frame,frame]);
  assert.deepEqual(plain(result.events),[{kind:"question",threadId:ctx.threadId!,itemId:"async",questions:[{id:"q",text:"Pick",options:[{value:"native-value",label:"Display label"},{value:"Exact string",label:"Exact string"}]}],response:"message"}]);
  const operation:Operation={kind:"answerQuestion",threadId:ctx.threadId!,itemId:"async",answers:[{questionId:"q",values:["native-value"]}]};
  const plan=hooks.runtimePlan(ctx,operation);
  assert.equal(plan.action.kind,"questionAnswer");
  if(plan.action.kind === "questionAnswer") assert.deepEqual(JSON.parse(plan.action.input.text),{itemId:"async",answers:[{questionId:"q",values:["native-value"]}]});
  const restored=hooks.runtimeReduce({...ctx,epoch:2},result.state,[frame]);assert.equal(restored.events[0].kind,"question");
});
check("blocking question sends native id instead of display label",()=> {
  const context={...ctx,pendingRequest:{requestId:"q",method:"item/tool/requestUserInput",params:{questions:[{id:"q1",question:"Choose",options:[{value:"native",label:"Display"}]}]}}};
  const plan=hooks.runtimePlan(context,{kind:"answer",requestId:"q",answer:{kind:"choice",values:["native"]}});
  assert.deepEqual(plain(plan.action),{kind:"reply",requestId:"q",result:{kind:"success",value:{answers:{q1:{answers:["native"]}}}}});
});
check("provider timestamps become epoch ms and receipt time stays host owned",()=> {
  const events=hooks.runtimeReduce(ctx,null,[{kind:"notification",method:"turn/started",params:{threadId:ctx.threadId!,turn:{id:"goal-a",startedAt:100}}},
    {kind:"notification",method:"turn/completed",params:{threadId:ctx.threadId!,turn:{id:"goal-a",startedAt:100,completedAt:105}}},
    {kind:"notification",method:"turn/started",params:{threadId:ctx.threadId!,turn:{id:"goal-b",startedAt:105}}}]).events;
  assert.deepEqual(plain(events),[{kind:"turnState",threadId:ctx.threadId!,turnId:"goal-a",state:"working",startedAt:100000},
    {kind:"turnState",threadId:ctx.threadId!,turnId:"goal-a",state:"idle",startedAt:100000,completedAt:105000},
    {kind:"turnState",threadId:ctx.threadId!,turnId:"goal-b",state:"working",startedAt:105000}]);
});
check("file change and turn diff use app-server arrays and exact hunks",()=> {
  const diff="@@ -1 +1 @@\n-old\n+new\n";
  const frame:RuntimeFrame={kind:"notification",method:"item/completed",params:{threadId:ctx.threadId!,turnId:"turn-a",item:{id:"file",type:"fileChange",status:"completed",changes:[{path:"file.txt",kind:{type:"update"},diff}]}}};
  const result=hooks.runtimeReduce(ctx,null,[frame]);const event=result.events[0];assert.ok(event.kind === "chatItem");
  assert.deepEqual(event.item.fileDiffs && plain(event.item.fileDiffs),[{path:"file.txt",kind:"modify",additions:1,deletions:1,hunks:[{oldStart:1,oldLines:1,newStart:1,newLines:1,lines:["-old","+new"]}]}]);
  const turn=hooks.runtimeReduce(ctx,null,[{kind:"notification",method:"turn/diff/updated",params:{threadId:ctx.threadId!,turnId:"turn-a",diff:`diff --git a/file.txt b/file.txt\n--- a/file.txt\n+++ b/file.txt\n${diff}`}}]).events[0];
  assert.ok(turn.kind === "chatItem");assert.deepEqual(plain(turn.item.fileDiffs),plain(event.item.fileDiffs));
});
console.log(`runtime expanded hooks: ${count} checks passed`);
check("descendant interrupt walks deepest first with explicit targets",()=> {
  const context={...ctx,threadParents:[{threadId:"child",parentThreadId:ctx.threadId!},{threadId:"grandchild",parentThreadId:"child"}],activeTurnId:"root-turn",activeTurns:[{threadId:ctx.threadId!,turnId:"root-turn"},{threadId:"child",turnId:"child-turn",parentThreadId:ctx.threadId!},{threadId:"grandchild",turnId:"grandchild-turn",parentThreadId:"child"}]};
  const operation:Operation={kind:"interrupt",threadId:ctx.threadId!,activeTurnId:"root-turn"};
  let plan=hooks.runtimePlan(context,operation);
  for(const target of [{threadId:"grandchild",turnId:"grandchild-turn"},{threadId:"child",turnId:"child-turn"},{threadId:ctx.threadId!,turnId:"root-turn"}]) {
    assert.equal(plan.action.kind,"request");if(plan.action.kind !== "request")return;
    assert.deepEqual(plain(plan.action.call.target),target);assert.deepEqual(plain(plan.action.call.params),target);
    const step=hooks.runtimeResponse(context,operation,plan.state,{kind:"success",value:{}});
    if(step.kind === "continue")plan=step.plan;else assert.equal(step.kind,"complete");
  }
});
check("subagent thread source verifies child turn state",()=> {
  const frames:RuntimeFrame[]=[
    {kind:"notification",method:"thread/started",params:{thread:{id:"child",source:{subAgent:{thread_spawn:{parent_thread_id:ctx.threadId!,depth:1}}}}}},
    {kind:"notification",method:"turn/started",params:{threadId:"child",turn:{id:"child-turn",startedAt:100}}},
    {kind:"notification",method:"turn/started",params:{threadId:"foreign",turn:{id:"foreign-turn"}}},
  ];
  assert.deepEqual(plain(hooks.runtimeReduce(ctx,null,frames).events),[{kind:"turnState",threadId:"child",turnId:"child-turn",parentThreadId:ctx.threadId!,state:"working",startedAt:100000}]);
});
check("initialize guards handshake before publishing observed auth",()=> {
  const operation:Operation={kind:"initialize"};let plan=hooks.runtimePlan(ctx,operation);
  let step=hooks.runtimeResponse(ctx,operation,plan.state,{kind:"success",value:{}});assert.equal(step.kind,"continue");if(step.kind !== "continue")return;
  assert.deepEqual(plain(step.plan.action),{kind:"notification",notification:{method:"initialized",params:{}}});
  step=hooks.runtimeResponse(ctx,operation,step.plan.state,{kind:"success",value:null});assert.equal(step.kind,"continue");if(step.kind !== "continue")return;
  assert.deepEqual(plain(step.plan.action),{kind:"request",call:{method:"account/read",params:{refreshToken:false}}});
  step=hooks.runtimeResponse(ctx,operation,step.plan.state,{kind:"success",value:{account:null,requiresOpenaiAuth:true}});
  assert.deepEqual(plain(step),{kind:"complete",result:{kind:"initialized"},events:[{kind:"auth",state:"signedOut"}]});
});
check("auth refresh and account updates retain unknown state honestly",()=> {
  const operation:Operation={kind:"control",action:"auth",value:null};const plan=hooks.runtimePlan(ctx,operation);
  assert.deepEqual(plain(hooks.runtimeResponse(ctx,operation,plan.state,{kind:"success",value:{}})),{kind:"complete",result:{kind:"done"},events:[{kind:"auth",state:"unknown"}]});
  assert.deepEqual(plain(hooks.runtimeReduce({...ctx,threadId:undefined},null,[{kind:"notification",method:"account/updated",params:{authMode:"chatgpt"}}]).events),[{kind:"auth",state:"signedIn"}]);
});
check("retry errors remain nonterminal with classified source metadata",()=> {
  const events=hooks.runtimeReduce(ctx,null,[{kind:"notification",method:"error",params:{threadId:ctx.threadId!,turnId:"turn-a",willRetry:true,error:{message:"Retrying",codexErrorInfo:{httpConnectionFailed:{httpStatusCode:503}}}}}]).events;
  assert.equal(events.length,1);const event=events[0];assert.ok(event.kind === "chatItem");assert.equal(event.item.systemSubtype,"provider_retry");assert.deepEqual(plain(event.item.toolInput),{code:"httpConnectionFailed",class:"transport_error",retryable:true});
});
console.log(`runtime final hooks: ${count} checks passed`);
check("history includes optional goal without making a failed optional probe fatal",()=> {
  const operation:Operation={kind:"readHistory",threadId:ctx.threadId!};const plan=hooks.runtimePlan(ctx,operation);
  const step=hooks.runtimeResponse(ctx,operation,plan.state,{kind:"success",value:{data:[]}});
  assert.equal(step.kind,"continue");if(step.kind !== "continue")return;
  assert.deepEqual(plain(step.plan.action),{kind:"request",call:{method:"thread/goal/get",params:{threadId:ctx.threadId}}});
  const success=hooks.runtimeResponse(ctx,operation,step.plan.state,{kind:"success",value:{goal:{objective:"Own goal"}}});
  assert.equal(success.kind,"complete");if(success.kind === "complete" && success.result.kind === "historyPage")assert.equal(success.result.items[0].sessionIntent?.goal,"Own goal");
  assert.equal(hooks.runtimeResponse(ctx,operation,step.plan.state,{kind:"error",error:{code:-32601,message:"unsupported"}}).kind,"complete");
});
check("one collaboration item maps every reported receiver",()=> {
  const frames:RuntimeFrame[]=[{kind:"notification",method:"item/completed",params:{threadId:ctx.threadId!,turnId:"turn-a",item:{id:"agents",type:"collabAgentToolCall",receiverThreadIds:["a","b"],agentsStates:{a:{status:"completed"},b:{status:"errored"}}}}}];
  const rows=hooks.runtimeReduce(ctx,null,frames).events;
  assert.equal(rows.length,2);assert.deepEqual(plain(rows.map(e=>e.kind === "chatItem" ? e.item.id : undefined)),["agents:a","agents:b"]);
});
console.log(`runtime completed hooks: ${count} checks passed`);
check("unsupported MCP forms remain visible and can only decline or cancel",()=> {
  const context={...ctx,pendingRequest:{requestId:"mcp",method:"mcpServer/elicitation/request",params:{threadId:ctx.threadId!,mode:"url",message:"Open this external form"}}};
  const result=hooks.runtimeReduce(ctx,null,[{kind:"request",requestId:"mcp",method:"mcpServer/elicitation/request",params:context.pendingRequest.params}]);
  const prompt=result.events[0];assert.ok(prompt.kind === "prompt");assert.deepEqual(plain(prompt.prompt.options.map(o=>o.value)),["decline","cancel"]);
  assert.throws(()=>hooks.runtimePlan(context,{kind:"answer",requestId:"mcp",answer:{kind:"choice",values:["accept"]}}));
  assert.deepEqual(plain(hooks.runtimePlan(context,{kind:"answer",requestId:"mcp",answer:{kind:"choice",values:["decline"]}}).action),{kind:"reply",requestId:"mcp",result:{kind:"success",value:{action:"decline"}}});
});
console.log(`runtime reviewed hooks: ${count} checks passed`);
check("inactive parent retains ancestry but is never an interrupt target",()=> {
  const context={...ctx,activeTurnId:"root-turn",threadParents:[{threadId:"child",parentThreadId:ctx.threadId!},{threadId:"grandchild",parentThreadId:"child"}],
    activeTurns:[{threadId:ctx.threadId!,turnId:"root-turn"},{threadId:"grandchild",turnId:"grandchild-turn"}]};
  const operation:Operation={kind:"interrupt",threadId:ctx.threadId!};
  const plan=hooks.runtimePlan(context,operation);assert.equal(plan.action.kind,"request");if(plan.action.kind !== "request")return;
  assert.deepEqual(plain(plan.action.call.target),{threadId:"grandchild",turnId:"grandchild-turn"});
  const next=hooks.runtimeResponse(context,operation,plan.state,{kind:"success",value:{}});assert.equal(next.kind,"continue");if(next.kind !== "continue" || next.plan.action.kind !== "request")return;
  assert.deepEqual(plain(next.plan.action.call.target),{threadId:ctx.threadId!,turnId:"root-turn"});
  assert.equal(hooks.runtimeResponse(context,operation,next.plan.state,{kind:"success",value:{}}).kind,"complete");
  const childStart:RuntimeFrame={kind:"notification",method:"turn/started",params:{threadId:"grandchild",turn:{id:"grandchild-turn"}}};
  assert.deepEqual(plain(hooks.runtimeReduce(context,null,[childStart]).events),[{kind:"turnState",threadId:"grandchild",turnId:"grandchild-turn",parentThreadId:"child",state:"working"}]);
});
check("idle root with no roster turn is not a synthetic interrupt target",()=> {
  assert.throws(()=>hooks.runtimePlan({...ctx,activeTurnId:"stale",activeTurns:[]},{kind:"interrupt",threadId:ctx.threadId!}));
});
console.log(`runtime ancestry hooks: ${count} checks passed`);
check("history questions survive item pagination and require exact later native answers",()=> {
  const operation:Operation={kind:"readHistory",threadId:ctx.threadId!};
  const question={id:"history-question",type:"agentMessage",delivery:"async",text:"Choose",questions:[{id:"q1",title:"Pick",options:[{value:"native",label:"Display"}]}]};
  function read(answerItem:JsonValue) {
    const first=hooks.runtimePlan(ctx,operation);
    let step=hooks.runtimeResponse(ctx,operation,first.state,{kind:"success",value:{data:[{id:"history-turn",status:"completed"}],nextCursor:"next-turn-page"}});
    assert.equal(step.kind,"continue");if(step.kind !== "continue")throw new Error("Missing items plan");
    step=hooks.runtimeResponse(ctx,operation,step.plan.state,{kind:"success",value:{data:[{item:question}],nextCursor:"next-item-page"}});
    assert.equal(step.kind,"continue");if(step.kind !== "continue")throw new Error("Missing item continuation");
    step=hooks.runtimeResponse(ctx,operation,step.plan.state,{kind:"success",value:{data:[{item:answerItem}]}});
    assert.ok(step.kind === "complete" && step.result.kind === "historyPage");
    return step.result;
  }
  const envelope={itemId:"history-question",answers:[{questionId:"q1",values:["native"]}]};
  const item={id:"answer",type:"userMessage",content:[{type:"text",text:JSON.stringify(envelope)}]};
  assert.deepEqual(plain(read(item).questions),[{threadId:ctx.threadId!,itemId:"history-question",questions:[{id:"q1",text:"Pick",options:[{value:"native",label:"Display"}]}],response:"message",resolved:true}]);
  for (const body of [ {...envelope,itemId:"wrong"},{...envelope,answers:[{questionId:"q1",values:["Display"]}]},{...envelope,answers:[{questionId:"wrong",values:["native"]}]}]) {
    const result=read({...item,content:[{type:"text",text:JSON.stringify(body)}]});
    assert.equal(result.questions?.[0].resolved,undefined);
  }
  assert.equal(read({id:"assistant-answer",type:"agentMessage",text:JSON.stringify(envelope)}).questions?.[0].resolved,undefined);
});
console.log(`runtime history hooks: ${count} checks passed`);
check("reinitialize response auth is part of the full replay event stream",()=> {
  const fixture=JSON.parse(fs.readFileSync(new URL("runtime-restart.json",root),"utf8")) as Fixture;
  const result=replay(fixture);assert.equal(result.events.filter(event=>event.kind === "auth").length,1);
  fixture.expected.events=fixture.expected.events.filter(event=>event.kind !== "auth");
  assert.throws(()=>replay(fixture));
});
console.log(`runtime full event hooks: ${count} checks passed`);
