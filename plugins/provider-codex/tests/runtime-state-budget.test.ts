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

check("parallel running tools with large output stay under the plugin total budget",()=>{
  const live = session();
  live.feed(note("turn/started",{turn:{id:"turn-6",status:"inProgress"}}));
  for (let index = 0; index < 24; index++) {
    live.feed(note("item/started",{turnId:"turn-6",item:{id:`par-${index}`,type:"commandExecution",command:"rg big",status:"inProgress"}}));
  }
  for (let round = 0; round < 3; round++) {
    for (let index = 0; index < 24; index++) {
      live.feed(note("item/commandExecution/outputDelta",{turnId:"turn-6",itemId:`par-${index}`,delta:`${round}:`.padEnd(12_000,"y")}));
    }
  }
  assert.ok(live.largest() <= 192 * 1024,`state peaked at ${live.largest()} bytes`);
});

check("a 64-frame growing tool batch keeps only its final row under the host bound",()=>{
  const frames = [note("item/started",{turnId:"batch",item:{id:"batch-tool",type:"commandExecution",command:"echo",status:"inProgress"}}),
    ...Array.from({length:62},()=>note("item/commandExecution/outputDelta",{turnId:"batch",itemId:"batch-tool",delta:"x".repeat(2200)})),
    note("item/completed",{turnId:"batch",item:{id:"batch-tool",type:"commandExecution",command:"echo",status:"completed",aggregatedOutput:"x".repeat(62 * 2200),exitCode:0}})];
  const batch = hooks.runtimeReduce(ctx,null,frames);
  const bytes = Buffer.byteLength(JSON.stringify(batch));
  console.log(`    growing batch: ${bytes} bytes`);
  assert.ok(bytes <= 4194304,`reducer returned ${bytes} bytes`);
  assert.equal(rows([...batch.events],"batch-tool").length,1);
  let state: JsonValue = null;
  let final: ChatMsg | undefined;
  for (const frame of frames) {
    const step = hooks.runtimeReduce(ctx,state,[frame]);
    state = step.state;
    final = rows([...step.events],"batch-tool").at(-1) ?? final;
  }
  assert.deepEqual(JSON.parse(JSON.stringify(batch.state)),JSON.parse(JSON.stringify(state)));
  assert.deepEqual(JSON.parse(JSON.stringify(rows([...batch.events],"batch-tool")[0])),JSON.parse(JSON.stringify(final)));
  assert.equal(final?.toolResult,"x".repeat(62 * 2200));
});

check("unique large rows share an aggregate budget including UTF-8 and JSON escaping",()=>{
  const output = '\u0000😀'.repeat(18000) + "TAIL";
  const frames = Array.from({length:16},(_,index)=>note("item/completed",{turnId:"wide",item:{id:`wide-${index}`,type:"commandExecution",command:"echo",status:"completed",aggregatedOutput:output,exitCode:0}}));
  const batch = hooks.runtimeReduce(ctx,null,frames);
  const bytes = Buffer.byteLength(JSON.stringify(batch));
  console.log(`    wide batch: ${bytes} bytes`);
  assert.ok(bytes <= 4194304,`reducer returned ${bytes} bytes`);
  assert.equal(batch.events.filter(event=>event.kind === "chatItem").length,16);
  for (const event of batch.events) {
    if (event.kind !== "chatItem") continue;
    assert.equal(event.item.toolStatus,"completed");
    assert.ok(event.item.toolResult?.endsWith("TAIL"));
    assert.ok((event.item.content + event.item.toolResult).includes("[earlier output truncated]"));
  }
  let state: JsonValue = null;
  for (const frame of frames) state = hooks.runtimeReduce(ctx,state,[frame]).state;
  assert.deepEqual(JSON.parse(JSON.stringify(batch.state)),JSON.parse(JSON.stringify(state)));
});

