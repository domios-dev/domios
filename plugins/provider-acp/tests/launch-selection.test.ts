import assert from "node:assert/strict";
import type {JsonValue, Operation, RuntimeContext, RuntimeSettings, RuntimeStep} from "@codeterm/plugin-sdk";
import {runtimePlan,runtimeResponse} from "../src/runtime";
import {launchSelection} from "../src/launchSelection";

const options=(model="m1",effort="low")=>[
  {id:"engine",name:"Engine",type:"select",category:"model",currentValue:model,options:[{value:"m1",name:"First"},{value:"m2",name:"Second"}]},
  {id:"thinking",name:"Thinking",type:"select",category:"thought_level",currentValue:effort,options:[{value:"low",name:"Low"},{value:"high",name:"High"}]},
];
function start(name:string,settings:RuntimeSettings) {
  const ctx:RuntimeContext={tabId:name,epoch:1,bindingNonce:"fixture",cwd:"/fixture",hostVersion:"1.19.1",settings:{},providerSettings:{},claimedSends:[],activeTurns:[],threadParents:[]};
  const init:Operation={kind:"initialize"};
  const plan=runtimePlan(ctx,init);
  runtimeResponse(ctx,init,plan.state,{kind:"success",value:{protocolVersion:1,agentCapabilities:{},authMethods:[]}});
  const op:Operation={kind:"openThread",instructions:"",settings};
  const opening=runtimePlan(ctx,op);
  return {ctx,op,step:runtimeResponse(ctx,op,opening.state,{kind:"success",value:{sessionId:"own",configOptions:options()}})};
}
function next(ctx:RuntimeContext,op:Operation,step:RuntimeStep,configOptions:JsonValue):RuntimeStep {
  assert.equal(step.kind,"continue");
  if (step.kind !== "continue") throw new Error("Expected option request");
  return runtimeResponse(ctx,op,step.plan.state,{kind:"success",value:{configOptions}});
}

const valid=start("valid",{model:"m2",effort:"high"});
assert.equal(valid.step.kind,"continue");
if (valid.step.kind !== "continue") throw new Error("Expected model selection");
assert.deepEqual(valid.step.plan.action,{kind:"request",call:{method:"session/set_config_option",params:{sessionId:"own",configId:"engine",value:"m2"}}});
const effort=next(valid.ctx,valid.op,valid.step,options("m2"));
assert.equal(effort.kind,"continue");
if (effort.kind !== "continue") throw new Error("Expected effort selection");
assert.deepEqual(effort.plan.action,{kind:"request",call:{method:"session/set_config_option",params:{sessionId:"own",configId:"thinking",value:"high"}}});
const loaded=next(valid.ctx,valid.op,effort,options("m2","high"));
assert.equal(loaded.kind,"complete");
if (loaded.kind !== "complete" || loaded.result.kind !== "threadLoaded") throw new Error("Expected ready session");
assert.equal(loaded.result.settings.model,"m2");
assert.equal(loaded.result.settings.effort,"high");
const prompt:Operation={kind:"submitTurn",threadId:"own",sendId:"first",input:{text:"opaque bootstrap",attachments:[]},settings:loaded.result.settings};
const first=runtimePlan({...valid.ctx,threadId:"own",settings:loaded.result.settings},prompt);
assert.equal(first.action.kind,"request");
if (first.action.kind === "request") assert.equal(first.action.call.method,"session/prompt");

for (const [settings,values] of [[{model:"invalid"},"m1, m2"],[{effort:"invalid"},"low, high"]] as const) {
  const invalid=start(`invalid-${values}`,settings);
  assert.equal(invalid.step.kind,"refused");
  if (invalid.step.kind === "refused") assert.ok(invalid.step.reason.message.includes(`offered values: ${values}`));
}
const unconfirmed=start("unconfirmed",{model:"m2"});
const refused=next(unconfirmed.ctx,unconfirmed.op,unconfirmed.step,options());
assert.equal(refused.kind,"refused");
const defaults=start("defaults",{});
assert.equal(defaults.step.kind,"complete");
const dynamic=next(valid.ctx,valid.op,valid.step,options("m2").map(option=>option.category === "thought_level" ? {...option,options:[{value:"low",name:"Low"}]} : option));
assert.equal(dynamic.kind,"refused");
assert.equal(launchSelection({effort:"high"},{},[]).kind,"refused");
console.log("✓ ACP model and effort confirmed before first prompt; invalid, dynamic and unconfirmed choices refuse; defaults preserved");
