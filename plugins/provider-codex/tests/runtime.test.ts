import assert from "node:assert/strict";
import { sessionChanges } from "../../../packages/chat-core/src/sessionChanges";
import type { NormalizedChatMessage } from "../../../packages/chat-core/src/types";
import fs from "node:fs";
import vm from "node:vm";
import { fileURLToPath } from "node:url";
import type { AgentRuntime, RuntimeContext, Operation, RuntimeAction, RpcReply, RuntimeFrame, RuntimeStep, AgentEvent, JsonValue, RuntimePlan, OperationResult, StdioAgentRuntimeCapability } from "@codeterm/plugin-sdk";
import { collectHistory } from "./runtimeHistoryHost";
import { notificationMethods } from "../src/runtimeProtocol";
import { agentActivityOf, buildAgentTree } from "../../../packages/chat-core/src/agentActivity";
import { descendantThread, historyThread } from "../src/runtimeBinding";

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
const hostOwnedConfigFixture = JSON.parse(fs.readFileSync(new URL("../../../tests/fixtures/app-server-host-owned-config.json",import.meta.url),"utf8")) as Record<string,boolean>;
const descriptor = JSON.parse(fs.readFileSync(new URL("./runtime-manifest.json",import.meta.url),"utf8")) as {agentRuntime:StdioAgentRuntimeCapability};
const sandbox = vm.createContext({module:{exports:{}},exports:{},host:new Proxy({}, {get:()=>{throw new Error("Runtime hooks accessed host");}})});
sandbox.exports = sandbox.module.exports;
vm.runInContext(fs.readFileSync(new URL("../plugin.js",import.meta.url),"utf8"),sandbox);
const hooks = sandbox.module.exports.default as AgentRuntime;
const reduce = hooks.runtimeReduce.bind(hooks);
hooks.runtimeReduce = (...args) => {
  const result = reduce(...args);
  const serialized = JSON.stringify(result, (key, value: unknown) => {
    assert.notEqual(value, undefined, `runtimeReduce returned undefined at ${key}`);
    assert.ok(!["function", "symbol", "bigint"].includes(typeof value), `runtimeReduce returned non-JSON at ${key}`);
    if (typeof value === "number") assert.ok(Number.isFinite(value), `runtimeReduce returned non-finite number at ${key}`);
    return value;
  });
  return JSON.parse(serialized);
};
const plain = <T>(value:T): T => JSON.parse(JSON.stringify(value));
let count = 0;
function check(name:string, run:()=>void) {run();count++;console.log(`  ✓ runtime ${name}`);}
function assertHostOwnedConfig(plan:RuntimePlan, method:string) {
  assert.equal(plan.action.kind,"request");
  if (plan.action.kind !== "request") throw new Error("Expected thread start request");
  assert.equal(plan.action.call.method,method);
  const params = plan.action.call.params as Record<string,JsonValue>;
  const config = params.config as Record<string,JsonValue>;
  assert.deepEqual(Object.fromEntries(Object.entries(config).filter(([key])=>key.startsWith("features."))),hostOwnedConfigFixture);
}
check("descendant reads require an unambiguous chain to the current root",()=> {
  const fixture = JSON.parse(fs.readFileSync(new URL("captured/subagent.json",root),"utf8")) as Fixture;
  const context = {...fixture.context,threadParents:[{threadId:"child",parentThreadId:fixture.context.threadId!},
    {threadId:"grandchild",parentThreadId:"child"}]};
  assert.equal(descendantThread(context,"grandchild"),true);
  for (const parents of [[],[{threadId:"child",parentThreadId:"foreign"}],
    [{threadId:"child",parentThreadId:"child"}],
    [...context.threadParents,{threadId:"child",parentThreadId:"foreign"}]]) {
    assert.equal(descendantThread({...context,threadParents:parents},"child"),false);
  }
  assert.equal(historyThread(context,{thread:{id:"child",parentThreadId:context.threadId}},"child"),true);
  assert.equal(historyThread(context,{thread:{id:"child",parentThreadId:"foreign"}},"child"),false);
  assert.equal(historyThread(context,{thread:{id:"foreign",parentThreadId:context.threadId}},"child"),false);
  assert.equal(historyThread(context,{thread:{id:"child",parentThreadId:context.threadId,source:{subAgent:{thread_spawn:{parent_thread_id:"foreign"}}}}},"child"),false);
});
check("captured child identities permit history plans without mutation authority",()=> {
  for (const name of ["subagent","subagent_v2"]) {
    const fixture = JSON.parse(fs.readFileSync(new URL(`captured/${name}.json`,root),"utf8")) as Fixture;
    const result = hooks.runtimeReduce(fixture.context,null,fixture.rpc.flatMap(record=>"frame" in record ? [record.frame] : []));
    const rows = result.events.flatMap(event=>event.kind === "chatItem" && event.item.toolKind === "agent" ? [event.item] : []);
    assert.ok(rows.length > 0);
    const input = rows[0].toolInput as Record<string,JsonValue>;
    const child = String(input.agent_id);
    assert.equal(typeof input.agent_ordinal,"number");
    const context = {...fixture.context,threadParents:[{threadId:child,parentThreadId:fixture.context.threadId!}]};
    const op:Operation = {kind:"readHistory",threadId:child,order:"desc"};
    const plan = hooks.runtimePlan(context,op);
    assert.equal(plan.action.kind,"request");
    if (plan.action.kind !== "request") throw new Error("Expected history request");
    assert.equal(plan.action.call.method,"thread/turns/list");
    assert.equal((plan.action.call.params as Record<string,JsonValue>).threadId,child);
    assert.throws(()=>hooks.runtimePlan(context,{...op,threadId:"foreign"}));
    assert.throws(()=>hooks.runtimePlan({...context,threadParents:[]},op));
    assert.throws(()=>hooks.runtimeResponse({...context,threadParents:[]},op,plan.state,{kind:"success",value:{data:[]}}));
    const mutations:Operation[] = [
      {kind:"resumeThread",threadId:child,instructions:"",settings:{}},
      {kind:"submitTurn",threadId:child,sendId:"send",input:{text:"hello",attachments:[]},settings:{}},
      {kind:"interrupt",threadId:child},
      {kind:"control",threadId:child,action:"compact",value:null},
    ];
    for (const mutation of mutations) assert.throws(()=>hooks.runtimePlan(context,mutation));
    const turns = hooks.runtimeResponse(context,op,plan.state,{kind:"success",value:{data:[{id:"child-turn",status:"completed"}],nextCursor:null}});
    if (turns.kind !== "continue" || turns.plan.action.kind !== "request") throw new Error("Expected child items");
    assert.equal(turns.plan.action.call.target?.threadId,child);
    const history = hooks.runtimeResponse(context,op,turns.plan.state,{kind:"success",value:{data:[{item:{id:"child-answer",type:"agentMessage",text:"Child history answer"}}],nextCursor:null}});
    if (history.kind !== "complete" || history.result.kind !== "historyPage") throw new Error("Expected child history");
    assert.equal(history.result.threadId,child);
    assert.equal(history.result.items[0].content,"Child history answer");
    const emptyChild = hooks.runtimeResponse(context,op,{stage:"unmaterialized"},{kind:"success",value:{thread:{id:child,parentThreadId:context.threadId!,turns:[]}}});
    assert.equal(emptyChild.kind,"complete","An authenticated child with no messages still opens read-only");
    const foreign = hooks.runtimeResponse(context,op,{stage:"unmaterialized"},{kind:"success",value:{thread:{id:"foreign",parentThreadId:context.threadId!,turns:[]}}});
    assert.equal(foreign.kind,"refused");
  }
});
check("only authenticated descendant spawn frames publish nested child metadata",()=>{
  const fixture = JSON.parse(fs.readFileSync(new URL("captured/subagent.json",root),"utf8")) as Fixture;
  const context = {...fixture.context,threadParents:[{threadId:"child",parentThreadId:fixture.context.threadId!}]};
  const frame:RuntimeFrame={kind:"notification",method:"item/completed",params:{threadId:"child",turnId:"turn",item:{type:"collabAgentToolCall",id:"nested",tool:"spawnAgent",receiverThreadIds:["grandchild"],prompt:"Inspect nested",model:"fixture",reasoningEffort:"high"}}};
  const rows=hooks.runtimeReduce(context,null,[frame]).events;
  assert.equal(rows.length,1);
  const row=rows[0];if(row.kind!=="chatItem")throw new Error("Expected child metadata");
  assert.equal(row.threadId,context.threadId);
  assert.equal((row.item.toolInput as Record<string,JsonValue>).agent_parent_thread_id,"child");
  assert.equal(hooks.runtimeReduce({...context,threadParents:[]},null,[frame]).events.length,0);
});
function allowed(op:Operation, action:RuntimeAction) {
  if (action.kind === "questionAnswer") {assert.equal(op.kind,"answerQuestion");return;}
  if (action.kind === "reply") {assert.equal(op.kind,"answer"); return;}
  const call = action.kind === "request" ? action.call : action.notification;
  assert.ok(descriptor.agentRuntime.requestMethods[op.kind]?.includes(call.method),`${op.kind}: ${call.method}`);
}
function replay(fixture:Fixture) {
  const chunks = new Map<number,Extract<RuntimeStep,{kind:"complete"}>["result"][]>();
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
      const expected = record.action.kind === "request" && record.action.call.method === "account/rateLimits/read"
        ? {...record.action,call:{...record.action.call,params:{excludeResetCreditDetails:true}}} : record.action;
      assert.deepEqual(plain(plan.action),expected);
      if (plan.action.kind === "questionAnswer") plans.delete(record.operation);
      else plans.set(record.operation,plan);
      ready.delete(record.operation); continue;
    }
    const plan = plans.get(record.operation);
    assert.ok(plan,"Reply follows outbound action");
    const count=(counts.get(record.operation) ?? 0)+1;
    counts.set(record.operation,count);assert.ok(count <= 32,"Operation step bound");
    let step = hooks.runtimeResponse(ctx,entry.operation,plan.state,record.reply);
    if (step.kind === "continue" && step.history) chunks.set(record.operation,[...(chunks.get(record.operation) ?? []),step.history]);
    if (step.kind === "complete" && step.result.kind === "historyPage") step = {...step,result:collectHistory([...(chunks.get(record.operation) ?? []),step.result])};
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
assert.equal(names.length,11);
for (const name of names) check(`replay ${name}`,()=>replay(JSON.parse(fs.readFileSync(new URL(name,root),"utf8"))));
for (const name of fs.readdirSync(new URL("captured/",root)).filter(n=>n.endsWith(".json") && n!=="metrics.json").sort())
  check(`captured traffic ${name}`,()=>replay(JSON.parse(fs.readFileSync(new URL(`captured/${name}`,root),"utf8"))));
