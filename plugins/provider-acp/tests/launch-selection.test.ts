import assert from "node:assert/strict";
import type {JsonValue, Operation, RuntimeContext, RuntimeSettings, RuntimeStep} from "@codeterm/plugin-sdk";
import {runtimePlan,runtimeResponse} from "../src/runtime";
import {launchSelection} from "../src/launchSelection";
import {sessionModel,sessionSettings} from "../src/runtimeControls";

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
  const afterRefusal=runtimePlan(invalid.ctx,invalid.op);
  assert.equal(afterRefusal.action.kind,"request");
  if (afterRefusal.action.kind === "request") assert.equal(afterRefusal.action.call.method,"session/new");
}
const unconfirmed=start("unconfirmed",{model:"m2"});
const refused=next(unconfirmed.ctx,unconfirmed.op,unconfirmed.step,options());
assert.equal(refused.kind,"refused");
assert.equal(runtimePlan(unconfirmed.ctx,unconfirmed.op).action.kind,"request");
const configRefusal=start("config-refusal",{model:"m2"});
if (configRefusal.step.kind !== "continue") throw new Error("Expected config selection");
const rejected=runtimeResponse(configRefusal.ctx,configRefusal.op,configRefusal.step.plan.state,
  {kind:"error",error:{code:-32602,message:"opaque selection refused"}});
assert.equal(rejected.kind,"refused");
assert.equal(runtimePlan(configRefusal.ctx,configRefusal.op).action.kind,"request");
const defaults=start("defaults",{});
assert.equal(defaults.step.kind,"complete");
const dynamic=next(valid.ctx,valid.op,valid.step,options("m2").map(option=>option.category === "thought_level" ? {...option,options:[{value:"low",name:"Low"}]} : option));
assert.equal(dynamic.kind,"refused");
assert.equal(launchSelection({effort:"high"},{},[]).kind,"refused");
console.log("✓ ACP model and effort confirmed before first prompt; invalid, dynamic and unconfirmed choices refuse; defaults preserved");

const models={currentModelId:"m1",availableModels:[{modelId:"m1",name:"First"},{modelId:"m2",name:"Second"}]};
const modelsOnly=sessionSettings({configOptions:[],models});
assert.equal(modelsOnly.model,"m1");
assert.equal(modelsOnly.effort,undefined);
assert.equal(launchSelection({effort:"low"},modelsOnly,[]).kind,"refused");
assert.equal(launchSelection({model:"unknown"},modelsOnly,[]).kind,"refused");
assert.equal(sessionModel({...models,currentModelId:"missing"}),undefined);
assert.equal(sessionModel({...models,availableModels:[{modelId:"m1",name:"First"},{modelId:"m1",name:"Duplicate"}]})?.options.length,1);
assert.equal(sessionModel(models,[{id:"acp-session-model",name:"Other",currentValue:"x",options:[]}])?.id,"acp-session-model-");
assert.equal(sessionSettings({models,configOptions:options()}).configOptions?.filter(option=>option.category === "model").length,1);
const modelRefusal=start("model-state-refusal",{});
const reopen:Operation={kind:"resumeThread",threadId:"own",instructions:"",settings:{model:"m2"}};
const resumedCtx={...modelRefusal.ctx,threadId:"own"};
const initialized=runtimePlan(resumedCtx,{kind:"initialize"});
runtimeResponse(resumedCtx,{kind:"initialize"},initialized.state,{kind:"success",value:{protocolVersion:1,agentCapabilities:{loadSession:true},authMethods:[]}});
const resuming=runtimePlan(resumedCtx,reopen);
const pending=runtimeResponse(resumedCtx,reopen,resuming.state,{kind:"success",value:{models}});
assert.equal(pending.kind,"continue");
if (pending.kind !== "continue") throw new Error("Expected session model request");
const modelRejected=runtimeResponse(resumedCtx,reopen,pending.plan.state,{kind:"error",error:{code:-32602,message:"opaque model refused"}});
assert.equal(modelRejected.kind,"refused");
console.log("✓ ACP session models preserve declared ids, reject undeclared effort and models, and propagate set_model refusal");

const fullOptions=Array.from({length:64},(_,index)=>({id:`setting-${index}`,name:"Other",type:"select",currentValue:"x",options:[]}));
const boundedModels=sessionSettings({models,configOptions:fullOptions});
assert.equal(boundedModels.configOptions?.length,64);
assert.equal(boundedModels.configOptions?.filter(option=>option.category === "model").length,1);
