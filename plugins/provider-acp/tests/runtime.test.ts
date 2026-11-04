import assert from "node:assert/strict";
import fs from "node:fs";
import vm from "node:vm";
import type {AgentRuntime, AgentEvent, JsonValue, Operation, RpcReply, RuntimeAction, RuntimeContext, RuntimeFrame, RuntimePlan, RuntimeStep, StdioAgentRuntimeCapability} from "@codeterm/plugin-sdk";

interface Fixture {
  schemaVersion:1;
  context:RuntimeContext;
  operations:{operation:Operation; context?:RuntimeContext}[];
  rpc:({direction:"outbound"; operation:number; action:RuntimeAction}
    | {direction:"inbound"; operation:number; reply:RpcReply}
    | {direction:"inbound"; frame:RuntimeFrame; context?:RuntimeContext})[];
  expected:{results:{operation:number; step:RuntimeStep}[]; events:AgentEvent[]; planErrors?:{operation:number; code:string}[]};
}
function loadHooks():AgentRuntime {
  const sandbox = vm.createContext({module:{exports:{}}, exports:{}, host:new Proxy({}, {get:()=>{throw new Error("Runtime accessed host");}})});
  sandbox.exports = sandbox.module.exports;
  vm.runInContext(fs.readFileSync(new URL("../plugin.js", import.meta.url), "utf8"), sandbox);
  return sandbox.module.exports.default as AgentRuntime;
}
const hooks = loadHooks();
const plain = <T>(value:T):T => JSON.parse(JSON.stringify(value));
const descriptor = JSON.parse(fs.readFileSync(new URL("./runtime-manifest.json",import.meta.url),"utf8")) as {agentRuntime:StdioAgentRuntimeCapability; scenarios:string[]};
const declaration = JSON.parse(fs.readFileSync(new URL("../plugin.json",import.meta.url),"utf8"));
assert.deepEqual(descriptor.agentRuntime, declaration.capabilities.agentRuntime);
const root = new URL("./fixtures/runtime/",import.meta.url);
const names = fs.readdirSync(root).filter(name=>name.endsWith(".json")).sort();
assert.ok(descriptor.scenarios.length > 0);
assert.equal(new Set(descriptor.scenarios).size,descriptor.scenarios.length);
assert.deepEqual(names,[...descriptor.scenarios].sort());
for (const name of names) {
  const fixture:Fixture = JSON.parse(fs.readFileSync(new URL(name,root),"utf8"));
  assert.equal(fixture.schemaVersion,1);
  const plans = new Map<number,RuntimePlan>();
  const results:{operation:number; step:RuntimeStep}[] = [];
  const events:AgentEvent[] = [];
  let state:JsonValue = null;
  for (const record of fixture.rpc) {
    if (record.direction === "outbound") {
      const entry = fixture.operations[record.operation];
      const plan = plans.get(record.operation) ?? hooks.runtimePlan(entry.context ?? fixture.context,entry.operation);
      assert.deepEqual(plain(plan.action),record.action,`${name} outbound`);
      const action = plan.action;
      if (action.kind === "request" || action.kind === "notification") {
        const call = action.kind === "request" ? action.call : action.notification;
        assert.ok(descriptor.agentRuntime.requestMethods[entry.operation.kind]?.includes(call.method));
      }
      plans.set(record.operation,plan);
    } else if ("reply" in record) {
      const entry = fixture.operations[record.operation];
      const plan = plans.get(record.operation)!;
      assert.ok(plan);
      const step = plain(hooks.runtimeResponse(entry.context ?? fixture.context,entry.operation,plan.state,record.reply));
      if (step.kind === "continue") plans.set(record.operation,step.plan);
      else {
        plans.delete(record.operation);
        results.push({operation:record.operation,step});
        if (step.kind === "complete") events.push(...step.events ?? []);
      }
    } else {
      const reduction = plain(hooks.runtimeReduce(record.context ?? fixture.context,state,[record.frame]));
      state = reduction.state;
      events.push(...reduction.events);
    }
  }
  for (const expected of fixture.expected.planErrors ?? []) {
    const entry = fixture.operations[expected.operation];
    assert.throws(()=>hooks.runtimePlan(entry.context ?? fixture.context,entry.operation),error=> (error as {code:string}).code === expected.code);
  }
  assert.equal(plans.size,0);
  assert.deepEqual(results,fixture.expected.results,`${name} results`);
  assert.deepEqual(events,fixture.expected.events,`${name} events`);
  console.log(`✓ ACP ${name}`);
}
const fixture:Fixture = JSON.parse(fs.readFileSync(new URL("initialize-new-prompt.json",root),"utf8"));
const ctx = {...fixture.context,tabId:"authority-tab"};
const init:Operation = {kind:"initialize"};
const initPlan = hooks.runtimePlan(ctx,init);
hooks.runtimeResponse(ctx,init,initPlan.state,{kind:"success",value:{protocolVersion:1,agentCapabilities:{sessionCapabilities:{resume:{}}},authMethods:[]}});
const open:Operation={kind:"openThread",instructions:"Startup instructions",settings:{}};
const opened = hooks.runtimePlan(ctx,open);
hooks.runtimeResponse(ctx,open,opened.state,{kind:"success",value:{sessionId:"own"}});
const bound={...ctx,threadId:"own"};
const submit:Operation={kind:"submitTurn",threadId:"own",sendId:"send",input:{text:"hello",attachments:[]},settings:{}};
assert.deepEqual(plain(hooks.runtimePlan(bound,submit).action),{kind:"request",call:{method:"session/prompt",params:{sessionId:"own",prompt:[{type:"text",text:"Startup instructions"},{type:"text",text:"hello"}]}}});
for (const changed of [{...bound,epoch:ctx.epoch+1},{...bound,bindingNonce:"foreign"},{...bound,tabId:"foreign"}]) assert.throws(()=>hooks.runtimePlan(changed,submit));
assert.throws(()=>hooks.runtimePlan(bound,{...submit,threadId:"foreign"}));
assert.throws(()=>hooks.runtimePlan(bound,{kind:"forkThread",sourceThreadId:"own",turnId:"turn"}));
assert.deepEqual(plain(hooks.runtimeReduce(bound,{marker:"unchanged"},[{kind:"notification",method:"session/update",params:{sessionId:"foreign",update:{sessionUpdate:"agent_message_chunk"}}}])),{state:{marker:"unchanged"},events:[]});
for (const resume of [false, true, null, "unsupported"]) {
  const context = {...ctx,tabId:`invalid-resume-${String(resume)}`,threadId:"saved"};
  const plan = hooks.runtimePlan(context,init);
  hooks.runtimeResponse(context,init,plan.state,{kind:"success",value:{protocolVersion:1,agentCapabilities:{loadSession:false,sessionCapabilities:{resume}},authMethods:[]}});
  assert.throws(()=>hooks.runtimePlan(context,{kind:"resumeThread",threadId:"saved",instructions:"",settings:{}}),error=>(error as {code:string}).code === "provider_refused");
}
assert.throws(()=>hooks.runtimePlan(bound,{...submit,input:{text:"hello",attachments:[{id:"image",mimeType:"image/png",path:"/work/image.png"}]}}));
const cancel:Operation = {kind:"interrupt",threadId:"own",activeTurnId:"turn"};
assert.throws(()=>hooks.runtimePlan({...bound,activeTurnId:"turn",activeTurns:[{threadId:"foreign",turnId:"turn"}]},cancel),error=>(error as {code:string}).code === "binding_mismatch");
assert.throws(()=>hooks.runtimePlan(bound,cancel),error=>(error as {code:string}).code === "binding_mismatch");
const cache = loadHooks();
const cachedContexts:RuntimeContext[] = [];
function initializeCached(context:RuntimeContext):void {
  const plan = cache.runtimePlan(context,init);
  const step = cache.runtimeResponse(context,init,plan.state,{kind:"success",value:{protocolVersion:1,agentCapabilities:{},authMethods:[]}});
  assert.equal(step.kind,"complete");
}
const fresh:Operation = {kind:"openThread",instructions:"",settings:{}};
for (let index=0;index<257;index++) {
  const context = {...ctx,tabId:`cache-tab-${index}`};
  cachedContexts.push(context);
  if (index === 256) cache.runtimePlan(cachedContexts[0],fresh);
  initializeCached(context);
}
assert.throws(()=>cache.runtimePlan(cachedContexts[1],fresh),error=>(error as {code:string}).code === "protocol_error");
cache.runtimePlan(cachedContexts[0],fresh);
const replacement = {...cachedContexts[256],epoch:2,bindingNonce:"replacement"};
initializeCached(replacement);
assert.throws(()=>cache.runtimePlan(cachedContexts[256],fresh),error=>(error as {code:string}).code === "protocol_error");
cache.runtimePlan(replacement,fresh);
cache.runtimePlan(cachedContexts[2],fresh);
for (let index=257;index<520;index++) initializeCached({...ctx,tabId:`cache-tab-${index}`});
assert.throws(()=>cache.runtimePlan(cachedContexts[0],fresh),error=>(error as {code:string}).code === "protocol_error");
console.log("✓ ACP cache eviction, same-tab epoch replacement and active-turn cancel authority");
console.log(`ACP lifecycle: ${names.length} fixture scenarios and binding regressions passed`);
