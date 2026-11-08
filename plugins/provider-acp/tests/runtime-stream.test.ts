import assert from "node:assert/strict";
import fs from "node:fs";
import type { AgentEvent, ChatMsg, JsonValue, RuntimeContext, RuntimeFrame, Operation } from "@codeterm/plugin-sdk";
import { reduceStream, settleStream, type StreamCursor } from "../src/runtimeReduce";
import { foldRow } from "./runtimeRows";
import { sessionSettings } from "../src/runtimeControls";
import { runtimePlan as sourcePlan, runtimeResponse as sourceResponse, runtimeReduce as sourceReduce } from "../src/runtime";
import { runtimeJson } from "./runtimeJson";
import { assertRuntimePlan, assertRuntimeReduction } from "./runtimeAuthority";
import { reduceTool } from "../src/runtimeReduceTools";
import { fitTextReduction } from "../src/runtimeReduceBudget";
const runtimePlan=(...args:Parameters<typeof sourcePlan>)=>{const plan=runtimeJson(sourcePlan(...args),"runtimePlan");assertRuntimePlan(args[0],args[1],plan);return plan;};
const runtimeResponse=(...args:Parameters<typeof sourceResponse>)=>{const step=runtimeJson(sourceResponse(...args),"runtimeResponse");if (step.kind === "continue") assertRuntimePlan(args[0],args[1],step.plan);return step;};
const runtimeReduce=(...args:Parameters<typeof sourceReduce>)=>{const reduction=runtimeJson(sourceReduce(...args),"runtimeReduce");assertRuntimeReduction(reduction);return reduction;};

for (const name of ["gemini-durable-replay-new-turn","resume-buffered-replay-new-turn"]) {
  const fixture=JSON.parse(fs.readFileSync(new URL(`./fixtures/runtime/${name}.json`,import.meta.url),"utf8"));
  const plans=new Map<number,ReturnType<typeof runtimePlan>>();
  const durableRows:ChatMsg[]=name === "gemini-durable-replay-new-turn"
    ? JSON.parse(fs.readFileSync(new URL("./fixtures/gemini-durable-rows.json",import.meta.url),"utf8")) : [];
  const rows=new Map<string,ChatMsg>(durableRows.map(row=>[row.id,{...row}]));
  let state:JsonValue=null;
  const events:AgentEvent[]=[];
  for (const record of fixture.rpc) {
    if (record.direction === "outbound") {
      const entry=fixture.operations[record.operation];
      const plan=runtimePlan(entry.context ?? fixture.context,entry.operation);
      assert.deepEqual(plan.action,record.action);
      plans.set(record.operation,plan);
    } else if (record.reply) {
      const entry=fixture.operations[record.operation];
      const step=runtimeResponse(entry.context ?? fixture.context,entry.operation,plans.get(record.operation)!.state,record.reply);
      assert.equal(step.kind,"complete");
      if (step.kind === "complete") events.push(...step.events ?? []);
    } else {
      const reduced=runtimeReduce(record.context ?? fixture.context,state,record.frames);
      state=reduced.state;
      events.push(...reduced.events);
      if (!record.context.streamWindow.turnRequests.some((request:{written:boolean})=>request.written)) {
        assert.equal(events.some(event=>event.kind === "turnState"),false,"buffered resume replay, even after planning a new prompt, leaves the tab idle");
      }
    }
  }
  assert.deepEqual(events,fixture.expected.events);
  assert.deepEqual(events.filter(event=>event.kind === "turnState").map(event=>[event.turnId,event.state]),
    [["thread:turn:8","working"],["thread:turn:8","idle"]]);
  for (const event of events) foldRow(rows,event);
  assert.deepEqual([...rows.values()].slice(0,durableRows.length),durableRows,"restore preserves durable row identities, order, content and tool results exactly");
  assert.equal(rows.size,durableRows.length+3,"only the new prompt, thought and reply are added");
  assert.equal([...rows.values()].filter(row=>row.type === "assistant" && row.content === "NEW-TURN-OK").length,1);
  assert.equal(events.some(event=>event.kind === "notice"),false);
  console.log("✓ ACP buffered resume replay stays idle; the first written new turn streams and completes once");
}