const sample = JSON.parse(fs.readFileSync(new URL("multi-turn.json",root),"utf8")) as Fixture;
const ctx = sample.context;
const submit = sample.operations[0].operation;
check("thread reverted evidence is scoped to the bound thread",()=> {
  const own=hooks.runtimeReduce(ctx,null,[{kind:"notification",method:"thread/reverted",params:{threadId:ctx.threadId}}]);
  assert.deepEqual(own.events,[{kind:"threadReverted",threadId:ctx.threadId}]);
  const foreign=hooks.runtimeReduce(ctx,null,[{kind:"notification",method:"thread/reverted",params:{threadId:"foreign-thread"}}]);
  assert.deepEqual(foreign.events,[]);
});
check("thread history refuses active turns before making a provider request",()=> {
  const operation:Operation={kind:"control",threadId:ctx.threadId,action:"threadHistory",value:{action:"revert",turnId:"turn-two"}};
  assert.throws(()=>hooks.runtimePlan({...ctx,activeTurnId:"turn-active"},operation),error=>
    error !== null && typeof error === "object" && "code" in error && "message" in error
      && error.code === "provider_refused" && /active turn/i.test(String(error.message)));
});
check("fork startup rejects a bound or active child runtime",()=> {
  const fixture=JSON.parse(fs.readFileSync(new URL("thread-fork.json",root),"utf8")) as Fixture;
  const operation=fixture.operations[0].operation;
  assert.throws(()=>hooks.runtimePlan({...fixture.context,threadId:"already-bound"},operation));
  assert.throws(()=>hooks.runtimePlan({...fixture.context,activeTurns:[{threadId:"active-thread",turnId:"active-turn"}]},operation));
});
check("recorded first replies follow binding without an injected developer item",()=> {
  const captured=JSON.parse(fs.readFileSync(new URL("captured/multi-turn.json",root),"utf8")) as Fixture;
  const opening=captured.operations.findIndex(entry=>entry.operation.kind === "openThread");
  const requests=captured.rpc.filter(record=>"operation" in record && record.operation === opening && record.direction === "outbound");
  assert.equal(requests.length,1);
  assert.ok("action" in requests[0] && requests[0].action.kind === "request");
  if (!("action" in requests[0]) || requests[0].action.kind !== "request")throw new Error("Expected start");
  assert.equal(requests[0].action.call.method,"thread/start");
  const replies=captured.rpc.flatMap(record=> {
    if (!("frame" in record) || record.frame.method !== "item/completed")return [];
    const params=record.frame.params as {item?:{type?:string;text?:string}};
    return params.item?.type === "agentMessage" ? [params.item.text] : [];
  });
  assert.deepEqual(replies,["ONE","TWO"]);
});
check("recorded compaction restores application context before the next input",()=> {
  const captured=JSON.parse(fs.readFileSync(new URL("captured/compaction.json",root),"utf8")) as Fixture;
  const controls=captured.operations.map(entry=>entry.operation).filter((op):op is Extract<Operation,{kind:"control"}>=>op.kind === "control");
  assert.deepEqual(controls.map(op=>op.action),["restoreContext","compact","restoreContext"]);
  assert.deepEqual(controls[0].value,controls[2].value);
  const replies=captured.rpc.flatMap(record=> {
    if (!("frame" in record) || record.frame.method !== "item/completed")return [];
    const params=record.frame.params as {item?:{type?:string;text?:string}};
    return params.item?.type === "agentMessage" ? [params.item.text] : [];
  });
  assert.deepEqual(replies,["ONE","SABLE"]);
});
check("manifest inbound notification roster",()=>assert.deepEqual(descriptor.agentRuntime.notificationMethods,[...notificationMethods]));
check("live activation matches the tested runtime declaration",()=> {
  const manifest = JSON.parse(fs.readFileSync(new URL("../plugin.json",import.meta.url),"utf8"));
  const declaration = JSON.parse(fs.readFileSync(new URL("./runtime-manifest.json",import.meta.url),"utf8"));
  assert.deepEqual(manifest.capabilities.agentRuntime,declaration.agentRuntime);
  assert.deepEqual(manifest.capabilities.chatBackend,declaration.chatBackend);
});
check("exact steer refusal only",()=> {
  const steer = {...submit,activeTurnId:"active"} as Operation;
  const context = {...ctx,activeTurnId:"active"};
  const plan = hooks.runtimePlan(context,steer);
  for (const error of [{code:-32600,message:"temporary error"},{code:-32600,message:"no active turn to steer",data:{retry:true}},{code:-32000,message:"no active turn to steer"}])
    assert.equal(hooks.runtimeResponse(context,steer,plan.state,{kind:"error",error}).kind,"refused");
  assert.equal(hooks.runtimeResponse(context,steer,plan.state,{kind:"success",value:{}}).kind,"complete");
});
check("taskless read-only verification turn maps through completion",()=> {
  const context:RuntimeContext = {...ctx,settings:{mode:"full-access"},providerSettings:{execution_intent:"read_only"},claimedSends:[],activeTurns:[],threadParents:[]};
  const threadId=context.threadId!;
  const frames:RuntimeFrame[] = [
    {kind:"notification",method:"turn/started",params:{threadId,turn:{id:"verify-turn",startedAt:100}}},
    {kind:"notification",method:"item/completed",params:{threadId,turnId:"verify-turn",item:{id:"intro",type:"agentMessage",text:"opaque-intro"}}},
    {kind:"notification",method:"item/started",params:{threadId,turnId:"verify-turn",item:{id:"exec",type:"dynamicToolCall",tool:"exec",arguments:{cmd:"git status --short"},status:"inProgress"}}},
    {kind:"notification",method:"item/completed",params:{threadId,turnId:"verify-turn",item:{id:"exec",type:"dynamicToolCall",tool:"exec",arguments:{cmd:"git status --short"},status:"completed",success:true,contentItems:[{type:"inputText",text:"opaque-result"}]}}},
    {kind:"notification",method:"item/started",params:{threadId,turnId:"verify-turn",item:{id:"command",type:"commandExecution",command:"git status --short",status:"inProgress"}}},
    {kind:"notification",method:"item/completed",params:{threadId,turnId:"verify-turn",item:{id:"command",type:"commandExecution",command:"git status --short",status:"completed",exitCode:0,aggregatedOutput:"opaque-command-result"}}},
    {kind:"notification",method:"item/completed",params:{threadId,turnId:"verify-turn",item:{id:"answer",type:"agentMessage",text:"opaque-answer"}}},
    {kind:"notification",method:"turn/completed",params:{threadId,turn:{id:"verify-turn",status:"completed",completedAt:101}}},
  ];
  let state:JsonValue=null;
  const events:AgentEvent[]=[];
  for(const frame of frames) {
    const reduced=hooks.runtimeReduce(context,state,[frame]);
    state=reduced.state;events.push(...reduced.events);
  }
  assert.ok(events.some(event=>event.kind === "chatItem" && event.item.id === "answer" && event.item.content === "opaque-answer"));
  assert.ok(events.some(event=>event.kind === "chatItem" && event.item.id === "command" && event.item.toolResult === "opaque-command-result"));
  assert.ok(events.some(event=>event.kind === "turnState" && event.turnId === "verify-turn" && event.state === "idle"));
});
check("numeric and string approval ids are distinct",()=> {
  assert.throws(()=>hooks.runtimePlan({...ctx,pendingRequest:{requestId:7,method:"item/commandExecution/requestApproval",params:{}}},
    {kind:"answer",requestId:"7",answer:{kind:"cancel"}}));
});
check("unsupported controls and modes refuse",()=> {
  assert.throws(()=>hooks.runtimePlan(ctx,{kind:"control",action:"globalConfig",value:{}}));
  assert.throws(()=>hooks.runtimePlan(ctx,{kind:"openThread",instructions:"",settings:{mode:"unknown"}}));
});
check("fresh thread binds from verified start without model input",()=> {
  const operation:Operation = {kind:"openThread",instructions:"Own instructions",settings:ctx.settings};
  const plan = hooks.runtimePlan(ctx,operation);
  assert.equal(plan.action.kind,"request");
  const step = hooks.runtimeResponse(ctx,operation,plan.state,{kind:"success",value:{thread:{id:"fresh"}}});
  assert.deepEqual(plain(step),{kind:"complete",result:{kind:"threadLoaded",threadId:"fresh",settings:ctx.settings}});
  for (const thread of [{}, {id:"foreign",parentThreadId:"parent"}, {id:"child",source:{subAgent:{}}}])
    assert.equal(hooks.runtimeResponse(ctx,operation,plan.state,{kind:"success",value:{thread}}).kind,"refused");
  assert.equal(hooks.runtimeResponse(ctx,operation,plan.state,{kind:"error",error:{code:-32000,message:"failed"}}).kind,"refused");
});
check("host-owned questions disable native input in fresh, resumed and forked threads",()=> {
  const hostOwned = {...ctx,hostOwned:{questions:true,todos:false}};
  const unbound = {...hostOwned,threadId:undefined,activeTurnId:undefined,activeTurns:[]};
  const open:Operation = {kind:"openThread",instructions:"Own instructions",settings:ctx.settings};
  assertHostOwnedConfig(hooks.runtimePlan(unbound,open),"thread/start");
  const resume:Operation = {kind:"resumeThread",instructions:"Resume instructions",threadId:ctx.threadId!,settings:ctx.settings};
  assertHostOwnedConfig(hooks.runtimePlan(hostOwned,resume),"thread/resume");
  const fork:Operation = {kind:"forkThread",sourceThreadId:"source-thread",turnId:"target-turn"};
  const forkPlan = hooks.runtimePlan(unbound,fork);
  const forkStep = hooks.runtimeResponse(unbound,fork,forkPlan.state,{kind:"success",value:{data:[{id:"target-turn",status:"completed"}],nextCursor:null}});
  assert.equal(forkStep.kind,"continue");
  if (forkStep.kind !== "continue") throw new Error("Expected fork request");
  assertHostOwnedConfig(forkStep.plan,"thread/fork");
  const control:Operation = {kind:"control",threadId:"source-thread",action:"threadHistory",value:{action:"fork",turnId:"target-turn"}};
  const controlPlan = hooks.runtimePlan(unbound,control);
  const controlStep = hooks.runtimeResponse(unbound,control,controlPlan.state,{kind:"success",value:{data:[{id:"target-turn",status:"completed"}],nextCursor:null}});
  assert.equal(controlStep.kind,"continue");
  if (controlStep.kind !== "continue") throw new Error("Expected control fork request");
  assertHostOwnedConfig(controlStep.plan,"thread/fork");
});
check("resume never accepts another thread",()=> {
  const operation:Operation = {kind:"resumeThread",instructions:"resume-instructions",threadId:ctx.threadId!,settings:ctx.settings};
  const plan = hooks.runtimePlan(ctx,operation);
  for (const thread of [{id:"foreign"},{id:ctx.threadId,parentThreadId:"parent"},{id:ctx.threadId,source:{subAgent:{}}}])
    assert.equal(hooks.runtimeResponse(ctx,operation,plan.state,{kind:"success",value:{thread}}).kind,"refused");
});
check("unmaterialized history verifies an empty own thread without injecting input",()=> {
  for (const order of ["asc","desc"] as const) {
    const op:Operation={kind:"readHistory",threadId:ctx.threadId!,order};
    const plan=hooks.runtimePlan(ctx,op);
    const step=hooks.runtimeResponse(ctx,op,plan.state,{kind:"error",error:{code:-32600,message:`thread ${ctx.threadId} is not materialized yet; thread/turns/list is unavailable before first user message`}});
    if(step.kind !== "continue")throw new Error("Expected identity verification");
    assert.deepEqual(plain(step.plan.action),{kind:"request",call:{method:"thread/read",params:{threadId:ctx.threadId,includeTurns:false}}});
    for(const thread of [{id:"foreign",turns:[]},{id:ctx.threadId},{id:ctx.threadId,turns:[{id:"existing"}]}])
      assert.equal(hooks.runtimeResponse(ctx,op,step.plan.state,{kind:"success",value:{thread}}).kind,"refused");
    assert.deepEqual(plain(hooks.runtimeResponse(ctx,op,step.plan.state,{kind:"success",value:{thread:{id:ctx.threadId,turns:[]}}})),{kind:"complete",result:{kind:"historyPage",threadId:ctx.threadId,items:[],openItems:[],order}});
  }
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
check("identical steering sends retain distinct protocol item identities",()=> {
  const claimedSends = ["repeat-a","repeat-b"].map(sendId=>({sendId,threadId:ctx.threadId!,input:{text:"same input",attachments:[]}}));
  const frames:RuntimeFrame[] = claimedSends.map((claim,index)=>({kind:"notification",method:"item/completed",params:{threadId:ctx.threadId!,turnId:"same-turn",item:{id:`user-${index}`,type:"userMessage",clientId:claim.sendId,content:[{type:"text",text:"same input"}]}}}));
  const events = hooks.runtimeReduce({...ctx,claimedSends},null,frames).events.filter(e=>e.kind === "sendEvidence");
  assert.deepEqual(events,claimedSends.map((claim,index)=>({kind:"sendEvidence",threadId:ctx.threadId!,sendId:claim.sendId,turnId:"same-turn",itemId:`user-${index}`})));
  const missingId:RuntimeFrame = {kind:"notification",method:"item/completed",params:{threadId:ctx.threadId!,turnId:"same-turn",item:{type:"userMessage",clientId:"repeat-a",content:[{type:"text",text:"same input"}]}}};
  assert.deepEqual(hooks.runtimeReduce({...ctx,claimedSends},null,[missingId]).events.filter(e=>e.kind === "sendEvidence"),[{kind:"sendEvidence",threadId:ctx.threadId!,sendId:"repeat-a",turnId:"same-turn"}]);
});
check("claim evidence is emitted once",()=> {
  const entry = sample.rpc.find(r=>"frame" in r && r.frame.method === "item/completed")!;
  assert.ok("frame" in entry);
  const reduction = hooks.runtimeReduce(ctx,null,[entry.frame,entry.frame]);
  assert.equal(reduction.events.filter(e=>e.kind === "sendEvidence").length,1);
});
check("delta batches preserve final state and publish only the final row",()=> {
  const frames:RuntimeFrame[] = ["a","b","c"].map(delta=>({kind:"notification",method:"item/agentMessage/delta",params:{threadId:ctx.threadId!,turnId:"turn-a",itemId:"stream",delta}}));
  const batch = hooks.runtimeReduce(ctx,null,frames);
  let state:JsonValue = null;const events:AgentEvent[]=[];
  for (const frame of frames) {const step=hooks.runtimeReduce(ctx,state,[frame]);state=step.state;events.push(...step.events);}
  assert.deepEqual(plain(batch.state),plain(state));
  assert.deepEqual(plain(batch.events),plain(events.slice(-1)));
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
console.log(`runtime hooks: ${count} checks passed (9 shared scenarios)`);
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
  const captured = JSON.parse(fs.readFileSync(new URL("./fixtures/captured-live-edit.json", import.meta.url), "utf8"));
  const capturedEvent = hooks.runtimeReduce(ctx,null,[{kind:"notification",method:"item/completed",params:{threadId:ctx.threadId!,turnId:"turn-a",item:captured}}]).events[0];
  assert.ok(capturedEvent.kind === "chatItem");
  assert.equal((capturedEvent.item.fileDiffs as {additions:number}[])[0].additions,42);
  const unknown = hooks.runtimeReduce(ctx,null,[{kind:"notification",method:"item/completed",params:{threadId:ctx.threadId!,turnId:"turn-a",item:{id:"unknown",type:"fileChange",status:"completed",changes:{opaque:{value:17}}}}}]).events[0];
  assert.ok(unknown.kind === "chatItem");
  assert.ok(unknown.item.toolDetail?.includes('"value": 17'));
  assert.equal(unknown.item.toolSummary,"File change");
  const diff="@@ -1 +1 @@\n-old\n+new\n";
  const frame:RuntimeFrame={kind:"notification",method:"item/completed",params:{threadId:ctx.threadId!,turnId:"turn-a",item:{id:"file",type:"fileChange",status:"completed",changes:[{path:"file.txt",kind:{type:"update"},diff}]}}};
  const result=hooks.runtimeReduce(ctx,null,[frame]);const event=result.events[0];assert.ok(event.kind === "chatItem");
  assert.deepEqual(event.item.fileDiffs && plain(event.item.fileDiffs),[{path:"file.txt",kind:"modify",additions:1,deletions:1,hunks:[{oldStart:1,oldLines:1,newStart:1,newLines:1,lines:["-old","+new"]}]}]);
  const turn=hooks.runtimeReduce(ctx,null,[{kind:"notification",method:"turn/diff/updated",params:{threadId:ctx.threadId!,turnId:"turn-a",diff:`diff --git a/file.txt b/file.txt\n--- a/file.txt\n+++ b/file.txt\n${diff}`}}]).events[0];
  assert.ok(turn.kind === "chatItem");assert.equal(turn.item.fileDiffs,undefined);
  assert.equal(turn.item.toolResult,`diff --git a/file.txt b/file.txt\n--- a/file.txt\n+++ b/file.txt\n${diff}`);
  const changes=sessionChanges([event.item,turn.item] as NormalizedChatMessage[]);
  assert.equal(changes.cumulative.length,1);
  assert.equal(changes.cumulative[0].additions,1);assert.equal(changes.cumulative[0].deletions,1);
});
console.log(`runtime expanded hooks: ${count} checks passed`);
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
  assert.deepEqual(plain(hooks.runtimeReduce({...ctx,threadId:undefined},null,[{kind:"notification",method:"account/updated",params:{authMode:"chatgpt"}}]).events),[{kind:"auth",state:"signedIn",billingMode:"subscription"}]);
});
check("retry errors remain nonterminal with classified source metadata",()=> {
  const events=hooks.runtimeReduce(ctx,null,[{kind:"notification",method:"error",params:{threadId:ctx.threadId!,turnId:"turn-a",willRetry:true,error:{message:"Retrying",codexErrorInfo:{httpConnectionFailed:{httpStatusCode:503}}}}}]).events;
  assert.equal(events.length,1);const event=events[0];assert.ok(event.kind === "chatItem");assert.equal(event.item.systemSubtype,"provider_retry");assert.deepEqual(plain(event.item.toolInput),{code:"httpConnectionFailed",class:"server_overloaded",retryable:true,retryableKind:"transient"});
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
    const chunk=step.history!;
    step=hooks.runtimeResponse(ctx,operation,step.plan.state,{kind:"success",value:{data:[{item:answerItem}]}});
    assert.ok(step.kind === "complete" && step.result.kind === "historyPage");
    return collectHistory([chunk,step.result]);
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
check("display JSON is identical for recursively reordered frame keys",()=> {
  const frame:RuntimeFrame={kind:"notification",method:"item/completed",params:{threadId:ctx.threadId!,turnId:"turn-a",item:{id:"ordered",type:"mcpToolCall",server:"fixture",tool:"lookup",status:"completed",
    arguments:{z:{b:2,a:1},a:[{z:4,a:3}],"2":"two","10":"ten"},result:{content:[{type:"text",text:"Found"}],metadata:{z:2,a:1},isError:false}}}};
  function reorder(value:JsonValue):JsonValue {
    if(Array.isArray(value))return value.map(reorder);
    if(value !== null && typeof value === "object")return Object.fromEntries(Object.entries(value).reverse().map(([key,entry])=>[key,reorder(entry)]));
    return value;
  }
  const reversed:RuntimeFrame={...frame,params:reorder(frame.params)};
  const first=hooks.runtimeReduce(ctx,null,[frame]).events[0];const second=hooks.runtimeReduce(ctx,null,[reversed]).events[0];
  assert.ok(first.kind === "chatItem" && second.kind === "chatItem");
  assert.deepEqual(plain(first),plain(second));
  assert.equal(first.item.toolSummary,'{\n  "10": "ten",\n  "2": "two",\n  "a": [\n    {\n      "a": 3,\n      "z": 4\n    }\n  ],\n  "z": {\n    "a": 1,\n    "b": 2\n  }\n}');
  assert.equal(first.item.toolArgs,first.item.toolSummary);
  assert.equal(first.item.toolResult,'{\n  "content": [\n    {\n      "text": "Found",\n      "type": "text"\n    }\n  ],\n  "isError": false,\n  "metadata": {\n    "a": 1,\n    "z": 2\n  }\n}');
  const prompt:RuntimeFrame={kind:"request",requestId:"permissions",method:"item/permissions/requestApproval",params:{threadId:ctx.threadId!,permissions:{z:{b:2,a:1},a:3}}};
  assert.deepEqual(plain(hooks.runtimeReduce(ctx,null,[prompt]).events),plain(hooks.runtimeReduce(ctx,null,[{...prompt,params:reorder(prompt.params)}]).events));
});
check("structured A12 launch options preserve PTY settings",()=> {
  const settings={model:"chosen-model",effort:"high",mode:"full-access",additionalDirectories:["/work/shared"]};
  const context={...ctx,settings};
  const thread=plain(hooks.runtimePlan(context,{kind:"openThread",instructions:"task",settings}));
  assert.equal(thread.action.kind,"request");
  if(thread.action.kind !== "request")throw new Error("Expected thread request");
  assert.deepEqual(thread.action.call.params,{cwd:ctx.cwd,model:"chosen-model",threadSource:"user",config:{check_for_update_on_startup:false,project_doc_max_bytes:65536,"sandbox_workspace_write.writable_roots":["/work/shared"],model_reasoning_effort:"high"},approvalPolicy:"never",approvalsReviewer:"user",sandbox:"danger-full-access",developerInstructions:"task"});
  const workspace={...settings,mode:"auto-accept-edits"};
  const op={...submit,settings:workspace} as Extract<Operation,{kind:"submitTurn"}>;
  const turn=plain(hooks.runtimePlan({...ctx,settings:workspace},op));
  assert.equal(turn.action.kind,"request");
  if(turn.action.kind !== "request")throw new Error("Expected turn request");
  assert.deepEqual((turn.action.call.params as Record<string,JsonValue>).sandboxPolicy,{type:"workspaceWrite",writableRoots:["/work/shared"]});
  const resume=hooks.runtimePlan(context,{kind:"resumeThread",instructions:"resume-instructions",threadId:ctx.threadId!,settings});
  assert.equal(resume.action.kind,"request");
  if(resume.action.kind !== "request")throw new Error("Expected resume request");
  assert.equal((resume.action.call.params as Record<string,JsonValue>).sandbox,"danger-full-access");
});
check("provider settings cannot replace the authorized launch mode",()=> {
  const context={...ctx,providerSettings:{runtimeSettings:{sandbox:"danger-full-access",approvalPolicy:"never",config:{sandbox_mode:"danger-full-access"}},args:["--dangerously-bypass-approvals-and-sandbox"]}};
  const plan=plain(hooks.runtimePlan(context,{kind:"openThread",settings:{mode:"approval-required"},instructions:"task"}));
  if(plan.action.kind !== "request")throw new Error("Expected request");
  const params=plan.action.call.params as Record<string,JsonValue>;
  assert.equal(params.sandbox,"read-only");assert.equal(params.approvalPolicy,"untrusted");
  assert.deepEqual(params.config,{check_for_update_on_startup:false,project_doc_max_bytes:65536});
});
check("A12b workspace-write network settings preserve false and provider defaults",()=> {
  for(const networkAccess of [true,false,undefined]) {
    const settings={mode:"auto-accept-edits",additionalDirectories:["/work/shared"],...(networkAccess !== undefined ? {networkAccess} : {})};
    const context={...ctx,settings};
    for(const op of [{kind:"openThread",instructions:"task",settings},{kind:"resumeThread",instructions:"resume-instructions",threadId:ctx.threadId!,settings}] as Operation[]) {
      const plan=plain(hooks.runtimePlan(context,op));
      if(plan.action.kind !== "request")throw new Error("Expected thread request");
      const params=plan.action.call.params as Record<string,JsonValue>;
      const config=params.config as Record<string,JsonValue>;
      assert.equal(config["sandbox_workspace_write.network_access"],networkAccess);
      assert.equal(Object.prototype.hasOwnProperty.call(config,"sandbox_workspace_write.network_access"),networkAccess !== undefined);
      assert.equal(params.sandbox,"workspace-write");
    }
    const plan=plain(hooks.runtimePlan(context,{...submit,settings} as Operation));
    if(plan.action.kind !== "request")throw new Error("Expected turn request");
    const policy=(plan.action.call.params as Record<string,JsonValue>).sandboxPolicy as Record<string,JsonValue>;
    assert.deepEqual(policy,{type:"workspaceWrite",writableRoots:["/work/shared"],...(networkAccess !== undefined ? {networkAccess} : {})});
  }
  for(const mode of ["approval-required","full-access"]) {
    const settings={mode,networkAccess:true};
    const context={...ctx,settings};
    const plan=plain(hooks.runtimePlan(context,{kind:"openThread",instructions:"task",settings}));
    if(plan.action.kind !== "request")throw new Error("Expected thread request");
    const config=(plan.action.call.params as Record<string,JsonValue>).config as Record<string,JsonValue>;
    assert.equal(Object.prototype.hasOwnProperty.call(config,"sandbox_workspace_write.network_access"),false);
    const turn=plain(hooks.runtimePlan(context,{...submit,settings} as Operation));
    if(turn.action.kind !== "request")throw new Error("Expected turn request");
    const policy=(turn.action.call.params as Record<string,JsonValue>).sandboxPolicy as Record<string,JsonValue>;
    assert.equal(Object.prototype.hasOwnProperty.call(policy,"networkAccess"),false);
  }
});
check("initialize negotiates experimental history before planning item pagination",()=> {
  const op:Operation={kind:"initialize"};
  let plan=plain(hooks.runtimePlan({...ctx,providerSettings:{includeTurnDiffs:false}},op));
  assert.deepEqual(plan.action,{kind:"request",call:{method:"initialize",params:{clientInfo:{name:"domios",version:ctx.hostVersion},capabilities:{experimentalApi:true,optOutNotificationMethods:[]}}}});
  let step=hooks.runtimeResponse(ctx,op,plan.state,{kind:"success",value:{}});
  assert.equal(step.kind,"continue");if(step.kind !== "continue")throw new Error("Expected notification");
  plan=plain(step.plan);assert.equal(plan.action.kind,"notification");
  step=hooks.runtimeResponse(ctx,op,plan.state,{kind:"success",value:null});
  if(step.kind !== "continue")throw new Error("Expected account read");
  plan=plain(step.plan);assert.equal(plan.action.kind,"request");
  step=hooks.runtimeResponse(ctx,op,plan.state,{kind:"success",value:{account:{type:"chatgpt"}}});
  assert.equal(step.kind,"continue");
  if(step.kind !== "continue") throw new Error("Expected current limits read");
  step=hooks.runtimeResponse(ctx,op,step.plan.state,{kind:"success",value:{rateLimits:{}}});
  assert.equal(step.kind,"complete");
  const history:Operation={kind:"readHistory",threadId:ctx.threadId!};
  plan=plain(hooks.runtimePlan(ctx,history));
  if(plan.action.kind !== "request")throw new Error("Expected history request");
  assert.equal(plan.action.call.method,"thread/turns/list");
  step=hooks.runtimeResponse(ctx,history,plan.state,{kind:"success",value:{data:[{id:"turn",status:"completed"}],nextCursor:null}});
  if(step.kind !== "continue" || step.plan.action.kind !== "request")throw new Error("Expected experimental item pagination");
  assert.equal(step.plan.action.call.method,"thread/items/list");
});
function readTurnPage(op:Extract<Operation,{kind:"readHistory"}>, turns:string[], nextCursor:string|null) {
  const plan=plain(hooks.runtimePlan(ctx,op));
  if(plan.action.kind !== "request")throw new Error("Expected turn page request");
  assert.equal(plan.action.call.method,"thread/turns/list");
  const params=plan.action.call.params as Record<string,JsonValue>;
  let step=hooks.runtimeResponse(ctx,op,plan.state,{kind:"success",value:{data:turns.map(id=>({id,status:"completed"})),nextCursor}});
  const requested:JsonValue[]=[];
  const chunks:OperationResult[]=[];
  while(step.kind === "continue") {
    if(step.history)chunks.push(step.history);
    const action=plain(step.plan.action);
    if(action.kind !== "request")throw new Error("Expected page request");
    if(action.call.method === "thread/goal/get") {
      step=hooks.runtimeResponse(ctx,op,step.plan.state,{kind:"error",error:{code:-32601,message:"unsupported goal"}});
      continue;
    }
    assert.equal(action.call.method,"thread/items/list");
    const turn=(action.call.params as Record<string,JsonValue>).turnId;
    requested.push(turn);
    step=hooks.runtimeResponse(ctx,op,step.plan.state,{kind:"success",value:{data:[{type:"agentMessage",id:`${String(turn)}-reply`,text:String(turn)}],nextCursor:null}});
  }
  const complete=plain(step);
  if(complete.kind !== "complete" || complete.result.kind !== "historyPage")throw new Error("Expected history page");
  return {params,requested,result:complete.result,items:collectHistory([...chunks,complete.result]).items.map(item=>item.id)};
}
check("history reads bounded newest turn pages and echoes descending order",()=> {
  const op:Extract<Operation,{kind:"readHistory"}>={kind:"readHistory",threadId:ctx.threadId!,order:"desc"};
  const first=readTurnPage(op,["newer"],"page-2");
  assert.equal(first.params.sortDirection,"desc");
  assert.equal(first.params.limit,1);
  assert.deepEqual(first.requested,["newer"]);
  assert.deepEqual(first.items,["newer-reply"]);
  assert.ok(first.result.nextCursor);
  assert.equal(first.result.order,"desc");
  const next=readTurnPage({...op,cursor:first.result.nextCursor!},["older"],null);
  assert.equal(next.params.cursor,"page-2");
  assert.equal(next.params.sortDirection,"desc");
  assert.equal(next.result.order,"desc");
});
check("history without a requested order stays ascending and echoes no order",()=> {
  const op:Extract<Operation,{kind:"readHistory"}>={kind:"readHistory",threadId:ctx.threadId!};
  const page=readTurnPage(op,["older","newer"],null);
  assert.equal(page.params.sortDirection,"asc");
  assert.deepEqual(page.requested,["older","newer"]);
  assert.deepEqual(page.items,["older-reply","newer-reply"]);
  assert.equal(Object.prototype.hasOwnProperty.call(page.result,"order"),false);
});
check("unsupported item pagination completes startup without requesting full history",()=> {
  const op:Operation={kind:"readHistory",threadId:ctx.threadId!};
  const plan=hooks.runtimePlan(ctx,op);
  const turns=hooks.runtimeResponse(ctx,op,plan.state,{kind:"success",value:{data:[{id:"turn",status:"completed"}],nextCursor:null}});
  if(turns.kind !== "continue")throw new Error("Expected item request");
  const step=plain(hooks.runtimeResponse(ctx,op,turns.plan.state,{kind:"error",error:{code:-32601,message:"unsupported"}}));
  assert.deepEqual(step,{kind:"complete",result:{kind:"historyPage",threadId:ctx.threadId!,items:[],openItems:[]}});
});
check("default model and effective effort survive thread start and resume verification",()=> {
  const context={...ctx,settings:{},providerSettings:{model:null}};
  const op:Operation={kind:"openThread",instructions:"task",settings:{additionalDirectories:["/work/shared"],networkAccess:false}};
  const plan=hooks.runtimePlan(context,op);
  const effective={thread:{id:ctx.threadId!},model:"provider-default",reasoningEffort:"medium",approvalPolicy:"on-request",approvalsReviewer:"user",sandbox:{type:"workspaceWrite"}};
  const step=hooks.runtimeResponse(context,op,plan.state,{kind:"success",value:effective});
  const complete=plain(step);
  assert.deepEqual(complete,{kind:"complete",result:{kind:"threadLoaded",threadId:ctx.threadId!,settings:{additionalDirectories:["/work/shared"],networkAccess:false,model:"provider-default",effort:"medium",mode:"auto-accept-edits"}}});
  const resume:Operation={kind:"resumeThread",instructions:"resume-instructions",threadId:ctx.threadId!,settings:{model:"requested-alias",effort:"high",mode:"full-access",additionalDirectories:["/work/shared"],networkAccess:false}};
  const resumed=hooks.runtimeResponse(context,resume,hooks.runtimePlan(context,resume).state,{kind:"success",value:effective});
  if(resumed.kind !== "continue")throw new Error("Expected identity verification");
  const verified=plain(hooks.runtimeResponse(context,resume,resumed.plan.state,{kind:"success",value:{thread:{id:ctx.threadId!,model:"stale-snapshot-model"}}}));
  assert.deepEqual(verified,complete);
  const omitted=hooks.runtimeResponse(context,resume,hooks.runtimePlan(context,resume).state,{kind:"success",value:{thread:{id:ctx.threadId!},model:null,reasoningEffort:null}});
  if(omitted.kind !== "continue")throw new Error("Expected verification");
  const fallback=plain(hooks.runtimeResponse(context,resume,omitted.plan.state,{kind:"success",value:{thread:{id:ctx.threadId!}}}));
  if(fallback.kind !== "complete" || fallback.result.kind !== "threadLoaded")throw new Error("Expected loaded settings");
  assert.deepEqual(fallback.result.settings,resume.settings);
});
check("settings controls confirm provider values and retain omitted requested fields",()=> {
  const op:Operation={kind:"control",threadId:ctx.threadId!,action:"model",value:"requested-alias"};
  const plan=hooks.runtimePlan({...ctx,settings:{effort:"high",additionalDirectories:["/work/shared"],networkAccess:false}},op);
  const reply={threadSettings:{model:"effective-model",effort:"low",approvalPolicy:"never",sandboxPolicy:{type:"dangerFullAccess"}}};
  const complete=plain(hooks.runtimeResponse(ctx,op,plan.state,{kind:"success",value:reply}));
  assert.deepEqual(complete,{kind:"complete",result:{kind:"done"},events:[{kind:"controlsConfirmed",threadId:ctx.threadId!,settings:{model:"effective-model",effort:"low",mode:"full-access",additionalDirectories:["/work/shared"],networkAccess:false}}]});
  const partial=plain(hooks.runtimeResponse(ctx,op,plan.state,{kind:"success",value:{reasoningEffort:"medium"}}));
  if(partial.kind !== "complete")throw new Error("Expected control completion");
  const event=partial.events?.[0];if(event?.kind !== "controlsConfirmed")throw new Error("Expected reported settings");
  assert.equal(event.settings.model,"requested-alias");assert.equal(event.settings.effort,"medium");
  const empty=plain(hooks.runtimeResponse(ctx,op,plan.state,{kind:"success",value:{}}));
  assert.deepEqual(empty,{kind:"complete",result:{kind:"done"},events:[{kind:"controlsConfirmed",threadId:ctx.threadId!,settings:{model:"requested-alias",effort:"high",additionalDirectories:["/work/shared"],networkAccess:false}}]});
});
check("resume refreshes instructions on direct and unarchive retry paths",()=> {
  const instructions='opaque-resume-contract\n${opaque} "value"';
  for(const value of [instructions,""]) {
    const op:Operation={kind:"resumeThread",threadId:ctx.threadId!,instructions:value,settings:ctx.settings};
    const plan=plain(hooks.runtimePlan(ctx,op));
    if(plan.action.kind !== "request")throw new Error("Expected resume request");
    assert.equal((plan.action.call.params as Record<string,JsonValue>).developerInstructions,value);
    const archived=hooks.runtimeResponse(ctx,op,plan.state,{kind:"error",error:{code:-32600,message:`session ${ctx.threadId!} is archived`}});
    if(archived.kind !== "continue")throw new Error("Expected unarchive");
    const retry=plain(hooks.runtimeResponse(ctx,op,archived.plan.state,{kind:"success",value:{}}));
    if(retry.kind !== "continue" || retry.plan.action.kind !== "request")throw new Error("Expected resumed retry");
    assert.equal((retry.plan.action.call.params as Record<string,JsonValue>).developerInstructions,value);
  }
});
check("restoreContext validates and injects developer and user context without evidence",()=> {
  const instructions='opaque-restored-contract\n${opaque} "value"';
  const value={instructions,additionalContext:[{key:"app",kind:"application",value:"application-context"},{key:"user",kind:"user",value:"user-context"}]};
  const op:Operation={kind:"control",threadId:ctx.threadId!,action:"restoreContext",value};
  const plan=plain(hooks.runtimePlan(ctx,op));
  assert.deepEqual(plan.action,{kind:"request",call:{method:"thread/inject_items",params:{threadId:ctx.threadId!,items:[
    {type:"message",role:"developer",content:[{type:"input_text",text:instructions}]},
    {type:"message",role:"developer",content:[{type:"input_text",text:"application-context"}]},
    {type:"message",role:"user",content:[{type:"input_text",text:"user-context"}]},
  ]}}});
  assert.deepEqual(plain(hooks.runtimeResponse(ctx,op,plan.state,{kind:"success",value:{}})),{kind:"complete",result:{kind:"done"}});
  assert.deepEqual(plain(hooks.runtimePlan(ctx,{...op,value:{...value,additionalContext:[...value.additionalContext,...value.additionalContext]}})),plan);
  for(const invalid of [null,{},[],{instructions:null,additionalContext:[]},{instructions:"",additionalContext:[]},{instructions:42,additionalContext:[]},{instructions:"x",additionalContext:{}},{instructions:"x",additionalContext:[{key:"k",kind:"system",value:"x"}]},{instructions:"x",additionalContext:[{key:"k",kind:"user",value:"x",extra:true}]},{instructions:"x",additionalContext:[{key:"k",kind:"user",value:"x"},{key:"k",kind:"user",value:"y"}]}]) {
    assert.throws(()=>hooks.runtimePlan(ctx,{...op,value:invalid} as Operation),error=>objectError(error).code === "invalid_schema");
  }
  const contextOnly=plain(hooks.runtimePlan(ctx,{...op,value:{instructions:null,additionalContext:[{key:"one",kind:"user",value:"payload"}]}}));
  if(contextOnly.action.kind !== "request")throw new Error("Expected context injection");
  assert.deepEqual((contextOnly.action.call.params as Record<string,JsonValue>).items,[{type:"message",role:"user",content:[{type:"input_text",text:"payload"}]}]);
});
function objectError(value:unknown):{code?:string} {return value as {code?:string};}
check("settings success acknowledges requests and errors cannot confirm them",()=> {
  const current={model:"old",effort:"high",mode:"auto-accept-edits",additionalDirectories:["/work/shared"],networkAccess:false};
  for(const [action,value,fields] of [["model","new-model",{model:"new-model"}],["effort","low",{effort:"low"}],["mode","full-access",{mode:"full-access"}],["settings",{model:"new-model",effort:"low"},{model:"new-model",effort:"low"}]] as const) {
    const context={...ctx,settings:current};
    const op:Operation={kind:"control",threadId:ctx.threadId!,action,value};
    const plan=hooks.runtimePlan(context,op);
    const success=plain(hooks.runtimeResponse(context,op,plan.state,{kind:"success",value:{}}));
    assert.deepEqual(success,{kind:"complete",result:{kind:"done"},events:[{kind:"controlsConfirmed",threadId:ctx.threadId!,settings:{...current,...fields}}]});
    const error=plain(hooks.runtimeResponse(context,op,plan.state,{kind:"error",error:{code:-32600,message:"refused"}}));
    assert.equal(error.kind,"refused");assert.equal("events" in error,false);
  }
});
check("turn start reconciles effective model and effort after control acknowledgement",()=> {
  const frame:RuntimeFrame={kind:"notification",method:"turn/started",params:{threadId:ctx.threadId!,turn:{id:"new-turn",model:"effective-model",reasoningEffort:"low",status:"inProgress"}}};
  const events=plain(hooks.runtimeReduce(ctx,null,[frame]).events);
  assert.deepEqual(events,[{kind:"controlsConfirmed",threadId:ctx.threadId!,settings:{model:"effective-model",effort:"low"}},{kind:"turnState",threadId:ctx.threadId!,turnId:"new-turn",state:"working"}]);
});
console.log(`runtime canonical JSON hooks: ${count} checks passed`);

check("unsupported turn pagination completes startup only on method absence",()=> {
  const op:Operation={kind:"readHistory",threadId:ctx.threadId!};
  const plan=hooks.runtimePlan(ctx,op);
  const step=plain(hooks.runtimeResponse(ctx,op,plan.state,{kind:"error",error:{code:-32601,message:"unsupported"}}));
  assert.deepEqual(step,{kind:"complete",result:{kind:"historyPage",threadId:ctx.threadId!,items:[],openItems:[]}});
  assert.equal(hooks.runtimeResponse(ctx,op,plan.state,{kind:"error",error:{code:-32000,message:"Permission denied"}}).kind,"refused");
});
check("archived resume recovery is case insensitive and bounded",()=> {
  const op:Operation={kind:"resumeThread",threadId:ctx.threadId!,settings:ctx.settings};
  const plan=hooks.runtimePlan(ctx,op);
  for(const message of ["Session saved is archived", "Run CODEX UNARCHIVE to recover"]) {
    const first=plain(hooks.runtimeResponse(ctx,op,plan.state,{kind:"error",error:{code:-32000,message}}));
    if(first.kind !== "continue")throw new Error("Expected unarchive");
    assert.deepEqual(first.plan.action,{kind:"request",call:{method:"thread/unarchive",params:{threadId:ctx.threadId!}}});
    const second=hooks.runtimeResponse(ctx,op,first.plan.state,{kind:"success",value:{}});
    if(second.kind !== "continue")throw new Error("Expected resume");
    assert.equal(hooks.runtimeResponse(ctx,op,second.plan.state,{kind:"error",error:{code:-32000,message}}).kind,"refused");
  }
  for(const message of ["session invalid", "An item is archived", "codex unarchiveable"]) {
    assert.equal(hooks.runtimeResponse(ctx,op,plan.state,{kind:"error",error:{code:-32000,message}}).kind,"refused");
  }
});
check("item snapshots retain streamed output only when the final field is absent",()=> {
  for(const type of ["agentMessage", "commandExecution"]) {
    const method=type === "agentMessage" ? "item/agentMessage/delta" : "item/commandExecution/outputDelta";
    const frame:RuntimeFrame={kind:"notification",method,params:{threadId:ctx.threadId!,turnId:"turn-a",itemId:"streamed",delta:"kept"}};
    const completed:RuntimeFrame={kind:"notification",method:"item/completed",params:{threadId:ctx.threadId!,turnId:"turn-a",item:{id:"streamed",type,status:"completed"}}};
    const result=hooks.runtimeReduce(ctx,null,[frame,completed]);
    const row=result.events.filter(event=>event.kind === "chatItem").at(-1);if(row?.kind !== "chatItem")throw new Error("Expected item");
    assert.equal(type === "agentMessage" ? row.item.content : row.item.toolResult,"kept");
    const final={...completed,params:{threadId:ctx.threadId!,turnId:"turn-a",item:{id:"streamed",type,status:"completed",[type === "agentMessage" ? "text" : "aggregatedOutput"]:""}}} as RuntimeFrame;
    const authoritative=hooks.runtimeReduce(ctx,null,[frame,final]).events.filter(event=>event.kind === "chatItem").at(-1);
    if(authoritative?.kind !== "chatItem")throw new Error("Expected final item");
    assert.equal(type === "agentMessage" ? authoritative.item.content : authoritative.item.toolResult,"");
  }
});
check("terminal turn errors retain classified failure metadata without a prior error notification",()=> {
  const result=hooks.runtimeReduce(ctx,null,[{kind:"notification",method:"turn/completed",params:{threadId:ctx.threadId!,turn:{id:"turn-a",status:"failed",error:{message:"Quota exhausted",codexErrorInfo:"usageLimitExceeded"}}}}]);
  assert.equal(result.events.length,2);
  const row=result.events[0];if(row.kind !== "chatItem")throw new Error("Expected error item");
  assert.equal(row.item.content,"Quota exhausted");
  assert.deepEqual(plain(row.item.toolInput),{code:"usageLimitExceeded",class:"usage_limit",retryable:false});
  assert.deepEqual(plain(result.events[1]),{kind:"turnState",threadId:ctx.threadId!,turnId:"turn-a",state:"idle",detail:"failed"});
});
check("failed turns map each declared provider class once",()=> {
  const failed=(error:Record<string,unknown>)=>hooks.runtimeReduce(ctx,null,[{kind:"notification",method:"turn/completed",params:{threadId:ctx.threadId!,turn:{id:"turn-a",status:"failed",error}}}]);
  const klass=(error:Record<string,unknown>)=>{
    const row=failed(error).events[0];
    if(row.kind !== "chatItem")throw new Error("Expected error item");
    return plain(row.item.toolInput).class;
  };
  assert.equal(klass({message:"Quota exhausted",codexErrorInfo:"usageLimitExceeded"}),"usage_limit");
  assert.equal(klass({message:"Slow down",codexErrorInfo:"rateLimitExceeded"}),"rate_limit");
  assert.equal(klass({message:"Busy",codexErrorInfo:"serverOverloaded"}),"server_overloaded");
  assert.equal(klass({message:"Busy",codexErrorInfo:{httpConnectionFailed:{httpStatusCode:503}}}),"server_overloaded");
  assert.equal(klass({message:"Expired",codexErrorInfo:"token_expired"}),"auth_expiry");
  assert.equal(klass({message:"Denied",status:403,codexErrorInfo:"usageLimitExceeded"}),"authentication");
  assert.equal(klass({message:"Down",codexErrorInfo:"httpConnectionFailed"}),"transport_error");
});
console.log(`runtime comparison regressions: ${count} checks passed`);

check("verified descendant approvals surface on the root without adopting the child turn",()=> {
  const context={...ctx,activeTurnId:"root-turn",activeTurns:[{threadId:ctx.threadId!,turnId:"root-turn"},{threadId:"child",turnId:"child-turn",parentThreadId:ctx.threadId!}],threadParents:[{threadId:"child",parentThreadId:ctx.threadId!}]};
  for(const method of ["item/commandExecution/requestApproval", "item/fileChange/requestApproval", "item/permissions/requestApproval", "item/tool/requestUserInput", "mcpServer/elicitation/request"]) {
    const params={threadId:"child",turnId:"child-turn",command:"tool",permissions:{},mode:"form",requestedSchema:{},questions:[{id:"q",question:"Pick",options:[{label:"Yes"}]}]};
    const request:RuntimeFrame={kind:"request",method,requestId:37,params};
    const result=hooks.runtimeReduce(context,null,[request]);
    assert.equal(result.events.length,2);
    const prompt=result.events[0];if(prompt.kind !== "prompt")throw new Error("Expected root prompt");
    assert.equal(prompt.threadId,ctx.threadId);assert.equal(prompt.requestId,37);
    assert.deepEqual(plain(result.events[1]),{kind:"turnState",threadId:ctx.threadId!,state:"needs_input",detail:method.endsWith("/requestApproval") ? "awaitingApproval" : "awaitingInput",turnId:"root-turn"});
    const value=method === "item/permissions/requestApproval" ? "deny" : method === "item/tool/requestUserInput" ? "Yes" : "decline";
    const answer=plain(hooks.runtimePlan({...context,pendingRequest:{requestId:37,method,params}},{kind:"answer",requestId:37,answer:{kind:"choice",values:[value]}}));
    assert.equal(answer.action.kind,"reply");
    if(answer.action.kind !== "reply")throw new Error("Expected exact reply");
    assert.equal(answer.action.requestId,37);
    assert.deepEqual(plain(hooks.runtimeReduce(ctx,null,[request]).events),[]);
  }
  const resolved:RuntimeFrame={kind:"notification",method:"serverRequest/resolved",params:{threadId:"child",requestId:37}};
  assert.deepEqual(plain(hooks.runtimeReduce(context,null,[resolved]).events),[{kind:"promptResolved",threadId:ctx.threadId!,requestId:37}]);
  assert.deepEqual(plain(hooks.runtimeReduce(ctx,null,[resolved]).events),[]);
});
console.log(`runtime descendant prompt regressions: ${count} checks passed`);

check("browser and device login use verified requests without a thread",()=>{
  for (const [method,type] of [["browser","chatgpt"],["device","chatgptDeviceCode"]]) {
    const op:Operation={kind:"control",action:"login",value:{method}};
    const context={...ctx,threadId:undefined};
    const plan=hooks.runtimePlan(context,op);allowed(op,plan.action);
    assert.deepEqual(plain(plan.action),{kind:"request",call:{method:"account/login/start",params:{type}}});
    const response=type === "chatgpt" ? {type,loginId:"fixture-login",authUrl:"https://example.test/oauth"} : {type,loginId:"fixture-login",verificationUrl:"https://example.test/device",userCode:"ABCD-1234"};
    const step=hooks.runtimeResponse(context,op,plan.state,{kind:"success",value:response});
    assert.equal(step.kind,"complete");
    if (step.kind === "complete") assert.deepEqual(plain(step.events),[{kind:"authLogin",login:{status:"pending",method,loginId:"fixture-login",url:type === "chatgpt" ? "https://example.test/oauth" : "https://example.test/device",...(method === "device" ? {userCode:"ABCD-1234"} : {})}}]);
  }
});
check("login cancel maps its exact ID and cancellation result",()=>{
  const op:Operation={kind:"control",action:"login",value:{action:"cancel",loginId:"own-login"}};
  const plan=hooks.runtimePlan({...ctx,threadId:undefined},op);allowed(op,plan.action);
  assert.deepEqual(plain(plan.action),{kind:"request",call:{method:"account/login/cancel",params:{loginId:"own-login"}}});
  const step=hooks.runtimeResponse(ctx,op,plan.state,{kind:"success",value:{status:"canceled"}});
  if (step.kind !== "complete") assert.fail("Cancellation did not complete");
  assert.deepEqual(plain(step.events),[{kind:"authLogin",login:{status:"canceled",loginId:"own-login"}}]);
});
check("login completion is generic and API key is absent from continuation state",()=>{
  const op:Operation={kind:"control",action:"login",value:{method:"apiKey",apiKey:"opaque-fixture"}};
  const plan=hooks.runtimePlan(ctx,op);allowed(op,plan.action);
  assert.deepEqual(plain(plan.state),{stage:"loginStart",method:"apiKey"});
  assert.deepEqual(plain(hooks.runtimeReduce(ctx,null,[{kind:"notification",method:"account/login/completed",params:{loginId:"own",success:true,error:null,onboardingEntrypoint:null}}]).events),[{kind:"authLogin",login:{status:"completed",loginId:"own"}}]);
  const step=hooks.runtimeResponse(ctx,op,plan.state,{kind:"success",value:{type:"apiKey"}});
  if(step.kind !== "complete") assert.fail("API-key response did not complete");
  assert.deepEqual(plain(step.events),[{kind:"authLogin",login:{status:"completed",method:"apiKey"}}]);
});

console.log(`runtime login regressions: ${count} checks passed`);

check("large history streams bounded states across turns and item pages",()=> {
  const operation:Operation={kind:"readHistory",threadId:ctx.threadId!};
  let plan=hooks.runtimePlan(ctx,operation);
  let step=hooks.runtimeResponse(ctx,operation,plan.state,{kind:"success",value:{data:[{id:"long-turn",status:"inProgress",startedAt:1,ignored:"x".repeat(300000)}]}});
  const chunks:OperationResult[]=[];
  for(let index=0;index<80;index++) {
    if(step.kind !== "continue")throw new Error("Expected history continuation");
    plan=step.plan;
    assert.ok(Buffer.byteLength(JSON.stringify(plan.state)) < 1024,"Only bounded pagination metadata is retained");
    for(const key of ["items","openItems","questions"]) assert.equal(key in (plan.state as Record<string,JsonValue>),false);
    step=hooks.runtimeResponse(ctx,operation,plan.state,{kind:"success",value:{data:[{item:{id:`long-${index}`,type:"agentMessage",text:"x".repeat(8192)}}],...(index<79 ? {nextCursor:`cursor-${index}`} : {})}});
    if(step.kind === "continue" && step.history)chunks.push(step.history);
  }
  if(step.kind !== "continue")throw new Error("Expected optional goal probe");
  assert.ok(Buffer.byteLength(JSON.stringify(step.plan.state)) < 1024);
  step=hooks.runtimeResponse(ctx,operation,step.plan.state,{kind:"error",error:{code:-32601,message:"unsupported"}});
  if(step.kind !== "complete")throw new Error("History did not finish");
  chunks.push(step.result);
  const history=collectHistory(chunks);
  assert.equal(history.items.length,80);assert.equal(history.openItems.length,80);
  assert.equal(history.items.at(-1)?.seq,80);
  assert.equal(history.items.reduce((bytes,item)=>bytes+item.content.length,0),655360);
  console.log("    overflow oracle: accumulated transcript 655360 bytes; every continuation state <1024 bytes");
});
console.log(`runtime bounded history regressions: ${count} checks passed`);
check("turn metadata and cursors have a fixed worst-case state bound",()=> {
  const op:Operation={kind:"readHistory",threadId:ctx.threadId!};
  const plan=hooks.runtimePlan(ctx,op);
  const turns=Array.from({length:8},(_,index)=>({id:"\u0001".repeat(4095)+index,status:"inProgress",startedAt:1,items:[{text:"x".repeat(300000)}]}));
  const cursor="\u0002".repeat(4096);
  let step=hooks.runtimeResponse(ctx,op,plan.state,{kind:"success",value:{data:turns,nextCursor:cursor}});
  if(step.kind !== "continue")throw new Error("Expected item request");
  step=hooks.runtimeResponse(ctx,op,step.plan.state,{kind:"success",value:{data:[],nextCursor:cursor}});
  if(step.kind !== "continue")throw new Error("Expected next page");
  assert.ok(Buffer.byteLength(JSON.stringify(step.plan.state)) < 256*1024);
  for(const data of [[...turns,turns[0]],[{id:"x".repeat(4097)}]]) {
    assert.equal(hooks.runtimeResponse(ctx,op,plan.state,{kind:"success",value:{data}}).kind,"refused");
  }
});
console.log(`runtime metadata bound regressions: ${count} checks passed`);

check("multi-MiB history emits bounded hook outputs and explicit attachment markers",()=> {
  const sizes=JSON.parse(fs.readFileSync(new URL("./fixtures/history-bounds.json",import.meta.url),"utf8"));
  const op:Operation={kind:"readHistory",threadId:ctx.threadId!};
  const data=[
    {item:{id:"image",type:"userMessage",content:[{type:"image",url:`data:image/png;base64,${"A".repeat(sizes.imageBytes)}`}]}},
    {item:{id:"tool",type:"commandExecution",command:"cat",aggregatedOutput:"x".repeat(sizes.toolBytes),status:"completed"}},
    ...Array.from({length:98},(_,index)=>({item:{id:`item-${index}`,type:"agentMessage",text:"\u0001雪🙂".repeat(2000)}})),
  ];
  let step=hooks.runtimeResponse(ctx,op,hooks.runtimePlan(ctx,op).state,{kind:"success",value:{data:[{id:"turn",status:"inProgress",startedAt:1}]}});
  const chunks:OperationResult[]=[];
  let outputs=0;
  while(step.kind === "continue" && step.plan.action.kind === "request" && step.plan.action.call.method === "thread/items/list") {
    assert.ok(++outputs <= 100,"History makes bounded progress");
    step=hooks.runtimeResponse(ctx,op,step.plan.state,{kind:"success",value:{data}});
    const wire=JSON.stringify(step);
    assert.ok(Buffer.byteLength(wire) < sizes.chunkBytes,"Entire hook output stays below 1 MiB");
    assert.ok(!wire.includes("data:image/"));
    if(step.kind === "continue" && step.history)chunks.push(step.history);
  }
  assert.ok(outputs > 1,"A provider item page splits by serialized bytes");
  if(step.kind !== "continue")throw new Error("Expected goal probe");
  step=hooks.runtimeResponse(ctx,op,step.plan.state,{kind:"error",error:{code:-32601,message:"unsupported"}});
  if(step.kind !== "complete")throw new Error("Expected completed history");
  assert.ok(Buffer.byteLength(JSON.stringify(step)) < sizes.chunkBytes);
  const history=collectHistory([...chunks,step.result]);
  assert.equal(history.items.length,100);
  assert.equal(new Set(history.items.map(item=>item.id)).size,100);
  assert.equal(history.items.at(-1)?.seq,100);
  assert.equal(history.openItems.length,99);
  assert.ok(history.items[0].content.includes(`[image omitted: ${sizes.imageBytes+22} bytes]`));
  assert.equal(history.items[0].attachments?.[0].data,undefined);
  assert.equal(history.items[0].attachments?.[0].url,undefined);
  assert.ok(String(history.items[1].toolResult).includes(`[content truncated: ${sizes.toolBytes} bytes total]`));
  assert.ok(history.items.slice(2).every(item=>item.content === "\u0001雪🙂".repeat(2000)));
});
console.log(`runtime byte budget regressions: ${count} checks passed`);

check("all historical tool image shapes retain byte markers without inline payloads",()=> {
  const op:Operation={kind:"readHistory",threadId:ctx.threadId!};
  const first=hooks.runtimePlan(ctx,op);
  let step=hooks.runtimeResponse(ctx,op,first.state,{kind:"success",value:{data:[{id:"turn",status:"completed"}]}});
  if(step.kind !== "continue")throw new Error("Expected items");
  step=hooks.runtimeResponse(ctx,op,step.plan.state,{kind:"success",value:{data:[
    {id:"mcp",type:"mcpToolCall",tool:"image",arguments:{},result:{content:[{type:"image",mimeType:"image/png",data:"A".repeat(100)}]}},
    {id:"generated",type:"imageGeneration",result:"B".repeat(200)},
    {id:"embedded",type:"functionCallOutput",output:`before data:image/png;base64,${"C".repeat(300)} after`},
  ]}});
  if(step.kind !== "continue" || !step.history)throw new Error("Expected history chunk");
  const wire=JSON.stringify(step);
  assert.ok(!wire.includes("data:image/"));
  assert.ok(wire.includes("[image omitted: 100 bytes]"));
  assert.ok(wire.includes("[image omitted: 200 bytes]"));
  assert.ok(wire.includes("[image omitted: 322 bytes]"));
  assert.equal(step.history.items.length,3);
});
console.log(`runtime history image and snapshot regressions: ${count} checks passed`);
check("captured spawn metadata reaches the shared Chat model",()=> {
  const fixture = JSON.parse(fs.readFileSync(new URL("captured/subagent.json",root),"utf8")) as Fixture;
  const {events} = replay(fixture);
  const rows = events.flatMap(event=>event.kind === "chatItem" ? [event.item] : []);
  const records = rows.flatMap(row=> {
    const activity = agentActivityOf(row as unknown as NormalizedChatMessage);
    return activity ? [activity] : [];
  });
  const tree = buildAgentTree(records);
  assert.equal(tree.agents.length,2);
  assert.deepEqual(tree.agents.map(agent=>[agent.model,agent.reasoningEffort,agent.ordinal,agent.state]),[
    ["gpt-5.6-luna","medium",1,"completed"],["gpt-5.6-luna","medium",2,"completed"],
  ]);
  assert.equal(tree.agents[0].description,"Read package.json in the current workspace. Report its relevant contents and key scripts/dependencies. Do not modify files.");
  assert.ok(tree.agents[0].result?.startsWith("`package.json` summary:"));
});
check("activity updates a spawn receiver without losing its identity or metadata",()=> {
  const spawn:RuntimeFrame={kind:"notification",method:"item/completed",params:{threadId:ctx.threadId!,turnId:"parent-turn",item:{id:"spawn",type:"collabAgentToolCall",tool:"spawnAgent",status:"completed",receiverThreadIds:["child-a","child-b"],prompt:"Inspect inputs",model:"fixture-model",reasoningEffort:"high",agentsStates:{"child-a":{status:"pendingInit"},"child-b":{status:"pendingInit"}}}}};
  const started=hooks.runtimeReduce(ctx,null,[spawn]);
  const saved=plain(started.state);
  const activity:RuntimeFrame={kind:"notification",method:"item/completed",params:{threadId:ctx.threadId!,turnId:"later-parent-turn",item:{id:"finished",type:"subAgentActivity",agentThreadId:"child-b",agentPath:"/root/inspect",kind:"completed"}}};
  const finished=hooks.runtimeReduce(ctx,started.state,[activity]);
  assert.deepEqual(plain(started.state),saved,"Reduction does not mutate its input state");
  assert.equal(finished.events.length,1);
  const event=finished.events[0];assert.equal(event.kind,"chatItem");if(event.kind !== "chatItem")return;
  assert.equal(event.item.id,"spawn:child-b");assert.equal(event.item.seq,2);
  assert.equal(event.item.toolFlowId,"parent-turn");
  assert.deepEqual(plain(event.item.toolInput),{agent_id:"child-b",agent_parent_thread_id:ctx.threadId!,agent_call_id:"spawn:child-b",agent_description:"Inspect inputs",agent_model:"fixture-model",agent_reasoning_effort:"high",agent_ordinal:2,agent_state:"completed",agent_path:"/root/inspect",agent_binding_nonce:ctx.bindingNonce,agent_runtime_epoch:ctx.epoch});
  const late={...activity,params:{...activity.params as Record<string,JsonValue>,item:{id:"late-start",type:"subAgentActivity",agentThreadId:"child-b",kind:"started"}}} satisfies RuntimeFrame;
  const lateEvent=hooks.runtimeReduce(ctx,finished.state,[late]).events[0];
  if(lateEvent.kind !== "chatItem")throw new Error("Expected receiver update");
  assert.equal((lateEvent.item.toolInput as Record<string,unknown>).agent_state,"completed");
  const descendant:RuntimeFrame={kind:"notification",method:"turn/started",params:{threadId:"child-b",turn:{id:"child-turn"}}};
  assert.deepEqual(plain(hooks.runtimeReduce(ctx,finished.state,[descendant]).events),[{kind:"turnState",threadId:"child-b",turnId:"child-turn",parentThreadId:ctx.threadId,state:"working"}]);
  assert.deepEqual(plain(hooks.runtimeReduce({...ctx,epoch:ctx.epoch+1},finished.state,[descendant]).events),[]);
  assert.deepEqual(plain(hooks.runtimeReduce(ctx,finished.state,[{...activity,params:{...activity.params as Record<string,JsonValue>,threadId:"foreign"}}]).events),[]);
});
console.log(`runtime subagent regressions: ${count} checks passed`);
check("thread and turn statuses map to shared turn details",()=> {
  const status=(value:unknown)=>plain(hooks.runtimeReduce(ctx,null,[{kind:"notification",method:"thread/status/changed",params:{threadId:ctx.threadId!,status:value as JsonValue}}]).events);
  assert.deepEqual(status({type:"idle"}),[{kind:"turnState",threadId:ctx.threadId,state:"idle"}]);
  assert.deepEqual(status({type:"active",activeFlags:[]}),[]);
  assert.deepEqual(status({type:"active",activeFlags:["waitingOnApproval"]}),[{kind:"turnState",threadId:ctx.threadId,state:"needs_input",detail:"awaitingApproval"}]);
  assert.deepEqual(status({type:"active",activeFlags:["waitingOnUserInput"]}),[{kind:"turnState",threadId:ctx.threadId,state:"needs_input",detail:"awaitingInput"}]);
  assert.deepEqual(status({type:"systemError"}),[{kind:"turnState",threadId:ctx.threadId,state:"needs_input",detail:"systemError"}]);
  assert.deepEqual(status({type:"notLoaded"}),[]);
  const ended=(value:string)=>plain(hooks.runtimeReduce(ctx,null,[{kind:"notification",method:"turn/completed",params:{threadId:ctx.threadId!,turn:{id:"t",status:value}}}]).events);
  assert.deepEqual(ended("interrupted"),[{kind:"turnState",threadId:ctx.threadId,turnId:"t",state:"idle",detail:"interrupted"}]);
  assert.deepEqual(ended("completed"),[{kind:"turnState",threadId:ctx.threadId,turnId:"t",state:"idle"}]);
});
check("compaction stays Compacting across thread status updates and adds no early marker",()=> {
  const params=(item:unknown)=>({threadId:ctx.threadId!,turnId:"c",item:item as JsonValue});
  const frames:RuntimeFrame[]=[
    {kind:"notification",method:"turn/started",params:{threadId:ctx.threadId!,turn:{id:"c"}}},
    {kind:"notification",method:"item/started",params:params({id:"k",type:"contextCompaction"})},
    {kind:"notification",method:"item/started",params:params({id:"k",type:"contextCompaction"})},
    {kind:"notification",method:"thread/status/changed",params:{threadId:ctx.threadId!,status:{type:"active",activeFlags:[]}}},
    {kind:"notification",method:"item/completed",params:params({id:"k",type:"contextCompaction"})},
  ];
  const events=plain(hooks.runtimeReduce(ctx,null,frames).events);
  assert.deepEqual(events.filter(e=>e.kind === "turnState").map(e=>e.kind === "turnState" ? e.detail ?? e.state : ""),["working","compacting","compacting","working"]);
  assert.equal(events.filter(e=>e.kind === "chatItem").length,1);
  assert.equal(events.filter(e=>e.kind === "contextCompacted").length,1);
});
check("reasoning summary part boundaries and live patch updates reduce into rows",()=> {
  const parts=hooks.runtimeReduce(ctx,null,[
    {kind:"notification",method:"item/reasoning/summaryPartAdded",params:{threadId:ctx.threadId!,turnId:"t",itemId:"r",summaryIndex:0}},
    {kind:"notification",method:"item/reasoning/summaryTextDelta",params:{threadId:ctx.threadId!,turnId:"t",itemId:"r",summaryIndex:0,delta:"one"}},
    {kind:"notification",method:"item/reasoning/summaryPartAdded",params:{threadId:ctx.threadId!,turnId:"t",itemId:"r",summaryIndex:1}},
  ]).events;
  assert.equal(parts.length,1);const row=parts[0];assert.ok(row.kind === "chatItem");assert.equal(row.item.content,"one");
  const patch=hooks.runtimeReduce(ctx,null,[
    {kind:"notification",method:"item/fileChange/outputDelta",params:{threadId:ctx.threadId!,turnId:"t",itemId:"p",delta:"applying"}},
    {kind:"notification",method:"item/fileChange/patchUpdated",params:{threadId:ctx.threadId!,turnId:"t",itemId:"p",changes:[{path:"notes.txt",kind:{type:"add"},diff:"rain\n"}]}},
  ]).events;
  const last=patch.at(-1);assert.ok(last?.kind === "chatItem");
  assert.equal(last.item.toolKind,"edit");assert.equal(last.item.toolResult,"applying");
  assert.deepEqual(plain(last.item.fileDiffs)?.map((diff:{path:string;additions:number})=>[diff.path,diff.additions]),[["notes.txt",1]]);
});
console.log(`runtime status fidelity regressions: ${count} checks passed`);

check("subscription initialization reads the recorded current limits",()=> {
  const operation:Operation = {kind:"initialize"};
  const step = hooks.runtimeResponse(ctx,operation,{stage:"initializeAccount"},{kind:"success",value:{account:{type:"chatgpt"}}});
  assert.equal(step.kind,"continue");
  if(step.kind !== "continue") return;
  assert.deepEqual(plain(step.plan.action),{kind:"request",call:{method:"account/rateLimits/read",params:{excludeResetCreditDetails:true}}});
  const value = JSON.parse(fs.readFileSync(new URL("./fixtures/runtime-rate-limits-read.json",import.meta.url),"utf8"));
  const done = hooks.runtimeResponse(ctx,operation,step.plan.state,{kind:"success",value});
  assert.equal(done.kind,"complete");
  if(done.kind !== "complete") return;
  assert.equal(done.events?.[1].kind,"rateLimits");
  assert.equal(hooks.runtimeResponse(ctx,operation,step.plan.state,{kind:"error",error:{code:-32601,message:"unsupported"}}).kind,"complete");
});
check("API-key auth publishes session billing without requesting subscription limits",()=> {
  const step = hooks.runtimeResponse(ctx,{kind:"initialize"},{stage:"initializeAccount"},{kind:"success",value:{account:{type:"apiKey"}}});
  assert.deepEqual(plain(step),{kind:"complete",result:{kind:"initialized"},events:[{kind:"auth",state:"signedIn",billingMode:"api"}]});
  const params = {threadId:ctx.threadId!,tokenUsage:{total:{inputTokens:100,outputTokens:20,cachedInputTokens:30,costUsd:0.002}}};
  const result = hooks.runtimeReduce(ctx,null,[{kind:"notification",method:"thread/tokenUsage/updated",params}]);
  assert.deepEqual(plain(result.events),[{kind:"usage",threadId:ctx.threadId,usage:{input_tokens:100,output_tokens:20,cache_read_tokens:30,cost_micros:2000}}]);
  assert.equal(hooks.runtimeReduce(ctx,null,[{kind:"notification",method:"thread/tokenUsage/updated",params:{...params,threadId:"foreign"}}]).events.length,0);
});

console.log(`runtime usage regressions: ${count} checks passed`);

check("stalled call remains running until the exact superseding turn event",()=> {
  const fixture=JSON.parse(fs.readFileSync(new URL("stalled-turn.json",root),"utf8")) as Fixture;
  const frames=fixture.rpc.flatMap(record=>"frame" in record ? [record.frame] : []);
  const prefix=hooks.runtimeReduce(fixture.context,null,frames.slice(0,4));
  const pending=plain(prefix.state) as {items:Record<string,{toolStatus:string}>};
  assert.equal(pending.items["call-stalled"].toolStatus,"running");
  const last=prefix.events.at(-1);
  assert.equal(last?.kind,"turnState");
  if(last?.kind === "turnState") assert.equal(last.state,"working");
  const finished=hooks.runtimeReduce(fixture.context,prefix.state,frames.slice(4));
  const state=plain(finished.state) as {items:Record<string,{toolStatus:string}>};
  assert.equal(state.items["call-stalled"].toolStatus,"abandoned");
  assert.equal(finished.events.filter(event=>event.kind === "chatItem" && event.item.systemSubtype === "turn_abandoned").length,1);
  assert.equal(hooks.runtimeReduce(fixture.context,finished.state,[frames[4]]).events.some(event=>event.kind === "chatItem"),false);
  const late:RuntimeFrame={kind:"notification",method:"item/completed",params:{threadId:fixture.context.threadId!,turnId:"turn-stalled",item:{id:"call-stalled",type:"commandExecution",status:"completed"}}};
  assert.deepEqual(plain(hooks.runtimeReduce(fixture.context,finished.state,[late]).events),[]);
  const reset=hooks.runtimeReduce({...fixture.context,epoch:fixture.context.epoch+1},finished.state,[frames[4]]);
  assert.equal(reset.events.some(event=>event.kind === "chatItem" && event.item.systemSubtype === "turn_abandoned"),false);
});
console.log(`runtime lifecycle regressions: ${count} checks passed`);

check("reset credit reads and consume outcomes stay inside declared runtime methods",()=> {
  const operation:Operation={kind:"control",action:"readResetCredits",value:{details:true}};
  const plan=hooks.runtimePlan(ctx,operation);
  allowed(operation,plan.action);
  assert.deepEqual(plain(plan.action),{kind:"request",call:{method:"account/rateLimits/read",params:{excludeResetCreditDetails:false}}});
  const background=hooks.runtimePlan(ctx,{...operation,value:{details:false}});
  assert.equal(background.action.kind,"request");
  if(background.action.kind === "request") assert.equal((background.action.call.params as Record<string,JsonValue>).excludeResetCreditDetails,true);
  const read=hooks.runtimeResponse(ctx,operation,plan.state,{kind:"success",value:{rateLimitResetCredits:{availableCount:5,credits:null}}});
  assert.equal(read.kind,"complete");
  if(read.kind === "complete") assert.deepEqual(plain(read.events),[{kind:"rateLimits",rateLimits:[],resets:{availableCount:5,nextExpiry:null,credits:null}}]);
  const missing=hooks.runtimeResponse(ctx,operation,plan.state,{kind:"success",value:{rateLimits:{primary:{usedPercent:0}}}});
  assert.equal(missing.kind,"refused");
  const consume:Operation={kind:"control",action:"redeemResetCredit",value:{idempotencyKey:"12345678-1234-4123-8123-123456789abc"}};
  const consumePlan=hooks.runtimePlan(ctx,consume);
  allowed(consume,consumePlan.action);
  assert.deepEqual(plain(hooks.runtimePlan(ctx,consume)),plain(consumePlan));
  assert.deepEqual(plain(consumePlan.action),{kind:"request",call:{method:"account/rateLimitResetCredit/consume",params:{idempotencyKey:"12345678-1234-4123-8123-123456789abc"}}});
  const selected:Operation = {...consume,value:{idempotencyKey:"12345678-1234-4123-8123-123456789abc",creditId:"opaque-credit"}};
  const selection = hooks.runtimePlan(ctx,selected);
  allowed(selected,selection.action);
  assert.deepEqual(plain(selection.action),{kind:"request",call:{method:"account/rateLimitResetCredit/consume",params:{idempotencyKey:"12345678-1234-4123-8123-123456789abc",creditId:"opaque-credit"}}});
  for (const creditId of ["", 1, {}]) assert.throws(() => hooks.runtimePlan(ctx,{...consume,value:{idempotencyKey:"12345678-1234-4123-8123-123456789abc",creditId}}));
  for(const outcome of ["nothingToReset","noCredit","alreadyRedeemed"]) {
    const done=hooks.runtimeResponse(ctx,consume,consumePlan.state,{kind:"success",value:{outcome}});
    assert.deepEqual(plain(done),{kind:"complete",result:{kind:"resetCreditConsumed",outcome}});
  }
  const reset=hooks.runtimeResponse(ctx,consume,consumePlan.state,{kind:"success",value:{outcome:"reset"}});
  assert.equal(reset.kind,"continue");
  if(reset.kind !== "continue") return;
  allowed(consume,reset.plan.action);
  const refreshed=hooks.runtimeResponse(ctx,consume,reset.plan.state,{kind:"success",value:{rateLimitResetCredits:{availableCount:0}}});
  assert.equal(refreshed.kind,"complete");
  if(refreshed.kind === "complete") assert.deepEqual(plain(refreshed.result),{kind:"resetCreditConsumed",outcome:"reset"});
  const refreshFailed=hooks.runtimeResponse(ctx,consume,reset.plan.state,{kind:"error",error:{code:-1,message:"offline"}});
  assert.deepEqual(plain(refreshFailed),{kind:"complete",result:{kind:"resetCreditConsumed",outcome:"reset"}});
  assert.throws(()=>hooks.runtimePlan(ctx,{kind:"control",action:"redeemResetCredit",value:{}}));
});

check("descending bounded history never falls back to a full-thread read",()=> {
  const op:Operation={kind:"readHistory",threadId:ctx.threadId!,order:"desc"};
  const plan=hooks.runtimePlan(ctx,op);
  const step=plain(hooks.runtimeResponse(ctx,op,plan.state,{kind:"error",error:{code:-32601,message:"unsupported"}}));
  assert.deepEqual(step,{kind:"complete",result:{kind:"historyPage",threadId:ctx.threadId!,items:[],openItems:[],order:"desc"}});
});

check("Fast mode maps both values through thread settings and the next turn with runtime readback",()=> {
  for (const tierId of [null,"priority","ultrafast"]) {
    const serviceTier = tierId;
    const selection = {id:tierId};
    const context = {...ctx,settings:{...ctx.settings,serviceTier:{id:null},serviceTierAvailability:{selected:[{id:"priority",name:"Fast",description:""},{id:"ultrafast",name:"Ultrafast",description:""}]}}};
    const op:Operation = {kind:"control",threadId:ctx.threadId!,action:"settings",value:{model:"selected",effort:"high",serviceTier:selection}};
    const plan = hooks.runtimePlan(context,op);
    assert.equal(plan.action.kind,"request");
    if (plan.action.kind !== "request") throw new Error("Expected settings request");
    assert.equal(plan.action.call.method,"thread/settings/update");
    assert.equal((plan.action.call.params as Record<string,JsonValue>).serviceTier,serviceTier);
    const step = plain(hooks.runtimeResponse(context,op,plan.state,{kind:"success",value:{threadSettings:{model:"selected",effort:"high",serviceTier}}}));
    if (step.kind !== "complete") throw new Error("Expected confirmation");
    assert.equal(step.events?.find(event=>event.kind === "controlsConfirmed"),undefined);
    const event = hooks.runtimeReduce(context,null,[{kind:"notification",method:"thread/settings/updated",params:{threadId:ctx.threadId!,threadSettings:{model:"selected",effort:"high",serviceTier}}}]).events.find(event=>event.kind === "controlsConfirmed");
    assert.ok(event?.kind === "controlsConfirmed");
    assert.deepEqual(event.settings.serviceTier,selection);
    const turn = hooks.runtimePlan(context,{...submit,kind:"submitTurn",settings:event.settings} as Operation);
    assert.equal(turn.action.kind,"request");
    if (turn.action.kind !== "request") throw new Error("Expected turn request");
    assert.equal((turn.action.call.params as Record<string,JsonValue>).serviceTier,serviceTier);
    const missing = hooks.runtimeResponse(context,op,plan.state,{kind:"success",value:{}});
    if (missing.kind !== "complete") throw new Error("Expected completion");
    assert.equal(missing.events?.find(event=>event.kind === "controlsConfirmed"),undefined);
    for (const kind of ["openThread","resumeThread"] as const) {
      const launch = hooks.runtimePlan(context,{kind,instructions:"",...(kind === "resumeThread" ? {threadId:ctx.threadId!} : {}),settings:{...ctx.settings,serviceTier:selection}} as Operation);
      assert.equal(launch.action.kind,"request");
      if (launch.action.kind !== "request") throw new Error("Expected thread request");
      assert.equal((launch.action.call.params as Record<string,JsonValue>).serviceTier,serviceTier);
    }
    const notification = hooks.runtimeReduce(context,null,[{kind:"notification",method:"thread/settings/updated",params:{threadId:ctx.threadId!,threadSettings:{serviceTier}}}]);
    const reported = notification.events.find(event=>event.kind === "controlsConfirmed");
    assert.ok(reported?.kind === "controlsConfirmed");
    assert.deepEqual(reported.settings.serviceTier,selection);
  }
});

console.log(`runtime Fast mode regressions: ${count} checks passed`);

check("runtime image attachments reach turn start once",()=> {
  const text = '<domios type="attachment" kind="image" path="C:/images/a.png" />';
  const input = {text,attachments:[{id:"attachment:C:/images/a.png",mimeType:"image/png",path:"C:/images/a.png"}]};
  const operation:Operation = {kind:"submitTurn",threadId:ctx.threadId!,sendId:"image-send",input,settings:ctx.settings};
  const plan = plain(hooks.runtimePlan({...ctx,activeTurnId:undefined},operation));
  assert.equal(plan.action.kind,"request");
  if (plan.action.kind !== "request") throw new Error("request absent");
  assert.equal(plan.action.call.method,"turn/start");
  assert.deepEqual((plan.action.call.params as {input:unknown}).input,[{type:"text",text},{type:"localImage",path:"C:/images/a.png"}]);
});