check("aggregate tail truncation preserves every final tool identity and outcome",()=>{
  const output = "HEAD" + "z".repeat(200000) + "TAIL";
  const frames = Array.from({length:16},(_,index)=>note("item/completed",{turnId:"tails",item:{id:`tail-${index}`,type:"commandExecution",command:"echo",status:"completed",aggregatedOutput:output,exitCode:index % 2}}));
  assert.ok(Buffer.byteLength(JSON.stringify(frames)) < 4194304);
  const batch = hooks.runtimeReduce(ctx,null,frames);
  assert.ok(Buffer.byteLength(JSON.stringify(batch)) <= 4194304);
  assert.equal(batch.events.length,16);
  batch.events.forEach((event,index)=>{
    assert.equal(event.kind,"chatItem");
    if (event.kind !== "chatItem") return;
    assert.equal(event.item.id,`tail-${index}`);
    assert.equal(event.item.toolExitCode,index % 2);
    assert.equal(event.item.toolError,index % 2 !== 0);
    assert.ok(event.item.toolResult?.startsWith("[earlier output truncated]"));
    assert.ok(event.item.toolResult?.endsWith("TAIL"));
    assert.ok(event.item.toolResult!.length < output.length);
    assert.ok(event.item.content.length > 0);
  });
});

check("coalescing interleaved rows retains final update positions and other event order",()=>{
  const frames = [note("item/agentMessage/delta",{itemId:"a",delta:"first"}),
    note("thread/tokenUsage/updated",{tokenUsage:{total:{inputTokens:1,outputTokens:2}}}),
    note("item/agentMessage/delta",{itemId:"b",delta:"other"}),
    note("item/agentMessage/delta",{itemId:"a",delta:" last"})];
  const batch = hooks.runtimeReduce(ctx,null,frames);
  assert.deepEqual(Array.from(batch.events,event=>event.kind === "chatItem" ? [event.item.id,event.item.seq,event.item.content] : event.kind),["usage",["b",2,"other"],["a",1,"first last"]]);
  let state: JsonValue = null;
  for (const frame of frames) state = hooks.runtimeReduce(ctx,state,[frame]).state;
  assert.deepEqual(JSON.parse(JSON.stringify(batch.state)),JSON.parse(JSON.stringify(state)));
});

function freezeInput<T>(value: T): T {
  if (value !== null && typeof value === "object") {
    for (const child of Object.values(value as Record<string,unknown>)) freezeInput(child);
    Object.freeze(value);
  }
  return value;
}

check("restoring live state avoids serializing its input and keeps frozen tool state isolated",()=>{
  const turnId = "opaque-copy-turn";
  const seed = [note("turn/started",{turn:{id:turnId,status:"inProgress"}}),
    ...Array.from({length:12},(_,index)=>[
      note("item/started",{turnId,item:{id:`opaque-copy-${index}`,type:"commandExecution",command:"opaque-command",status:"inProgress"}}),
      note("item/commandExecution/outputDelta",{turnId,itemId:`opaque-copy-${index}`,delta:"x".repeat(8192)}),
    ]).flat()];
  const state = freezeInput(hooks.runtimeReduce(ctx,null,seed).state);
  const before = JSON.stringify(state);
  const runtimeJson = vm.runInContext("JSON",sandbox) as typeof JSON;
  const stringify = runtimeJson.stringify;
  let inputSerializations = 0;
  runtimeJson.stringify = ((value: unknown,...args: unknown[]) => {
    if (value === state) inputSerializations++;
    return Reflect.apply(stringify,runtimeJson,[value,...args]);
  }) as typeof JSON.stringify;
  let result: ReturnType<AgentRuntime["runtimeReduce"]>;
  try {
    result = hooks.runtimeReduce(ctx,state,[note("item/commandExecution/outputDelta",{turnId,itemId:"opaque-copy-0",delta:"-tail"})]);
  } finally { runtimeJson.stringify = stringify; }
  assert.equal(inputSerializations,0,"restoring the supplied state must not stringify the whole input blob");
  assert.equal(JSON.stringify(state),before,"cached reducer input remains reusable");
  assert.equal(rows([...result.events],"opaque-copy-0").at(-1)?.toolResult,"x".repeat(8192)+"-tail");
  assert.ok(Buffer.byteLength(JSON.stringify(result.state)) <= 192 * 1024);
});

