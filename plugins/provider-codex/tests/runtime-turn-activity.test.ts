import assert from "node:assert/strict";
import fs from "node:fs";
import vm from "node:vm";
import type { AgentRuntime, JsonValue, RuntimeContext, RuntimeFrame } from "@codeterm/plugin-sdk";

const sandbox = vm.createContext({module:{exports:{}},exports:{}});
sandbox.exports = sandbox.module.exports;
vm.runInContext(fs.readFileSync(new URL("../plugin.js",import.meta.url),"utf8"),sandbox);
const hooks = sandbox.module.exports.default as AgentRuntime;

const names = ["multi-turn", "tool-call", "interrupt", "interrupt-during-tool-work", "captured/streaming-short", "captured/approval", "captured/interrupt"];
for (const name of names) {
  const fixture = JSON.parse(fs.readFileSync(new URL(`./fixtures/runtime/${name}.json`,import.meta.url),"utf8")) as {
    context: RuntimeContext;
    rpc: {frame?:RuntimeFrame}[];
  };
  fixture.context.threadId ??= (fixture.rpc.find(record=>record.frame?.method === "turn/started")?.frame?.params as {threadId:string}).threadId;
  let state: JsonValue = null;
  let active: string | undefined;
  let working = false;
  let waiting = false;
  for (const {frame} of fixture.rpc) {
    if (!frame) continue;
    const params = frame.params as {threadId?:string;turn?:{id:string}};
    if (params.threadId === fixture.context.threadId) {
      if (frame.method === "turn/started") active = params.turn?.id;
      if (frame.method === "turn/completed" && params.turn?.id === active) active = undefined;
    }
    const result = hooks.runtimeReduce(fixture.context,state,[frame]);
    state = result.state;
    for (const event of result.events) {
      if (event.kind === "turnState" && event.threadId === fixture.context.threadId) { working = event.state === "working"; waiting = event.state === "needs_input"; }
    }
    assert.equal(working || (!!active && waiting),!!active,`${name}: ${frame.method}`);
  }
  assert.equal(working,false,name);

  const note = (method:string,params:Record<string,JsonValue>):RuntimeFrame => ({kind:"notification",method,params:{threadId:fixture.context.threadId!,...params}});
  for (const frame of [
    note("item/started",{item:{id:"background",type:"commandExecution",command:"fixture",status:"inProgress"}}),
    note("item/commandExecution/outputDelta",{itemId:"background",delta:"output"}),
    note("thread/status/changed",{status:{type:"active",activeFlags:[]}}),
    note("item/completed",{item:{id:"background",type:"commandExecution",command:"fixture",status:"completed",exitCode:0}}),
    note("item/started",{item:{id:"late-compaction",type:"contextCompaction"}}),
  ]) {
    const result = hooks.runtimeReduce(fixture.context,state,[frame]);
    state = result.state;
    assert.equal(result.events.some(event=>event.kind === "turnState" && event.state === "working"),false,`${name}: late ${frame.method}`);
    if (frame.method.startsWith("item/") && frame.method !== "item/started") assert.ok(result.events.some(event=>event.kind === "chatItem"),"background output remains visible");
  }
}
for (const outcome of ["completed", "interrupted", "failed"]) {
  const ctx = JSON.parse(fs.readFileSync(new URL("./fixtures/runtime/tool-call.json",import.meta.url),"utf8")).context as RuntimeContext;
  let state:JsonValue = null;
  let working = false;
  for (const [method,params] of [
    ["turn/started",{turn:{id:"background-turn"}}],
    ["item/started",{turnId:"background-turn",item:{id:"terminal",type:"commandExecution",command:"fixture",status:"inProgress"}}],
    ["turn/completed",{turn:{id:"background-turn",status:outcome}}],
    ["item/commandExecution/outputDelta",{itemId:"terminal",delta:"background output"}],
    ["thread/status/changed",{status:{type:"active",activeFlags:[]}}],
  ] as [string,Record<string,JsonValue>][]) {
    const result = hooks.runtimeReduce(ctx,state,[{kind:"notification",method,params:{threadId:ctx.threadId!,...params}}]);
    state = result.state;
    for (const event of result.events) if (event.kind === "turnState") working = event.state === "working";
    if (method === "turn/started" || method === "item/started") assert.equal(working,true);
    else assert.equal(working,false,`${outcome}: ${method}`);
  }
}
console.log(`runtime turn activity: ${names.length} recorded sequences and background tails passed`);