interface Fixture {
  context:RuntimeContext;
  operations:{operation:Operation;context?:RuntimeContext}[];
  rpc:({direction:"outbound";operation:number;action:{kind:string}} | {direction:"inbound";operation:number;reply:{kind:string;value?:JsonValue}} |
    {direction:"inbound";frame:RuntimeFrame;context?:RuntimeContext})[];
}
const root=new URL("./fixtures/runtime/",import.meta.url);
const names=["stream-message-thought-chunks","tool-call-lifecycle","plan-updates","commands-modes-config-usage","load-replay-no-duplicates","unknown-update-kind","cancel-mid-tool","lost-stream"];
function replay(name:string):{state:Record<string,JsonValue>;rows:ChatMsg[];events:AgentEvent[];fixture:Fixture} {
  const fixture:Fixture=JSON.parse(fs.readFileSync(new URL(`${name}.json`,root),"utf8"));
  let state:JsonValue=null;
  const cursor:StreamCursor={prompt:0,replay:0,loading:false};
  const rows=new Map<string,ChatMsg>();
  const events:AgentEvent[]=[];
  const collect=(batch:readonly AgentEvent[]):void=>{for (const event of batch) {events.push(event);foldRow(rows,event);}};
  for (const record of fixture.rpc) {
    if (record.direction === "outbound") {
      const op=fixture.operations[record.operation].operation;
      if (op.kind === "submitTurn") {cursor.prompt++;cursor.loading=false;}
      if (op.kind === "resumeThread" && record.action.kind !== "local") {cursor.replay++;cursor.prompt=0;cursor.loading=true;}
    } else if ("frame" in record) {
      const before=JSON.stringify(state);
      const reduced=reduceStream(record.context ?? fixture.context,state,[record.frame],cursor);
      assert.equal(JSON.stringify(state),before,`${name}: reducer must not mutate its input`);
      state=reduced.state;collect(reduced.events);
    } else {
      const entry=fixture.operations[record.operation];
      if (entry.operation.kind === "resumeThread") cursor.loading=false;
      if (entry.operation.kind === "submitTurn") {
        const result=record.reply.value as Record<string,JsonValue>;
        const settled=settleStream(entry.context!,state,String(result.stopReason));
        state=settled.state;collect(settled.events);
      }
    }
  }
  return {state:state as Record<string,JsonValue>,rows:[...rows.values()],events,fixture};
}
assert.equal(names.length,8);
const scenarios=new Map(names.map(name=>[name,replay(name)]));
const messages=scenarios.get("stream-message-thought-chunks")!;
assert.deepEqual(messages.rows.map(row=>[row.id,row.type,row.content,row.collapsed ?? false]),[
  ["session-fixture:turn:1:item:1","user","Hello",false],
  ["session-fixture:turn:1:item:2","thinking","Thinking",true],
  ["session-fixture:turn:1:item:3","assistant","Answer",false],
]);
const tools=scenarios.get("tool-call-lifecycle")!;
assert.equal(tools.rows.length,1);
assert.equal(tools.rows[0].toolId,"call-1");
assert.equal(tools.rows[0].toolResult,"Edited");
assert.equal(tools.rows[0].toolStatus,"completed");
assert.equal((tools.rows[0].fileDiffs as {path:string}[])[0].path,"a.txt");
assert.equal(scenarios.get("plan-updates")!.rows.length,1);
assert.equal(scenarios.get("commands-modes-config-usage")!.rows.length,0);
const replayed=scenarios.get("load-replay-no-duplicates")!;
assert.equal(replayed.rows.length,3,"same-epoch reattach must keep exactly three transcript rows");
assert.deepEqual(replayed.rows,messages.rows,"reattach retains live row ids, order, content and sequence");
assert.equal(replayed.state.replay,0);
const unknown=scenarios.get("unknown-update-kind")!;
assert.equal(unknown.rows.filter(row=>row.systemSubtype === "runtime_warning").length,0);
assert.equal(unknown.events.filter(event=>event.kind === "notice").length,1);
assert.equal((unknown.state.warnings as Record<string,number>).future_update,3);
assert.equal(unknown.rows.at(-1)!.content,"Continues");
const cancelled=scenarios.get("cancel-mid-tool")!;
assert.equal(cancelled.rows.length,1);
assert.equal(cancelled.rows[0].id,"call-cancel");
assert.equal(cancelled.rows[0].toolStatus,"interrupted");
assert.equal(cancelled.state.sessionId,"session-fixture");
assert.deepEqual(cancelled.events.at(-1),{kind:"turnState",threadId:"session-fixture",turnId:"session-fixture:turn:1",state:"idle",detail:"interrupted"});
const lost=scenarios.get("lost-stream")!;
assert.equal(lost.rows.length,1);
assert.equal(lost.rows[0].content,"Partial output");
assert.equal(lost.fixture.operations.some(entry=>entry.operation.kind === "submitTurn"),false,"the recorded prefix never synthesizes a prompt resend");
assert.equal(lost.events.some(event=>event.kind === "turnState" && event.state === "idle"),false,"pipe loss is host lifecycle evidence, not an ACP notification");
for (const [status,expected] of Object.entries({pending:"running",in_progress:"running",completed:"completed",failed:"failed",cancelled:"interrupted"})) {
  const row=reduceTool({toolCallId:"call",status,title:"Opaque",kind:"edit"},undefined,1)!;
  assert.equal(row.toolStatus,expected);
  assert.equal(row.toolResult,undefined,"status never invents a tool result");
  assert.equal(row.fileDiffs,undefined,"title and kind never invent diffs");
}
const malformed=reduceTool({toolCallId:"call",status:"__proto__",kind:"__proto__"},undefined,1)!;
assert.equal(malformed.toolStatus,undefined);
assert.equal(malformed.toolKind,"generic");
const context={...messages.fixture.context,threadId:"session-fixture",activeTurnId:"turn",claimedSends:[{sendId:"claimed",threadId:"session-fixture",input:{text:"Hello",attachments:[]}}]};
const frame=(kind:string,value:string,id?:string):RuntimeFrame=>({kind:"notification",method:"session/update",params:{sessionId:"session-fixture",update:{sessionUpdate:kind,...(id ? {messageId:id} : {}),content:{type:"text",text:value}}}});
const cursor={prompt:1,replay:0,loading:false};
const first=reduceStream(context,null,[frame("user_message_chunk","Hel","user-id")],cursor);
assert.equal(first.events.some(event=>event.kind === "sendEvidence"),false);
const second=reduceStream(context,first.state,[frame("user_message_chunk","lo","user-id")],cursor);
assert.equal(second.events.some(event=>event.kind === "sendEvidence"),false,"a matching prefix cannot confirm an unfinished user chunk");
const closed=reduceStream(context,second.state,[frame("agent_thought_chunk","Thinking")],cursor);
assert.ok(closed.events.some(event=>event.kind === "chatItem" && event.item.id === "session-fixture:turn:1:message:user_message_chunk:user-id" && event.item.content === "Hello"),"evidence and its exact user row must share a host validation batch");
assert.deepEqual(closed.events.filter(event=>event.kind === "sendEvidence"),[{kind:"sendEvidence",threadId:"session-fixture",sendId:"claimed",turnId:"turn",itemId:"session-fixture:turn:1:message:user_message_chunk:user-id"}]);
const loaded=reduceStream(context,second.state,[frame("user_message_chunk","Hello","user-id")],{prompt:0,replay:1,loading:true});
assert.equal(loaded.events.some(event=>event.kind === "sendEvidence"),false,"load replay cannot confirm a live send");
assert.equal(Object.keys(loaded.state && (loaded.state as Record<string,JsonValue>).items as object).length,1);
const interleaved=reduceStream(context,null,[frame("agent_message_chunk","first","message-id"),frame("agent_thought_chunk","thought","thought-id"),frame("agent_message_chunk"," second","message-id")],cursor);
const interleavedRows=new Map<string,ChatMsg>();
for (const event of interleaved.events) foldRow(interleavedRows,event);
assert.equal(interleavedRows.get("session-fixture:turn:1:message:agent_message_chunk:message-id")?.content,"first second");
assert.equal(((interleaved.state as Record<string,JsonValue>).items as Record<string,ChatMsg>)["session-fixture:turn:1:message:agent_message_chunk:message-id"].content,"");
const unmatched=reduceStream({...context,claimedSends:[{sendId:"foreign",threadId:"foreign",input:{text:"Hello",attachments:[]}}]},null,[frame("user_message_chunk","Hello"),frame("agent_message_chunk","Answer")],cursor);
assert.equal(unmatched.events.some(event=>event.kind === "sendEvidence"),false,"a foreign claimed send cannot authorize evidence");
const historyFrames:RuntimeFrame[]=[frame("user_message_chunk","One"),
  {kind:"notification",method:"session/update",params:{sessionId:"session-fixture",update:{sessionUpdate:"tool_call",toolCallId:"history-call",status:"completed"}}},
  {kind:"notification",method:"session/update",params:{sessionId:"session-fixture",update:{sessionUpdate:"plan",entries:[]}}},
  frame("agent_thought_chunk","Thought"),frame("agent_message_chunk","First answer")];
