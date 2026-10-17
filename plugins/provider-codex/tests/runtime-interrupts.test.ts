import assert from "node:assert/strict";
import fs from "node:fs";
import vm from "node:vm";
import type { AgentRuntime, Operation, RuntimeContext, RuntimeFrame } from "@codeterm/plugin-sdk";

const sandbox = vm.createContext({module:{exports:{}},exports:{}});
sandbox.exports = sandbox.module.exports;
vm.runInContext(fs.readFileSync(new URL("../plugin.js",import.meta.url),"utf8"),sandbox);
const hooks = sandbox.module.exports.default as AgentRuntime;
const ctx = JSON.parse(fs.readFileSync(new URL("./fixtures/runtime/interrupt.json",import.meta.url),"utf8")).context as RuntimeContext;
const plain = <T>(value:T):T => JSON.parse(JSON.stringify(value));
let count = 0;
function check(name:string,run:()=>void) {run();count++;console.log(`  ✓ runtime interrupt ${name}`);}

check("interrupted completion publishes the interrupted idle outcome",()=> {
  const turnId="interrupted-turn";
  const context={...ctx,activeTurnId:turnId,activeTurns:[{threadId:ctx.threadId!,turnId}]};
  const frame:RuntimeFrame={kind:"notification",method:"turn/completed",params:{threadId:ctx.threadId!,turn:{id:turnId,status:"interrupted"}}};
  assert.deepEqual(plain(hooks.runtimeReduce(context,null,[frame]).events),[
    {kind:"turnState",threadId:ctx.threadId!,turnId,state:"idle",detail:"interrupted"},
  ]);
});

check("wrong thread and active turn refuse",()=> {
  assert.throws(()=>hooks.runtimePlan(ctx,{kind:"interrupt",threadId:"foreign",activeTurnId:"active"}));
  assert.throws(()=>hooks.runtimePlan(ctx,{kind:"interrupt",threadId:ctx.threadId!,activeTurnId:"wrong"}));
});

check("interrupt plan carries the exact root turn and refuses absent authority",()=> {
  const context={...ctx,activeTurnId:"current-turn",activeTurns:[{threadId:ctx.threadId!,turnId:"current-turn"}]};
  const operation:Operation={kind:"interrupt",threadId:ctx.threadId!,activeTurnId:"current-turn"};
  const plan=hooks.runtimePlan(context,operation);
  assert.equal(plan.action.kind,"request");if(plan.action.kind !== "request")return;
  assert.equal(plan.action.call.method,"turn/interrupt");
  assert.deepEqual(plain(plan.action.call.params),{threadId:ctx.threadId,turnId:"current-turn"});
  assert.deepEqual(plain(plan.action.call.target),{threadId:ctx.threadId,turnId:"current-turn"});
  assert.throws(()=>hooks.runtimePlan({...context,activeTurns:[]},operation));
  assert.throws(()=>hooks.runtimePlan(context,{...operation,threadId:"foreign"}));
});

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

check("completion interrupts unfinished calls without inventing output",()=> {
  const frames:RuntimeFrame[]=[
    {kind:"notification",method:"turn/started",params:{threadId:ctx.threadId!,turn:{id:"open"}}},
    {kind:"notification",method:"item/started",params:{threadId:ctx.threadId!,turnId:"open",item:{id:"pending",type:"mcpToolCall",tool:"lookup",arguments:{},status:"inProgress"}}},
    {kind:"notification",method:"turn/completed",params:{threadId:ctx.threadId!,turn:{id:"open",status:"interrupted"}}},
  ];
  const result=hooks.runtimeReduce(ctx,null,frames);
  const row=result.events.filter(event=>event.kind === "chatItem").at(-1);
  assert.ok(row?.kind === "chatItem");
  assert.equal((row.item as {toolStatus?:string}).toolStatus,"interrupted");
  assert.equal(row.item.toolResult,undefined);
  assert.equal(row.item.toolError,undefined);
  const last=result.events.at(-1);
  assert.ok(last?.kind === "turnState");assert.equal(last.state,"idle");
});

console.log(`runtime interrupt regressions: ${count} checks passed`);
