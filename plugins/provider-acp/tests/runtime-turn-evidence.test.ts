import assert from "node:assert/strict";
import type { AgentEvent, JsonValue, Operation, RuntimeContext, RuntimeFrame } from "@codeterm/plugin-sdk";
import { runtimePlan, runtimeReduce, runtimeResponse } from "../src/runtime";

const SESSION="session-fixture";
const base:RuntimeContext={
  epoch:1,cwd:"/work/project",hostVersion:"1.19.1",bindingNonce:"nonce-fixture",tabId:"",
  settings:{},providerSettings:{},claimedSends:[],activeTurns:[],threadParents:[],
} as unknown as RuntimeContext;
const update=(body:Record<string,JsonValue>):RuntimeFrame=>({kind:"notification",method:"session/update",params:{sessionId:SESSION,update:body}});
const chunk=(kind:string,text:string):RuntimeFrame=>update({sessionUpdate:kind,content:{type:"text",text}});
const CONTROL_UPDATES:Record<string,JsonValue>[]=[
  {sessionUpdate:"current_mode_update",currentModeId:"yolo"},
  {sessionUpdate:"available_commands_update",availableCommands:[{name:"inspect",description:"Inspect"}]},
  {sessionUpdate:"config_option_update",configOptions:[{id:"model",name:"Model",category:"model",type:"select",currentValue:"opaque",options:[{value:"opaque",name:"Opaque"}]}]},
  {sessionUpdate:"session_info_update",title:"opaque"},
  {sessionUpdate:"usage_update",used:3,size:1000},
  {sessionUpdate:"opaque_future_update"},
];
const turnStates=(events:readonly AgentEvent[])=>events.filter(event=>event.kind === "turnState");

function openedSession(tabId:string):RuntimeContext {
  const unbound={...base,tabId};
  const initialize:Operation={kind:"initialize"};
  runtimeResponse(unbound,initialize,runtimePlan(unbound,initialize).state,{kind:"success",value:{protocolVersion:1,agentCapabilities:{},authMethods:[]}});
  const open:Operation={kind:"openThread",instructions:"",settings:{}};
  runtimeResponse(unbound,open,runtimePlan(unbound,open).state,{kind:"success",value:{
    sessionId:SESSION,modes:{currentModeId:"default",availableModes:[{id:"default",name:"Default"},{id:"yolo",name:"YOLO"}]},
  }});
  return {...unbound,threadId:SESSION};
}

const idleWindow=(own:RuntimeContext,sequence:number):RuntimeContext=>({...own,activeTurnId:undefined,streamWindow:{frameSequences:[sequence],turnRequests:[]}} as RuntimeContext);

// A switched mode is a control round trip, then the agent's own echo: neither is a turn.
{
  const own=openedSession("control-idle");
  const control:Operation={kind:"control",threadId:SESSION,action:"mode",value:"yolo"};
  const plan=runtimePlan(own,control);
  assert.equal(plan.action.kind === "request" && plan.action.call.method,"session/set_mode");
  const step=runtimeResponse(own,control,plan.state,{kind:"success",value:{}});
  assert.equal(step.kind,"complete");
  assert.deepEqual(step.kind === "complete" ? turnStates(step.events ?? []) : [],[],"a control reply opens no turn");
  const echo=runtimeReduce(idleWindow(own,1),null,[update(CONTROL_UPDATES[0])]);
  assert.deepEqual(turnStates(echo.events),[],"a control echo opens no turn");
  assert.equal(echo.events.filter(event=>event.kind === "controlsConfirmed").length,1,"the echo still confirms the mode");
  console.log("✓ ACP mode control and its echo open no turn on an idle session");
}

// A turn the host already ended leaves this reducer unsettled, which must not let a later control echo reopen it.
{
  const own=openedSession("control-after-host-ended-turn");
  const send:Operation={kind:"submitTurn",threadId:SESSION,sendId:"send",input:{text:"opaque",attachments:[]},settings:{}};
  runtimePlan(own,send);
  const turn=`${SESSION}:turn:1`;
  const written={...own,activeTurnId:turn,claimedSends:[{sendId:"send",threadId:SESSION,input:send.input}],
    streamWindow:{frameSequences:[11],turnRequests:[{sendId:"send",turnId:turn,afterSequence:10,written:true}]}} as RuntimeContext;
  let state=runtimeReduce(written,null,[chunk("agent_message_chunk","partial")]).state;
  for (const [index,body] of CONTROL_UPDATES.entries()) {
    const reduced=runtimeReduce(idleWindow(own,20+index),state,[update(body)]);
    assert.deepEqual(turnStates(reduced.events),[],`${String(body.sessionUpdate)} after a host-ended turn opens no turn`);
    state=reduced.state;
  }
  const stray=runtimeReduce(idleWindow(own,40),state,[chunk("agent_message_chunk","late")]);
  assert.deepEqual(turnStates(stray.events),[],"output no written prompt owns opens no turn");
  const permission=runtimeReduce(idleWindow(own,41),stray.state,[{kind:"request",method:"session/request_permission",requestId:1,params:{sessionId:SESSION,options:[],toolCall:{toolCallId:"call"}}}]);
  assert.deepEqual(turnStates(permission.events),[],"a client request no written prompt owns opens no turn");
  console.log("✓ ACP control echoes and unowned output never reopen a turn the host ended");
}

// Unprompted output on a session that never received a prompt opens nothing either.
{
  const own=openedSession("unprompted-output");
  const reduced=runtimeReduce(idleWindow(own,1),null,[chunk("agent_message_chunk","unprompted")]);
  assert.deepEqual(turnStates(reduced.events),[]);
  console.log("✓ ACP unprompted output opens no turn");
}

// A written prompt still opens its turn from the first update it owns.
{
  const own=openedSession("written-prompt-activity");
  const send:Operation={kind:"submitTurn",threadId:SESSION,sendId:"send",input:{text:"opaque",attachments:[]},settings:{}};
  runtimePlan(own,send);
  const turn=`${SESSION}:turn:1`;
  const written={...own,activeTurnId:undefined,claimedSends:[{sendId:"send",threadId:SESSION,input:send.input}],
    streamWindow:{frameSequences:[11],turnRequests:[{sendId:"send",turnId:turn,afterSequence:10,written:true}]}} as RuntimeContext;
  const activity=runtimeReduce(written,null,[chunk("agent_thought_chunk","thinking")]);
  assert.deepEqual(turnStates(activity.events).map(event=>[event.turnId,event.state]),[[turn,"working"]]);
  console.log("✓ ACP written prompt opens its turn from its own updates");
}