const liveOne=reduceStream(context,null,historyFrames,cursor);
const liveSettled=settleStream(context,liveOne.state,"end_turn");
const liveTwo=reduceStream(context,liveSettled.state,[frame("user_message_chunk","Two"),frame("agent_message_chunk","Second answer")],{...cursor,prompt:2});
const history=reduceStream(context,liveTwo.state,[...historyFrames,frame("user_message_chunk","Two"),frame("agent_message_chunk","Second answer")],{prompt:0,replay:1,loading:true});
assert.deepEqual((history.state as Record<string,JsonValue>).items,(liveTwo.state as Record<string,JsonValue>).items,"multi-turn replay preserves ordinals across tool and plan boundaries");
assert.equal(Object.keys((history.state as Record<string,JsonValue>).items as object).length,7);
const replayAgain=reduceStream(context,history.state,[...historyFrames,frame("user_message_chunk","Two"),frame("agent_message_chunk","Second answer")],{prompt:0,replay:2,loading:true});
assert.deepEqual((replayAgain.state as Record<string,JsonValue>).items,(history.state as Record<string,JsonValue>).items,"repeated remote replay preserves all row identities and content");
const withoutTurn=reduceStream({...context,activeTurnId:undefined},null,[frame("user_message_chunk","Hello"),frame("agent_message_chunk","Answer")],cursor);
assert.ok(withoutTurn.events.some(event=>event.kind === "sendEvidence" && event.turnId === "session-fixture:turn:1"));
assert.ok(withoutTurn.events.some(event=>event.kind === "turnState" && event.turnId === "session-fixture:turn:1"));
const bounded=reduceStream(context,null,Array.from({length:100},(_,index)=>({kind:"notification" as const,method:"session/update",params:{sessionId:"session-fixture",update:{sessionUpdate:`future-${index}-${"x".repeat(1000)}`}}})),cursor);
assert.equal(bounded.events.length,32);
assert.ok(bounded.events.every(event=>event.kind === "notice" && event.message.length < 200));
for (const name of names) console.log(`✓ ACP stream ${name}`);
console.log(`ACP stream: ${names.length} scenarios; exact replay row counts, pure reduction and bounded warnings passed`);

