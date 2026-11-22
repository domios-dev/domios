import assert from "node:assert/strict";
import type { Operation, RuntimeContext } from "@codeterm/plugin-sdk";
import { runtimePlan, runtimeReduce, runtimeResponse } from "../src/runtime";
import { contextUsage, usage, usageFromPayload, usageFromReply } from "../src/runtimeReducePlan";

assert.deepEqual(contextUsage("own",{used:53000,size:200000}),{kind:"context",threadId:"own",used_tokens:53000,window_tokens:200000});
assert.deepEqual(contextUsage("own",{used:0,size:0,cost:{amount:0,currency:"USD"}}),{kind:"context",threadId:"own",used_tokens:0,window_tokens:0,cost_micros:0});
assert.deepEqual(contextUsage("own",{used:1,size:2,cost:{amount:0.0123456,currency:"USD"}}),{kind:"context",threadId:"own",used_tokens:1,window_tokens:2,cost_micros:12346});
for (const cost of [undefined,null,{}, {amount:1}, {amount:1,currency:"EUR"}, {amount:-1,currency:"USD"}, {amount:Infinity,currency:"USD"}, {amount:NaN,currency:"USD"}, {amount:Number.MAX_VALUE,currency:"USD"}]) {
  const event=contextUsage("own",{used:1,size:2,cost});
  assert.ok(event);
  assert.equal("cost_micros" in event,false,"unreported or invalid USD cost is omitted");
}
for (const raw of [{}, {used:1}, {size:2}, {used:null,size:2}, {used:-1,size:2}, {used:1,size:-2}, {used:1.5,size:2}, {used:1,size:Infinity}, {used:Number.MAX_SAFE_INTEGER+1,size:2}]) {
  assert.equal(contextUsage("own",raw),undefined,"invalid or missing context counters do not emit an event");
}
assert.equal(usage({used:1,size:2}),undefined,"context-only observations never fabricate prompt token usage");
assert.deepEqual(usage({inputTokens:2,outputTokens:3,cachedReadTokens:1,cost:{amount:0.001,currency:"USD"}}),{input_tokens:2,output_tokens:3,cache_read_tokens:1,cost_micros:1000});
assert.deepEqual(usage({inputTokens:0,outputTokens:0}),{input_tokens:0,output_tokens:0});
assert.deepEqual(
  usage({inputTokens:35000,outputTokens:12000,thoughtTokens:5000,cachedReadTokens:5000,cachedWriteTokens:1000,totalTokens:53000,cost:{amount:0.042,currency:"USD"}}),
  {input_tokens:36000,output_tokens:17000,cache_read_tokens:5000,cache_write_tokens:1000,cost_micros:42000},
);
assert.deepEqual(
  usage({inputTokens:25368,outputTokens:33,totalTokens:25401,cachedReadTokens:25088,cacheCreationTokens:0,reasoningTokens:17,costUsdTicks:45226800}),
  {input_tokens:25368,output_tokens:33,cache_read_tokens:25088,cache_write_tokens:0,cost_micros:4523},
);
assert.deepEqual(
  usage({inputTokens:100,outputTokens:50,cachedReadTokens:5000,cachedWriteTokens:200,thoughtTokens:40}),
  {input_tokens:5300,output_tokens:90,cache_read_tokens:5000,cache_write_tokens:200},
);
assert.deepEqual(usageFromReply({_meta:{usage:{inputTokens:10,outputTokens:5,thoughtTokens:3}}}),{input_tokens:10,output_tokens:8});
assert.deepEqual(usageFromPayload({usage:{inputTokens:7,outputTokens:2,costUsd:0.005}}),{input_tokens:7,output_tokens:2,cost_micros:5000});

const base:RuntimeContext={
  epoch:1,cwd:"/work/project",hostVersion:"1.21.1",tabId:"acp-usage-test",bindingNonce:"nonce-usage",
  settings:{},providerSettings:{},claimedSends:[],activeTurns:[],threadParents:[],
};
const initOp:Operation={kind:"initialize"};
const initPlan=runtimePlan(base,initOp);
runtimeResponse(base,initOp,initPlan.state,{kind:"success",value:{protocolVersion:1,agentCapabilities:{},authMethods:[]}});
const openOp:Operation={kind:"openThread",instructions:"",settings:{}};
const openPlan=runtimePlan(base,openOp);
runtimeResponse(base,openOp,openPlan.state,{kind:"success",value:{sessionId:"sess-usage"}});
const bound:RuntimeContext={...base,threadId:"sess-usage"};

