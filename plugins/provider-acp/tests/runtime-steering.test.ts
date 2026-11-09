import assert from "node:assert/strict";
import type { JsonValue, Operation, RpcReply, RuntimeContext, RuntimeStep } from "@codeterm/plugin-sdk";
import { runtimePlan, runtimeResponse } from "../src/runtime";

const SESSION="session-fixture";
const TURN=`${SESSION}:turn:1`;
const base={
  epoch:1,cwd:"/work/project",hostVersion:"1.19.1",bindingNonce:"nonce-fixture",tabId:"",
  settings:{},providerSettings:{},claimedSends:[],activeTurns:[],threadParents:[],
} as unknown as RuntimeContext;
const submit=(sendId:string,activeTurnId?:string):Extract<Operation,{kind:"submitTurn"}>=>({kind:"submitTurn",threadId:SESSION,sendId,input:{text:`opaque ${sendId}`,attachments:[]},settings:{},...(activeTurnId ? {activeTurnId} : {})});

function opened(tabId:string,meta?:JsonValue,instructions=""):{own:RuntimeContext;initialized:RuntimeStep} {
  const unbound={...base,tabId};
  const initialize:Operation={kind:"initialize"};
  const initialized=runtimeResponse(unbound,initialize,runtimePlan(unbound,initialize).state,{kind:"success",value:{protocolVersion:1,agentCapabilities:{},authMethods:[],...(meta ? {_meta:meta} : {})}});
  const open:Operation={kind:"openThread",instructions,settings:{}};
  runtimeResponse(unbound,open,runtimePlan(unbound,open).state,{kind:"success",value:{sessionId:SESSION}});
  return {own:{...unbound,threadId:SESSION},initialized};
}
const running=(own:RuntimeContext):RuntimeContext=>({...own,activeTurnId:TURN,activeTurns:[{threadId:SESSION,turnId:TURN}]} as RuntimeContext);
const steering=(tabId:string,instructions="")=>{
  const {own}=opened(tabId,{steering:{supported:true}},instructions);
  runtimePlan(own,submit("first"));
  const context=running(own);
  const op=submit("second",TURN);
  return {own,context,op,plan:runtimePlan(context,op)};
};

{
  const {own,initialized}=opened("not-advertised");
  assert.deepEqual(initialized.kind === "complete" && initialized.result,{kind:"initialized"},"no advertisement declares no mid-turn input");
  runtimePlan(own,submit("first"));
  const plan=runtimePlan(running(own),submit("second",TURN));
  assert.equal(plan.action.kind === "request" && plan.action.call.method,"session/prompt","an agent that did not advertise steering is never steered");
  console.log("✓ steering stays off unless the agent advertised it");
}

{
  for (const meta of [{steering:{supported:false}},{steering:true},{}] as JsonValue[]) {
    const {initialized}=opened("not-supported",meta);
    assert.deepEqual(initialized.kind === "complete" && initialized.result,{kind:"initialized"});
  }
  const {initialized}=opened("advertised",{steering:{supported:true}});
  assert.deepEqual(initialized.kind === "complete" && initialized.result,{kind:"initialized",midTurnInput:true});
  console.log("✓ only an exact advertisement declares mid-turn input");
}

{
  const {plan,context,op}=steering("steer","opaque instructions");
  assert.deepEqual(plan.action,{kind:"request",call:{
    method:"_session/steering",
    params:{sessionId:SESSION,prompt:[{type:"text",text:"opaque second"}],_meta:{steering:{idleBehavior:"promptRequired"}}},
    target:{threadId:SESSION,turnId:TURN},
  }},"a steer targets the running turn, without first-turn instructions, and asks to keep idle input");
  const step=runtimeResponse(context,op,plan.state,{kind:"success",value:{outcome:"injected"}});
  assert.deepEqual(step,{kind:"complete",result:{kind:"turnAccepted",threadId:SESSION,sendId:"second",turnId:TURN}});
  console.log("✓ a steer joins the running turn and is accepted by its exact outcome");
}

{
  const {own,context,op,plan}=steering("steer-keeps-ordinal");
  runtimeResponse(context,op,plan.state,{kind:"success",value:{outcome:"injected"}});
  const first=submit("first");
  const firstPlan={method:"session/prompt",prompt:1} as JsonValue;
  runtimeResponse(running(own),first,firstPlan,{kind:"success",value:{stopReason:"end_turn"}});
  const next=runtimePlan(own,submit("third"));
  assert.deepEqual(next.action.kind === "request" && next.action.call.target,{threadId:SESSION,turnId:`${SESSION}:turn:2`},"a steer owns no turn, so the next prompt takes the next ordinal");
  console.log("✓ a steer consumes no prompt ordinal");
}

{
  const {own}=opened("no-running-turn",{steering:{supported:true}});
  const idle=runtimePlan(own,submit("first"));
  assert.equal(idle.action.kind === "request" && idle.action.call.method,"session/prompt","with no prompt in flight there is nothing to steer");
  const {own:quiet}=opened("host-turn-without-prompt",{steering:{supported:true}});
  const plan=runtimePlan(running(quiet),submit("first",TURN));
  assert.equal(plan.action.kind === "request" && plan.action.call.method,"session/prompt","a turn with no prompt of ours cannot be steered");
  console.log("✓ steering requires this runtime's own prompt in flight");
}

{
  const cases:[string,RpcReply,boolean][]=[
    ["turn ended first",{kind:"success",value:{outcome:"promptRequired"}},true],
    ["method absent",{kind:"error",error:{code:-32601,message:"opaque"}},true],
    ["params refused",{kind:"error",error:{code:-32602,message:"opaque"}},true],
    ["agent failure may follow injection",{kind:"error",error:{code:-32603,message:"opaque"}},false],
    ["unknown outcome",{kind:"success",value:{outcome:"opaque"}},false],
    ["missing outcome",{kind:"success",value:{}},false],
  ];
  for (const [label,reply,beforeAcceptance] of cases) {
    const {context,op,plan}=steering(`steer-refused-${label}`);
    const step=runtimeResponse(context,op,plan.state,reply);
    assert.equal(step.kind,"refused",label);
    assert.equal(step.kind === "refused" && step.beforeAcceptance,beforeAcceptance,label);
  }
  console.log("✓ only proven non-injection returns input to the host; anything else stays uncertain");
}

{
  const {context,op,plan}=steering("steer-auth");
  const step=runtimeResponse(context,op,plan.state,{kind:"error",error:{code:-32000,message:"opaque"}});
  assert.equal(step.kind === "refused" && step.reason.code,"authentication_required");
  assert.equal(step.kind === "refused" && step.beforeAcceptance,true);
  console.log("✓ a signed-out steer keeps the input for after sign-in");
}
