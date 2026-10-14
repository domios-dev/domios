import assert from "node:assert/strict";
import fs from "node:fs";
import vm from "node:vm";
import type { AgentEvent, AgentRuntime, ChatMsg, JsonValue, RuntimeContext, RuntimeFrame } from "@codeterm/plugin-sdk";

const MAX_RUNTIME_STATE_BYTES = 256 * 1024;
const sandbox = vm.createContext({module:{exports:{}},exports:{}});
sandbox.exports = sandbox.module.exports;
vm.runInContext(fs.readFileSync(new URL("../plugin.js",import.meta.url),"utf8"),sandbox);
const hooks = sandbox.module.exports.default as AgentRuntime;
const ctx = (JSON.parse(fs.readFileSync(new URL("./fixtures/runtime/tool-call.json",import.meta.url),"utf8")) as {context:RuntimeContext}).context;
const thread = ctx.threadId!;

function session() {
  let state: JsonValue = null;
  let largest = 0;
  return {
    feed(...frames: RuntimeFrame[]): AgentEvent[] {
      const events: AgentEvent[] = [];
      for (const frame of frames) {
        const reduced = hooks.runtimeReduce(ctx,state,[frame]);
        state = JSON.parse(JSON.stringify(reduced.state));
        largest = Math.max(largest,JSON.stringify(state).length);
        events.push(...reduced.events);
      }
      return events;
    },
    largest: () => largest,
  };
}
const note = (method: string, params: Record<string,unknown>): RuntimeFrame => ({kind:"notification",method,params:{threadId:thread,...params}});
const rows = (events: AgentEvent[], id: string): ChatMsg[] => events.flatMap(event => event.kind === "chatItem" && event.item.id === id ? [event.item] : []);
let count = 0;
function check(name: string, run: () => void) {run();count++;console.log(`  ✓ state budget ${name}`);}

check("a long session with large tool outputs stays under the host state limit",()=>{
  const live = session();
  live.feed(note("turn/started",{turn:{id:"turn-1",status:"inProgress"}}));
  for (let index = 0; index < 60; index++) {
    const id = `call-${index}`;
    const output = `${index}:`.padEnd(30_000,"x");
    live.feed(note("item/started",{turnId:"turn-1",item:{id,type:"commandExecution",command:"rg big",status:"inProgress"}}),
      note("item/commandExecution/outputDelta",{turnId:"turn-1",itemId:id,delta:output}));
    const completed = rows(live.feed(note("item/completed",{turnId:"turn-1",item:{id,type:"commandExecution",command:"rg big",status:"completed",aggregatedOutput:output,exitCode:0}})),id).at(-1);
    assert.equal(completed?.toolResult,output);
  }
  assert.ok(live.largest() < MAX_RUNTIME_STATE_BYTES,`state peaked at ${live.largest()} bytes`);
});

check("assistant deltas keep accumulating across compaction",()=>{
  const live = session();
  live.feed(note("turn/started",{turn:{id:"turn-2",status:"inProgress"}}),
    note("item/started",{turnId:"turn-2",item:{id:"msg",type:"agentMessage",text:""}}));
  const streamed = ["Hello"," there"," again"].flatMap(delta => rows(live.feed(note("item/agentMessage/delta",{turnId:"turn-2",itemId:"msg",delta})),"msg"));
  assert.equal(streamed.at(-1)?.content,"Hello there again");
});

check("a running tool interrupted later keeps its streamed output",()=>{
  const live = session();
  live.feed(note("turn/started",{turn:{id:"turn-3",status:"inProgress"}}),
    note("item/started",{turnId:"turn-3",item:{id:"hung",type:"commandExecution",command:"sleep",status:"inProgress"}}),
    note("item/commandExecution/outputDelta",{turnId:"turn-3",itemId:"hung",delta:"partial output"}));
  for (let index = 0; index < 20; index++) live.feed(note("turn/plan/updated",{turnId:"turn-3",plan:[{step:`step ${index}`,status:"pending"}]}));
  const interrupted = rows(live.feed(note("turn/completed",{turn:{id:"turn-3",status:"interrupted"}})),"hung").at(-1) as ChatMsg & {toolStatus?:string};
  assert.equal(interrupted.toolStatus,"interrupted");
  assert.equal(interrupted.toolResult,"partial output");
});

check("a single oversized output is tail-truncated, never dropped",()=>{
  const live = session();
  const output = "head".padEnd(600_000,"y") + "TAIL";
  live.feed(note("turn/started",{turn:{id:"turn-4",status:"inProgress"}}),
    note("item/started",{turnId:"turn-4",item:{id:"huge",type:"commandExecution",command:"cat big",status:"inProgress"}}));
  const row = rows(live.feed(note("item/completed",{turnId:"turn-4",item:{id:"huge",type:"commandExecution",command:"cat big",status:"completed",aggregatedOutput:output,exitCode:0}})),"huge").at(-1);
  assert.ok(row?.toolResult?.endsWith("TAIL"));
  assert.ok(row!.toolResult!.length < output.length);
  assert.ok(live.largest() < MAX_RUNTIME_STATE_BYTES);
});

check("late deltas for a finished item never replace its final row",()=>{
  const live = session();
  live.feed(note("turn/started",{turn:{id:"turn-5",status:"inProgress"}}),
    note("item/started",{turnId:"turn-5",item:{id:"done",type:"agentMessage",text:""}}),
    note("item/completed",{turnId:"turn-5",item:{id:"done",type:"agentMessage",text:"Final answer"}}));
  assert.deepEqual(rows(live.feed(note("item/agentMessage/delta",{turnId:"turn-5",itemId:"done",delta:"stray"})),"done"),[]);
});

console.log(`runtime state budget: ${count} checks passed`);