const turn1:Operation={kind:"submitTurn",threadId:"sess-usage",sendId:"send-1",input:{text:"hi",attachments:[]},settings:{}};
const plan1=runtimePlan(bound,turn1);
const step1=runtimeResponse(bound,turn1,plan1.state,{kind:"success",value:{stopReason:"end_turn",usage:{inputTokens:1000,outputTokens:200,thoughtTokens:300,cachedReadTokens:400,cost:{amount:0.01,currency:"USD"}}}});
assert.equal(step1.kind,"complete");
assert.deepEqual(
  step1.kind === "complete" ? step1.events?.filter(event=>event.kind === "usage") : [],
  [{kind:"usage",threadId:"sess-usage",usage:{input_tokens:1000,output_tokens:500,cache_read_tokens:400,cost_micros:10000,response_id:"send-1"}}],
);

const turn2:Operation={kind:"submitTurn",threadId:"sess-usage",sendId:"send-2",input:{text:"next",attachments:[]},settings:{}};
const plan2=runtimePlan(bound,turn2);
const reduced2=runtimeReduce(bound,null,[{
  kind:"notification",method:"_x.ai/session/update",
  params:{sessionId:"sess-usage",update:{sessionUpdate:"turn_completed",prompt_id:"p2",stop_reason:"end_turn",usage:{inputTokens:25368,outputTokens:33,totalTokens:25401,cachedReadTokens:25088,cacheCreationTokens:0,reasoningTokens:17,costUsdTicks:45226800}}},
}]);
assert.equal(reduced2.events.length,0,"extension turn_completed does not emit unknown-update notices");
const step2=runtimeResponse(bound,turn2,plan2.state,{kind:"success",value:{stopReason:"end_turn"}});
assert.equal(step2.kind,"complete");
assert.deepEqual(
  step2.kind === "complete" ? step2.events?.filter(event=>event.kind === "usage") : [],
  [{kind:"usage",threadId:"sess-usage",usage:{input_tokens:25368,output_tokens:33,cache_read_tokens:25088,cache_write_tokens:0,cost_micros:4523,response_id:"send-2"}}],
);

const turn3:Operation={kind:"submitTurn",threadId:"sess-usage",sendId:"send-3",input:{text:"via usage_update",attachments:[]},settings:{}};
const plan3=runtimePlan(bound,turn3);
const reduced3=runtimeReduce(bound,null,[{
  kind:"notification",method:"session/update",
  params:{sessionId:"sess-usage",update:{sessionUpdate:"usage_update",used:120,size:1000,inputTokens:90,outputTokens:30,cachedReadTokens:20}},
}]);
assert.deepEqual(reduced3.events,[{kind:"turnState",threadId:"sess-usage",turnId:"sess-usage:turn:3",state:"working"},{kind:"context",threadId:"sess-usage",used_tokens:120,window_tokens:1000}]);
const step3=runtimeResponse(bound,turn3,plan3.state,{kind:"success",value:{stopReason:"end_turn"}});
assert.equal(step3.kind,"complete");
assert.deepEqual(
  step3.kind === "complete" ? step3.events?.filter(event=>event.kind === "usage") : [],
  [{kind:"usage",threadId:"sess-usage",usage:{input_tokens:90,output_tokens:30,cache_read_tokens:20,used_tokens:120,window_tokens:1000,response_id:"send-3"}}],
);

const turn4:Operation={kind:"submitTurn",threadId:"sess-usage",sendId:"send-4",input:{text:"unreported",attachments:[]},settings:{}};
const plan4=runtimePlan(bound,turn4);
const step4=runtimeResponse(bound,turn4,plan4.state,{kind:"success",value:{stopReason:"end_turn"}});
assert.equal(step4.kind,"complete");
assert.deepEqual(step4.kind === "complete" ? step4.events?.filter(event=>event.kind === "usage") : [],[],"turns without reported usage emit no usage event");

console.log("✓ ACP usage mapping captures reply, usage_update, and turn_completed tokens/cost without fabricating unreported turns");