const liveFixture=JSON.parse(fs.readFileSync(new URL("./fixtures/live-turns.json",import.meta.url),"utf8")) as {
  firstPrompt:string;secondPrompt:string;updates:JsonValue[];secondUpdate:JsonValue;completedReply:JsonValue;cancelledReply:JsonValue;
};
const updateFrame=(update:JsonValue):RuntimeFrame=>({kind:"notification",method:"session/update",params:{sessionId:"session-fixture",update}});
{
  const midReply=reduceStream(context,null,[frame("agent_message_chunk","before "),frame("agent_message_chunk","[MODE_UPDATE] plan"),frame("agent_message_chunk","after")],{prompt:1,loading:false});
  const rows=new Map<string,ChatMsg>();
  for (const event of midReply.events) foldRow(rows,event);
  assert.deepEqual([...rows.values()].map(row=>row.content),["before after"],"the marker never joins the reply it interrupts");
  for (const [text,id] of [["[MODE_UPDATE] plan and more"],["see [MODE_UPDATE] plan"],["[MODE_UPDATE] "],["[MODE_UPDATE] plan","message-identified"]] as const) {
    const prose=reduceStream(context,null,[frame("agent_message_chunk",text,id)],{prompt:1,loading:false});
    assert.equal(prose.events.some(event=>event.kind === "controlsConfirmed"),false,`assistant text stays text: ${text}`);
    assert.equal(prose.events.some(event=>event.kind === "chatItem" && event.item.content === text),true,`assistant text stays visible: ${text}`);
  }
  console.log("✓ a whole-chunk mode marker updates the mode quietly and prose quoting it stays assistant text");
}
const failures:string[]=[];
for (const scenario of ["no-echo-delivery", "cancel-race", "two-turn-retention", "empty-session-modes"]) {
  try {
    const base={...context,tabId:`live-${scenario}`,threadId:undefined,activeTurnId:undefined,activeTurns:[],claimedSends:[]};
    const init:Operation={kind:"initialize"};
    const ip=runtimePlan(base,init);
    runtimeResponse(base,init,ip.state,{kind:"success",value:{protocolVersion:1,agentCapabilities:{sessionCapabilities:{resume:{}}},authMethods:[]}});
    const open:Operation={kind:"openThread",instructions:"",settings:{mode:"auto-accept-edits"}};
    const op=runtimePlan(base,open);
    const opened=runtimeResponse(base,open,op.state,{kind:"success",value:{sessionId:"session-fixture",modes:{availableModes:[],currentModeId:"auto-accept-edits"}}});
    if (scenario === "empty-session-modes") {
      assert.ok(opened.kind === "complete" && opened.result.kind === "threadLoaded");
      assert.equal(opened.result.settings.mode,undefined);
      const resumed={...base,epoch:2,threadId:"session-fixture"};
      const reinit=runtimePlan(resumed,init);
      runtimeResponse(resumed,init,reinit.state,{kind:"success",value:{protocolVersion:1,agentCapabilities:{sessionCapabilities:{resume:{}}},authMethods:[]}});
      const resume:Operation={kind:"resumeThread",threadId:resumed.threadId,instructions:"",settings:{}};
      const plan=runtimePlan(resumed,resume);
      assert.deepEqual(plan.action,{kind:"request",call:{method:"session/resume",params:{sessionId:resumed.threadId,cwd:resumed.cwd,mcpServers:[]}}});
      const restored=runtimeResponse(resumed,resume,plan.state,{kind:"success",value:{modes:{availableModes:[]}}});
      assert.ok(restored.kind === "complete" && restored.result.kind === "threadLoaded");
      assert.equal(restored.result.threadId,"session-fixture");
      assert.equal(restored.result.settings.mode,undefined);
      console.log(`✓ ACP live ${scenario}`);
      continue;
    }
    const bound={...base,threadId:"session-fixture"};
    const submit:Operation={kind:"submitTurn",threadId:bound.threadId,sendId:"live-send-1",input:{text:liveFixture.firstPrompt,attachments:[]},settings:{}};
    const plan=runtimePlan(bound,submit);
    const claimed={...bound,claimedSends:[{sendId:submit.sendId,threadId:bound.threadId,input:submit.input}]};
    const observedRows=new Map<string,ChatMsg>();
    const observe=(events:readonly AgentEvent[]):void=>{for (const event of events) foldRow(observedRows,event);};
    const requestFirst=scenario === "cancel-race"
      ? runtimeReduce(claimed,null,[{kind:"request",method:"terminal/output",requestId:"read-output",params:{sessionId:bound.threadId,terminalId:"terminal"}}]) : null;
    if (requestFirst) {
      observe(requestFirst.events);
      assert.equal(requestFirst.events.filter(event=>event.kind === "sendEvidence").length,0,"client requests cannot confirm delivery");
      assert.equal(requestFirst.events.filter(event=>event.kind === "chatItem")[0]?.item.type,"user");
    }
    const first=runtimeReduce(claimed,requestFirst?.state ?? null,[updateFrame(liveFixture.updates[0])]);
    observe(first.events);
    if (scenario === "no-echo-delivery") {
      assert.deepEqual(first.events.filter(e=>e.kind === "chatItem").map(e=>[e.item.type,e.item.content,e.item.seq]),[["user","First prompt",1],["thinking","Thinking",2]]);
      assert.equal(first.events.filter(e=>e.kind === "sendEvidence").length,1);
      const echo=runtimeReduce(claimed,first.state,[frame("user_message_chunk","First ","echo"),frame("user_message_chunk","prompt","echo")]);
      assert.equal(echo.events.filter(e=>e.kind === "chatItem" || e.kind === "chatItemAppend" || e.kind === "sendEvidence").length,0,"optional user echoes cannot duplicate or replace the submitted row");
      const foreign=runtimeReduce(claimed,first.state,[{...frame("agent_message_chunk","foreign"),params:{sessionId:"foreign",update:{sessionUpdate:"agent_message_chunk",content:{type:"text",text:"foreign"}}}}]);
      assert.equal(foreign.events.length,0);
    }
    const running={...claimed,activeTurnId:"session-fixture:turn:1",activeTurns:[{threadId:bound.threadId,turnId:"session-fixture:turn:1"}]};
    let streamed=runtimeReduce(running,first.state,[updateFrame(liveFixture.updates[1])]);
    observe(streamed.events);
    if (scenario !== "cancel-race") {
      streamed=runtimeReduce(running,streamed.state,[updateFrame({sessionUpdate:"tool_call_update",toolCallId:"live-tool",status:"completed"})]);
      observe(streamed.events);
    }
    streamed=runtimeReduce(running,streamed.state,[updateFrame(liveFixture.updates[2])]);
    observe(streamed.events);
    for (let index=0;index<100;index++) {
      streamed=runtimeReduce(running,streamed.state,[frame("agent_message_chunk",".","answer")]);
      observe(streamed.events);
    }
    if (scenario === "cancel-race") {
      const interrupt:Operation={kind:"interrupt",threadId:bound.threadId,activeTurnId:running.activeTurnId};
      const cancel=runtimePlan(running,interrupt);
      assert.equal(cancel.action.kind,"notification");
      const ack=runtimeResponse(running,interrupt,cancel.state,{kind:"success",value:{}});
      assert.ok(ack.kind === "complete");
      assert.equal(ack.events?.length ?? 0,0,"cancel write acknowledgement cannot settle a prompt");
    }
    const reason=scenario === "cancel-race" ? "cancelled" : "end_turn";
    const settled=runtimeResponse(running,submit,plan.state,{kind:"success",value:reason === "cancelled" ? liveFixture.cancelledReply : liveFixture.completedReply});
    assert.ok(settled.kind === "complete");
    assert.deepEqual(settled.events?.filter(e=>e.kind === "turnState"),[{kind:"turnState",threadId:bound.threadId,turnId:running.activeTurnId,state:"idle",...(reason === "cancelled" ? {detail:"interrupted"} : {})}]);
    const late=runtimeReduce(bound,streamed.state,[frame("agent_message_chunk","late")]);
    assert.equal(late.events.filter(e=>e.kind === "turnState" || e.kind === "chatItem" || e.kind === "chatItemAppend").length,0,"late frames never reopen a settled prompt");
    const twice=runtimeResponse(bound,submit,plan.state,{kind:"success",value:reason === "cancelled" ? liveFixture.cancelledReply : liveFixture.completedReply});
    assert.ok(twice.kind === "complete");
    assert.equal(twice.events?.filter(e=>e.kind === "turnState").length,0,"prompt reply settles exactly once");
    if (scenario === "two-turn-retention") {
      const next={...submit,sendId:"live-send-2",input:{text:liveFixture.secondPrompt,attachments:[]}};
      const secondPlan=runtimePlan(bound,next);
      const duplicate=runtimeResponse(bound,submit,plan.state,{kind:"success",value:{stopReason:"cancelled"}});
      assert.ok(duplicate.kind === "complete");
      assert.equal(duplicate.events?.length,0,"old reply cannot settle a later prompt");
      const second=runtimeReduce({...bound,claimedSends:[{sendId:next.sendId,threadId:bound.threadId,input:next.input}]},streamed.state,[updateFrame(liveFixture.secondUpdate)]);
      observe(second.events);
      const rows=[...observedRows.values()].sort((a,b)=>a.seq-b.seq);
      const retained=Object.values((second.state as Record<string,JsonValue>).items as Record<string,ChatMsg>);
      assert.ok(retained.filter(row=>row.type !== "tool_call").every(row=>row.content === ""),"plugin state retains identities without message bodies");
      assert.deepEqual(rows.map(row=>[row.type,row.content]),[["user","First prompt"],["thinking","Thinking"],["tool_call",""],["assistant",`First answer${".".repeat(100)}`],["user","Second prompt"],["assistant","Second answer"]]);
      assert.equal(new Set(rows.map(row=>row.id)).size,6);
      assert.deepEqual(rows.map(row=>row.seq),[1,2,3,4,5,6]);
      runtimeResponse(bound,next,secondPlan.state,{kind:"success",value:{stopReason:"end_turn"}});
    }
    console.log(`✓ ACP live ${scenario}`);
  } catch (error) {failures.push(`${scenario}: ${String(error)}`);}
}
assert.deepEqual(failures,[]);

