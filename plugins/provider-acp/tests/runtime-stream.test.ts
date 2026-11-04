import assert from "node:assert/strict";
import fs from "node:fs";
import type { AgentEvent, ChatMsg, JsonValue, RuntimeContext, RuntimeFrame, Operation } from "@codeterm/plugin-sdk";
import { reduceStream, settleStream, type StreamCursor } from "../src/runtimeReduce";
import { reduceTool } from "../src/runtimeReduceTools";

interface Fixture {
  context:RuntimeContext;
  operations:{operation:Operation;context?:RuntimeContext}[];
  rpc:({direction:"outbound";operation:number} | {direction:"inbound";operation:number;reply:{kind:string;value?:JsonValue}} |
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
  const collect=(batch:readonly AgentEvent[]):void=>{for (const event of batch) {events.push(event);if(event.kind === "chatItem") rows.set(event.item.id,event.item);}};
  for (const record of fixture.rpc) {
    if (record.direction === "outbound") {
      const op=fixture.operations[record.operation].operation;
      if (op.kind === "submitTurn") {cursor.prompt++;cursor.loading=false;}
      if (op.kind === "resumeThread") {cursor.replay++;cursor.prompt=0;cursor.loading=true;}
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
assert.equal(replayed.rows.length,3,"repeated session/load must keep exactly three transcript rows");
assert.deepEqual(replayed.rows,messages.rows,"live and replay retain the same row ids, order, content and sequence");
assert.equal(replayed.state.replay,2);
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
assert.deepEqual(cancelled.events.at(-1),{kind:"turnState",threadId:"session-fixture",state:"idle",detail:"interrupted"});
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
assert.ok(closed.events.some(event=>event.kind === "chatItem" && event.item.id === "user-id" && event.item.content === "Hello"),"evidence and its exact user row must share a host validation batch");
assert.deepEqual(closed.events.filter(event=>event.kind === "sendEvidence"),[{kind:"sendEvidence",threadId:"session-fixture",sendId:"claimed",turnId:"turn",itemId:"user-id"}]);
const loaded=reduceStream(context,second.state,[frame("user_message_chunk","Hello","user-id")],{prompt:0,replay:1,loading:true});
assert.equal(loaded.events.some(event=>event.kind === "sendEvidence"),false,"load replay cannot confirm a live send");
assert.equal(Object.keys(loaded.state && (loaded.state as Record<string,JsonValue>).items as object).length,1);
const interleaved=reduceStream(context,null,[frame("agent_message_chunk","first","message-id"),frame("agent_thought_chunk","thought","thought-id"),frame("agent_message_chunk"," second","message-id")],cursor);
assert.equal(((interleaved.state as Record<string,JsonValue>).items as Record<string,ChatMsg>)["message-id"].content,"first second");
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
const withoutTurn=reduceStream({...context,activeTurnId:undefined},null,[frame("user_message_chunk","Hello"),frame("agent_message_chunk","Answer")],cursor);
assert.ok(withoutTurn.events.some(event=>event.kind === "sendEvidence" && event.turnId === "session-fixture:turn:1"));
assert.ok(withoutTurn.events.some(event=>event.kind === "turnState" && event.turnId === "session-fixture:turn:1"));
const bounded=reduceStream(context,null,Array.from({length:100},(_,index)=>({kind:"notification" as const,method:"session/update",params:{sessionId:"session-fixture",update:{sessionUpdate:`future-${index}-${"x".repeat(1000)}`}}})),cursor);
assert.equal(bounded.events.length,32);
assert.ok(bounded.events.every(event=>event.kind === "notice" && event.message.length < 200));
for (const name of names) console.log(`✓ ACP stream ${name}`);
console.log(`ACP stream: ${names.length} scenarios; exact replay row counts, pure reduction and bounded warnings passed`);