check("copied metadata preserves reused-input reasoning, ancestry, evidence and terminal fences",()=>{
  const context: RuntimeContext = {...ctx,claimedSends:["old","new"].map(id=>({sendId:`opaque-send-${id}`,threadId:thread,input:{text:`opaque-payload-${id}`,attachments:[]}}))};
  const user = (id:string) => ({id:`opaque-user-${id}`,type:"userMessage",clientId:`opaque-send-${id}`,content:[{type:"text",text:`opaque-payload-${id}`}]});
  const question = (id:string) => ({id,type:"agentMessage",delivery:"async",questions:[{id,question:"opaque-question",options:["opaque-option"]}]});
  const child = (id:string) => note("thread/started",{thread:{id,source:{subAgent:{thread_spawn:{parent_thread_id:thread}}}}});
  const spawn = (id:string,agent:string) => note("item/started",{turnId:"opaque-live",item:{id,type:"collabAgentToolCall",tool:"spawnAgent",status:"inProgress",receiverThreadIds:[agent],agentsStates:{[agent]:{status:"running"}},prompt:"opaque-goal"}});
  const state = hooks.runtimeReduce(context,null,[
    note("turn/started",{turn:{id:"opaque-old",status:"inProgress"}}),
    note("turn/started",{turn:{id:"opaque-live",status:"inProgress"}}),
    note("item/started",{turnId:"opaque-live",item:{id:"opaque-thinking",type:"reasoning",summary:["opaque-summary"],content:["opaque-content"]}}),
    note("item/started",{turnId:"opaque-live",item:{id:"opaque-tool",type:"commandExecution",command:"opaque-command",status:"inProgress"}}),
    note("item/commandExecution/outputDelta",{turnId:"opaque-live",itemId:"opaque-tool",delta:"opaque-progress"}),
    note("item/started",{turnId:"opaque-live",item:{id:"opaque-file",type:"fileChange",status:"inProgress",changes:[{path:"opaque.txt",kind:{type:"update"},diff:"@@ -1 +1 @@\n-old\n+new"}]}}),
    child("opaque-child"),spawn("opaque-spawn","opaque-child"),
    {kind:"notification",method:"turn/started",params:{threadId:"opaque-child",turn:{id:"opaque-child-old"}}},
    {kind:"notification",method:"turn/completed",params:{threadId:"opaque-child",turn:{id:"opaque-child-old"}}},
    {kind:"notification",method:"turn/started",params:{threadId:"opaque-child",turn:{id:"opaque-child-live"}}},
    note("item/completed",{turnId:"opaque-live",item:question("opaque-question-old")}),
    note("item/completed",{turnId:"opaque-live",item:{id:"opaque-compact-old",type:"contextCompaction"}}),
    note("item/completed",{turnId:"opaque-live",item:user("old")}),
  ]).state;
  const before = JSON.stringify(state);
  const frames: RuntimeFrame[] = [
    note("item/reasoning/summaryTextDelta",{turnId:"opaque-live",itemId:"opaque-thinking",summaryIndex:0,delta:"-tail"}),
    note("item/fileChange/outputDelta",{turnId:"opaque-live",itemId:"opaque-file",delta:"opaque-save-progress"}),
    note("item/completed",{turnId:"opaque-live",item:{id:"opaque-file",type:"fileChange",status:"completed",changes:[{path:"opaque.txt",kind:{type:"update"},diff:"@@ -1 +1 @@\n-old\n+final"}]}}),
    child("opaque-child-next"),spawn("opaque-spawn-next","opaque-child-next"),
    {kind:"notification",method:"turn/completed",params:{threadId:"opaque-child",turn:{id:"opaque-child-live"}}},
    note("item/completed",{turnId:"opaque-live",item:question("opaque-question-new")}),
    note("item/completed",{turnId:"opaque-live",item:{id:"opaque-compact-new",type:"contextCompaction"}}),
    {kind:"notification",method:"item/completed",params:{threadId:"opaque-foreign",turnId:"opaque-live",item:user("new")}},
    note("item/completed",{turnId:"opaque-live",item:user("new")}),
    note("item/completed",{turnId:"opaque-live",item:user("new")}),
    note("turn/started",{turn:{id:"opaque-next",status:"inProgress"}}),
    note("turn/completed",{turn:{id:"opaque-live",status:"completed"}}),
  ];
  const expected = hooks.runtimeReduce(context,JSON.parse(before),frames);
  freezeInput(state);
  const progress = hooks.runtimeReduce(context,state,[frames[1]]);
  const saving = rows([...progress.events],"opaque-file").at(-1);
  assert.equal(saving?.toolStatus,"running");
  assert.equal(saving?.toolResult,"opaque-save-progress");
  assert.equal(saving?.fileDiffs?.[0]?.path,"opaque.txt");
  const actual = hooks.runtimeReduce(context,state,frames);
  assert.equal(JSON.stringify(state),before);
  assert.deepEqual(JSON.parse(JSON.stringify(actual)),JSON.parse(JSON.stringify(expected)));
  assert.deepEqual(JSON.parse(JSON.stringify(hooks.runtimeReduce(context,state,frames))),JSON.parse(JSON.stringify(actual)),"the same frozen input can be replayed as a pure fixture, never as live input");
  assert.equal(rows([...actual.events],"opaque-thinking").at(-1)?.content,"opaque-summary-tail");
  const tool = rows([...actual.events],"opaque-tool").at(-1);
  assert.equal(tool?.toolStatus,"abandoned");
  assert.equal(tool?.toolResult,"opaque-progress");
  const files = rows([...actual.events],"opaque-file");
  const file = files.at(-1);
  assert.equal(file?.toolStatus,"completed");
  assert.equal(file?.fileDiffs?.[0]?.path,"opaque.txt");
  assert.equal(actual.events.filter(event=>event.kind === "sendEvidence" && event.sendId === "opaque-send-new").length,1);
  const final = actual.state as Record<string,JsonValue>;
  assert.equal(final.activeTurnId,"opaque-next");
  const parents = final.parents as Record<string,JsonValue>;
  assert.equal(parents["opaque-child"],thread);
  assert.equal(parents["opaque-child-next"],thread);
  for (const changed of [{...context,epoch:context.epoch+1},{...context,bindingNonce:"opaque-other-binding"}]) {
    const reset = hooks.runtimeReduce(changed,state,[]);
    assert.equal(reset.events.length,0);
    assert.deepEqual(Object.keys((reset.state as Record<string,JsonValue>).items as Record<string,JsonValue>),[]);
    assert.equal(JSON.stringify(state),before);
  }
});