assert.deepEqual(sessionSettings({modes:{availableModes:[{id:"safe",name:"Safe"},{id:"safe",name:"Duplicate"}],currentModeId:"safe"}}),{modes:[{id:"safe",name:"Safe"}],mode:"safe"});

for (const priorTurn of [false,true]) {
  const base={...context,tabId:`window-${priorTurn}`,threadId:undefined,activeTurnId:undefined,activeTurns:[],claimedSends:[]};
  const initialize:Operation={kind:"initialize"};
  runtimeResponse(base,initialize,runtimePlan(base,initialize).state,{kind:"success",value:{protocolVersion:1,agentCapabilities:{},authMethods:[]}});
  const open:Operation={kind:"openThread",instructions:"",settings:{}};
  runtimeResponse(base,open,runtimePlan(base,open).state,{kind:"success",value:{sessionId:"session-fixture"}});
  const own={...base,threadId:"session-fixture"};
  const first:Operation={kind:"submitTurn",threadId:own.threadId,sendId:"one",input:{text:"one",attachments:[]},settings:{}};
  const firstPlan=runtimePlan(own,first);
  let projected:JsonValue=null;
  if (priorTurn) projected=runtimeReduce({...own,claimedSends:[{sendId:"one",threadId:own.threadId,input:first.input}]},null,[frame("agent_message_chunk","prior")]).state;
  const current=priorTurn ? {...first,sendId:"two",input:{text:"two",attachments:[]}} : first;
  if (priorTurn) runtimePlan({...own,activeTurnId:"session-fixture:turn:1",activeTurns:[{threadId:own.threadId,turnId:"session-fixture:turn:1"}]},{...current,activeTurnId:"session-fixture:turn:1"});
  const turn=priorTurn ? 2 : 1;
  const scoped={...own,claimedSends:[{sendId:current.sendId,threadId:own.threadId,input:current.input}],streamWindow:{
    frameSequences:[10],turnRequests:[{sendId:current.sendId,turnId:`session-fixture:turn:${turn}`,afterSequence:10,written:true}],
  }};
  const controls=[
    updateFrame({sessionUpdate:"available_commands_update",availableCommands:[{name:"inspect",description:"Inspect"}]}),
    updateFrame({sessionUpdate:"current_mode_update",currentModeId:"safe"}),
    updateFrame({sessionUpdate:"config_option_update",configOptions:[{id:"model",name:"Model",category:"model",type:"select",currentValue:"opaque",options:[{value:"opaque",name:"Opaque"}]}]}),
    updateFrame({sessionUpdate:"usage_update",used:3,size:1000}),
  ];
  const control=runtimeReduce({...scoped,streamWindow:{...scoped.streamWindow,frameSequences:[7,8,9,10]}},projected,controls);
  assert.equal(control.events.filter(event=>event.kind === "controlsConfirmed").length,3,"prewrite commands, mode and config declarations are retained");
  assert.equal(control.events.filter(event=>event.kind === "context").length,1,"prewrite context usage is retained");
  assert.equal(control.events.some(event=>event.kind === "sendEvidence"),false,"a prewrite control cannot confirm the pending prompt");
  let before=control.state;
  if (!priorTurn) {
    const echo=runtimeReduce({...scoped,streamWindow:{...scoped.streamWindow,frameSequences:[9,10]}},before,[frame("user_message_chunk",current.input.text),frame("agent_thought_chunk","old thought")]);
    assert.equal(echo.events.some(event=>event.kind === "sendEvidence"),false,"a prewrite matching echo cannot mint evidence through chunk closure");
    before=echo.state;
  }
  const previous=runtimeReduce(scoped,before,[frame("agent_message_chunk"," tail")]);
  assert.equal(previous.events.some(event=>event.kind === "sendEvidence"),false);
  if (priorTurn) {
    const continuation=previous.events.find(event=>event.kind === "chatItemAppend");
    assert.ok(continuation?.kind === "chatItemAppend");
    assert.equal(continuation.content," tail");
    assert.equal(continuation.offset,5);
    assert.equal(continuation.itemId,"session-fixture:turn:1:item:2","a concurrent previous-turn chunk stays in its original row");
  }
  if (priorTurn) runtimeResponse(own,first,firstPlan.state,{kind:"success",value:{stopReason:"end_turn"}});
  const next=runtimeReduce({...scoped,streamWindow:{...scoped.streamWindow,frameSequences:[11]}},previous.state,[frame("agent_message_chunk","new first chunk")]);
  assert.equal(next.events.filter(event=>event.kind === "sendEvidence").length,1,"the first postwrite chunk confirms without being dropped");
  assert.equal(next.events.find(event=>event.kind === "sendEvidence")?.turnId,`session-fixture:turn:${turn}`);
  assert.equal(next.events.filter(event=>event.kind === "chatItem").at(-1)?.item.content,"new first chunk");
}
console.log("✓ ACP stream window: prewrite controls, usage and prior chunks retained; only postwrite updates confirm");