type ReductionSize = { bytes: number; units: number };
function observeReductionSizes(run: (sizes: ReductionSize[]) => void): void {
  const runtimeJson = vm.runInContext("JSON",sandbox) as typeof JSON;
  const stringify = runtimeJson.stringify;
  const sizes: ReductionSize[] = [];
  runtimeJson.stringify = ((value: unknown,...args: unknown[]) => {
    const serialized = Reflect.apply(stringify,runtimeJson,[value,...args]);
    if (value !== null && typeof value === "object" && "state" in value && "events" in value
      && Array.isArray((value as {events:unknown}).events)) {
      assert.equal(typeof serialized,"string");
      sizes.push({bytes:Buffer.byteLength(serialized),units:serialized.length});
    }
    return serialized;
  }) as typeof JSON.stringify;
  try { run(sizes); } finally { runtimeJson.stringify = stringify; }
}

check("a below-budget reduction counts its unchanged serialized result once",()=>{
  const state = freezeInput(hooks.runtimeReduce(ctx,null,[note("turn/started",{turn:{id:"opaque-count-turn",status:"inProgress"}})]).state);
  const before = JSON.stringify(state);
  observeReductionSizes(sizes=>{
    const batch = hooks.runtimeReduce(ctx,state,[
      note("item/agentMessage/delta",{turnId:"opaque-count-turn",itemId:"opaque-a",delta:"first"}),
      note("thread/tokenUsage/updated",{tokenUsage:{total:{inputTokens:1,outputTokens:2}}}),
      note("item/agentMessage/delta",{turnId:"opaque-count-turn",itemId:"opaque-b",delta:"other"}),
      note("item/agentMessage/delta",{turnId:"opaque-count-turn",itemId:"opaque-a",delta:" last"}),
    ]);
    assert.equal(sizes.length,1,"old final guard redundantly counts this same immutable result twice");
    assert.equal(sizes[0].bytes,Buffer.byteLength(JSON.stringify(batch)));
    assert.ok(sizes[0].bytes < 4194304);
    assert.deepEqual(Array.from(batch.events,event=>event.kind === "chatItem" ? [event.item.id,event.item.content] : event.kind),
      ["usage",["opaque-b","other"],["opaque-a","first last"]]);
    assert.equal(JSON.stringify(state),before);
  });
});