const interruptedEcho=reduceStream(context,null,[
  frame("user_message_chunk","Hel","split-user"),
  frame("agent_thought_chunk","thought","split-thought"),
  frame("user_message_chunk","lo","split-user"),
  frame("agent_message_chunk","answer","split-answer"),
],cursor);
assert.equal(interruptedEcho.events.filter(event=>event.kind === "sendEvidence").length,1);
const changedClaim=reduceStream({...context,claimedSends:[{...context.claimedSends[0],threadId:"foreign"}]},second.state,[frame("agent_message_chunk","answer")],cursor);
assert.equal(changedClaim.events.some(event=>event.kind === "sendEvidence"),false);
console.log("✓ ACP interleaved user echo keeps exact matching offsets and rechecks claim authority at closure");

const followupFailures:string[]=[];
for (const scenario of ["tool-identities","overlapping-prompts","overlapping-early-reply","restore-ordinal","astral-chunking"]) {
  try {
    if (scenario === "tool-identities") {
      let state:JsonValue=null;
      const rows=new Map<string,ChatMsg>();
      for (let index=0;index<3;index++) {
        const reduction=reduceStream(context,state,[updateFrame({sessionUpdate:"tool_call",toolCallId:`large-${index}`,title:`Title ${index}`,kind:"execute",status:"in_progress",rawInput:{command:"opaque"},rawOutput:"x".repeat(15900)})],cursor);
        state=reduction.state;
        for (const event of reduction.events) foldRow(rows,event);
      }
      const late=reduceStream(context,state,[updateFrame({sessionUpdate:"tool_call_update",toolCallId:"large-0",status:"completed"})],cursor);
      for (const event of late.events) foldRow(rows,event);
      assert.equal(rows.get("large-0")?.toolTitle,"Title 0");
      assert.deepEqual(rows.get("large-0")?.toolInput,{command:"opaque"});
      assert.equal(rows.get("large-0")?.toolResult,"x".repeat(15900));
      assert.equal(rows.get("large-0")?.seq,1);
      const cancelled=settleStream(context,state,"cancelled");
      for (const event of cancelled.events) foldRow(rows,event);
      assert.equal([...rows.values()].filter(row=>row.toolStatus === "interrupted").length,3);
    } else if (scenario === "astral-chunking") {
      const value="a".repeat(8191)+"🌍".repeat(8192)+"tail".repeat(10000);
      const reduction=runtimeJson(reduceStream(context,null,[frame("agent_message_chunk",value)],cursor),"runtimeReduce");
      const rows=new Map<string,ChatMsg>();
      for (const event of reduction.events) {
        foldRow(rows,event);
        if (event.kind === "chatItem" || event.kind === "chatItemAppend") {
          const content=event.kind === "chatItem" ? event.item.content : event.content;
          assert.equal(reduction.events.filter(event=>event.kind === "chatItem" || event.kind === "chatItemAppend").length,1);
          assert.equal(content.isWellFormed(),true);
        }
      }
      assert.equal([...rows.values()][0].content,value);
    } else {
      const base={...context,tabId:`followup-${scenario}`,activeTurnId:undefined,activeTurns:[],threadId:undefined,claimedSends:[],restoredRowOrdinal:scenario === "restore-ordinal" ? 4 : undefined};
      const init:Operation={kind:"initialize"};
      runtimeResponse(base,init,runtimePlan(base,init).state,{kind:"success",value:{protocolVersion:1,agentCapabilities:{sessionCapabilities:{resume:{}}},authMethods:[]}});
      const open:Operation=scenario === "restore-ordinal" ? {kind:"resumeThread",threadId:"session-fixture",instructions:"",settings:{}} : {kind:"openThread",instructions:"",settings:{}};
      const own=scenario === "restore-ordinal" ? {...base,threadId:"session-fixture"} : base;
      runtimeResponse(own,open,runtimePlan(own,open).state,{kind:"success",value:{sessionId:"session-fixture"}});
      const bound={...base,threadId:"session-fixture"};
      const submit:Operation={kind:"submitTurn",threadId:bound.threadId,sendId:"first",input:{text:"first",attachments:[]},settings:{}};
      const firstPlan=runtimePlan(bound,submit);
      if (scenario === "restore-ordinal") {
        assert.ok(firstPlan.action.kind === "request");
        assert.equal(firstPlan.action.call.target?.turnId,"session-fixture:turn:5");
      } else {
        const first=runtimeReduce(bound,null,[updateFrame({sessionUpdate:"tool_call",toolCallId:"self-send",title:"Send",status:"in_progress"})]);
        const active={...bound,activeTurnId:"session-fixture:turn:1",activeTurns:[{threadId:bound.threadId,turnId:"session-fixture:turn:1"}]};
        const next={...submit,activeTurnId:active.activeTurnId,sendId:"queued",input:{text:"queued",attachments:[]}};
        const secondPlan=runtimePlan(active,next);
        if (scenario === "overlapping-early-reply") {
          const early=runtimeResponse(active,next,secondPlan.state,{kind:"success",value:{stopReason:"end_turn"}});
          assert.ok(early.kind === "complete");
          assert.equal(early.events?.length,0);
        }
        const settled=runtimeResponse(active,submit,firstPlan.state,{kind:"success",value:{stopReason:"end_turn"}});
        assert.ok(settled.kind === "complete");
        assert.deepEqual(settled.events?.filter(event=>event.kind === "turnState").map(event=>[event.turnId,event.state]),[["session-fixture:turn:1","idle"],["session-fixture:turn:2","working"],...(scenario === "overlapping-early-reply" ? [["session-fixture:turn:2","idle"]] : [])]);
        assert.ok(settled.events?.some(event=>(event.kind === "chatItem" || event.kind === "chatItemPatch") && event.item.toolStatus === "interrupted"));
        const secondActive={...active,activeTurnId:"session-fixture:turn:2",activeTurns:[{threadId:bound.threadId,turnId:"session-fixture:turn:2"}]};
        runtimeReduce(secondActive,first.state,[frame("agent_message_chunk","second")]);
        const end=runtimeResponse(secondActive,next,secondPlan.state,{kind:"success",value:{stopReason:"end_turn"}});
        assert.ok(end.kind === "complete");
        assert.deepEqual(end.events?.filter(event=>event.kind === "turnState").map(event=>[event.turnId,event.state]),scenario === "overlapping-early-reply" ? [] : [["session-fixture:turn:2","idle"]]);
        const twice=runtimeResponse(secondActive,next,secondPlan.state,{kind:"success",value:{stopReason:"end_turn"}});
        assert.ok(twice.kind === "complete");
        assert.equal(twice.events?.length,0);
      }
    }
    console.log(`✓ ACP follow-up ${scenario}`);
  } catch (error) { followupFailures.push(`${scenario}: ${String(error)}`); }
}
assert.deepEqual(followupFailures,[]);

let longSession:JsonValue=null;
for (let turn=1;turn<=140;turn++) {
  const update:RuntimeFrame=updateFrame({sessionUpdate:"tool_call",toolCallId:`turn-${turn}-tool`,title:"Retained title ".repeat(8),kind:"execute",status:"in_progress"});
  const running=reduceStream(context,longSession,[update],{...cursor,prompt:turn});
  assert.ok(running.events.some(event=>event.kind === "chatItem" && event.item.toolStatus === "running"),`turn ${turn} admits a running tool after older turns finish`);
  const complete=reduceStream(context,running.state,[updateFrame({sessionUpdate:"tool_call_update",toolCallId:`turn-${turn}-tool`,status:"completed",rawOutput:"done"})],{...cursor,prompt:turn});
  assert.ok(complete.events.some(event=>event.kind === "chatItemPatch" && event.item.toolStatus === "completed"));
  longSession=settleStream(context,complete.state,"end_turn").state;
  assert.ok(Buffer.byteLength(JSON.stringify(longSession))<65536);
}
console.log("✓ ACP tool admission reclaims settled identities across 140 turns");

for (const [name,values] of [
  ["single",['"'.repeat(2*1024*1024-150)]],
  ["aggregate",["x".repeat(3*1024*1024),"y".repeat(1024*1024-600)]],
  ["event-count",Array.from({length:1024},()=>"z")],
  ["control",["\u0001".repeat(Math.floor((4*1024*1024-300)/6))]],
  ["astral",["🌍".repeat(Math.floor((4*1024*1024-300)/4))]],
] as const) {
  const base={...context,tabId:`byte-limit-${name}`,threadId:undefined,activeTurnId:undefined,activeTurns:[],claimedSends:[]};
  const init:Operation={kind:"initialize"};
  runtimeResponse(base,init,runtimePlan(base,init).state,{kind:"success",value:{protocolVersion:1,agentCapabilities:{},authMethods:[]}});
  const open:Operation={kind:"openThread",instructions:"",settings:{}};
  runtimeResponse(base,open,runtimePlan(base,open).state,{kind:"success",value:{sessionId:"session-fixture"}});
  const own={...base,threadId:"session-fixture"};
  runtimePlan(own,{kind:"submitTurn",threadId:own.threadId,sendId:"budget",input:{text:"opaque",attachments:[]},settings:{}});
  const frames=values.map(value=>frame("agent_message_chunk",value,"budget"));
  assert.ok(Buffer.byteLength(JSON.stringify(frames))<=4*1024*1024,"input batch fits host limit");
  const reduction=runtimeReduce(own,null,frames);
  assert.ok(Buffer.byteLength(JSON.stringify(reduction.state))<65536);
  const rows=new Map<string,ChatMsg>();
  for (const event of reduction.events) foldRow(rows,event);
  if (name === "event-count") assert.ok(reduction.events.some(event=>event.kind === "notice" && event.message.includes("runtime reduction event limit")));
  else assert.ok([...rows.values()][0].content.includes("[ACP row truncated: runtime reduction byte limit]"));
  assert.equal([...rows.values()][0].content.isWellFormed(),true);
  const next=runtimeReduce(own,reduction.state,[frame("agent_message_chunk","healthy","budget")]);
  for (const event of next.events) foldRow(rows,event);
  assert.ok([...rows.values()][0].content.endsWith("healthy"),"continuation offset follows the visible prefix and marker");
}
console.log("✓ ACP serialized reduction byte limit degrades only the affected text and preserves continuation offsets");

{
  const content="x".repeat(1024*1024);
  const state={omitted:undefined,callback:()=>null,values:[undefined,()=>null]};
  const event:AgentEvent={kind:"chatItem",threadId:"session-fixture",item:{id:"omitted-sizing",seq:1,type:"assistant",content}};
  const reduced=fitTextReduction(state as unknown as JsonValue,[event]);
  assert.deepEqual(reduced.events,[event],"omitted object values and null array slots cannot consume the text budget");
}