check("aggregate counting refreshes after shrinking and enforces UTF8 rather than code units",()=>{
  const output = '\u0000😀'.repeat(14000) + "OPAQUE-TAIL";
  const frames = freezeInput(Array.from({length:16},(_,index)=>note("item/completed",{turnId:"opaque-utf8",item:{
    id:`opaque-utf8-${index}`,type:"commandExecution",command:"opaque-command",status:"completed",aggregatedOutput:output,exitCode:index % 2,
  }})));
  assert.ok(Buffer.byteLength(JSON.stringify(frames)) < 4194304);
  observeReductionSizes(sizes=>{
    const batch = hooks.runtimeReduce(ctx,null,frames);
    assert.equal(sizes.length,2,"initial count plus one changed-result count, without a duplicated final guard");
    assert.ok(sizes[0].units < 4194304 && sizes[0].bytes > 4194304,"UTF8 expansion crosses the bound while code-unit count does not");
    assert.ok(sizes[1].bytes <= 4194304 && sizes[1].bytes < sizes[0].bytes);
    assert.equal(sizes[1].bytes,Buffer.byteLength(JSON.stringify(batch)));
    assert.equal(batch.events.length,16);
    batch.events.forEach((event,index)=>{
      assert.equal(event.kind,"chatItem");
      if (event.kind !== "chatItem") return;
      assert.equal(event.item.id,`opaque-utf8-${index}`);
      assert.equal(event.item.toolResult,output);
      assert.equal(event.item.toolExitCode,index % 2);
      assert.equal(event.item.toolError,index % 2 !== 0);
      assert.equal(event.item.toolStatus,index % 2 ? "failed" : "completed");
    });
  });
});

check("metadata that cannot shrink still refuses the same bounded output",()=>{
  const options = Array.from({length:64},(_,index)=>`${index}:`.padEnd(35000,"q"));
  const frames = freezeInput([note("item/completed",{turnId:"opaque-overflow",item:{id:"opaque-question",type:"agentMessage",delivery:"async",
    questions:[{id:"opaque-question",question:"opaque-prompt",options}],
  }})]);
  const before = JSON.stringify(frames);
  assert.ok(Buffer.byteLength(before) < 4194304);
  observeReductionSizes(sizes=>{
    assert.throws(()=>hooks.runtimeReduce(ctx,null,frames),/runtime batch metadata exceeds output budget/);
    assert.ok(sizes.length > 1 && sizes.every(size=>size.bytes > 4194304));
    assert.equal(JSON.stringify(frames),before);
  });
});

console.log(`runtime state budget: ${count} checks passed`);
