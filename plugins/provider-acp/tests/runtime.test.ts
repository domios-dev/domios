import assert from "node:assert/strict";
import fs from "node:fs";
import vm from "node:vm";
import { assertRuntimeOperation } from "./runtimeAuthority";
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
  const hooks=sandbox.module.exports.default as AgentRuntime;
  return {...hooks,
    runtimePlan(ctx,op) { assertRuntimeOperation(ctx,op);return hooks.runtimePlan(ctx,op); },
    runtimeResponse(ctx,op,state,reply) { assertRuntimeOperation(ctx,op);return hooks.runtimeResponse(ctx,op,state,reply); },
  };
}
const hooks = loadHooks();
function assertPlanRefusal(plan:RuntimePlan):void {
  assert.equal(plan.action.kind,"refused");
  if (plan.action.kind !== "refused") throw new Error("Expected a local plan refusal");
  assert.ok(plan.action.reason.length > 0 && plan.action.reason.length <= 4096);
  assert.equal(plan.state,null);
}
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
  for (const entry of fixture.operations) assertRuntimeOperation(entry.context ?? fixture.context,entry.operation);
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
    const plan = hooks.runtimePlan(entry.context ?? fixture.context,entry.operation);
    assertPlanRefusal(plan);
    assert.equal(expected.code,"provider_refused",`${name} operation ${expected.operation}`);
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
for (const changed of [{...bound,epoch:ctx.epoch+1},{...bound,bindingNonce:"foreign"},{...bound,tabId:"foreign"}]) assertPlanRefusal(hooks.runtimePlan(changed,submit));
assert.throws(()=>hooks.runtimePlan(bound,{...submit,threadId:"foreign"}));
assert.throws(()=>hooks.runtimePlan(bound,{kind:"forkThread",sourceThreadId:"own",turnId:"turn"}),/fork requires an unbound runtime/);
assert.deepEqual(plain(hooks.runtimeReduce(bound,{marker:"unchanged"},[{kind:"notification",method:"session/update",params:{sessionId:"foreign",update:{sessionUpdate:"agent_message_chunk"}}}])),{state:{marker:"unchanged"},events:[]});
for (const resume of [false, true, null, "unsupported"]) {
  const context = {...ctx,tabId:`invalid-resume-${String(resume)}`,threadId:"saved"};
  const plan = hooks.runtimePlan(context,init);
  hooks.runtimeResponse(context,init,plan.state,{kind:"success",value:{protocolVersion:1,agentCapabilities:{loadSession:false,sessionCapabilities:{resume}},authMethods:[]}});
  assertPlanRefusal(hooks.runtimePlan(context,{kind:"resumeThread",threadId:"saved",instructions:"",settings:{}}));
}
assertPlanRefusal(hooks.runtimePlan(bound,{...submit,input:{text:"hello",attachments:[{id:"image",mimeType:"image/png",path:"/work/image.png"}]}}));
const cancel:Operation = {kind:"interrupt",threadId:"own",activeTurnId:"turn"};
assert.throws(()=>hooks.runtimePlan({...bound,activeTurnId:"turn",activeTurns:[{threadId:"foreign",turnId:"turn"}]},cancel),/active descendant has no known ancestry/);
assert.throws(()=>hooks.runtimePlan(bound,cancel),/interrupt retargets turn/);
assert.equal(hooks.runtimePlan(bound,submit).action.kind,"request","ordinary refusals preserve another operation's negotiated session");
assert.throws(()=>hooks.runtimePlan({...bound,activeTurnId:"turn"},submit),/submit retargets turn/,"the original native replay mismatch must fail before invoking the JS VM");
const authorityCases:{context:RuntimeContext;operation:Operation;error:RegExp}[]=[
  {context:bound,operation:open,error:/open replaces bound thread/},
  {context:bound,operation:{kind:"resumeThread",threadId:"foreign",instructions:"",settings:{}},error:/resume retargets thread/},
  {context:bound,operation:{kind:"readHistory",threadId:"foreign"},error:/operation retargets thread/},
  {context:bound,operation:{kind:"control",threadId:"foreign",action:"opaque",value:null},error:/operation retargets thread/},
  {context:bound,operation:{kind:"backgroundProcesses",threadId:"foreign",action:{kind:"clean"}},error:/unowned thread/},
  {context:bound,operation:{kind:"answer",requestId:"unknown",answer:{kind:"cancel"}},error:/answer targets unknown request/},
  {context:bound,operation:{kind:"answerQuestion",threadId:"foreign",itemId:"item",answers:[{questionId:"q",values:[]}]},error:/operation retargets thread/},
  {context:bound,operation:{kind:"answerQuestion",threadId:"own",itemId:"item",answers:[]},error:/invalid question answer/},
  {context:bound,operation:{kind:"answerQuestion",threadId:"own",itemId:"item",answers:[{questionId:"q",values:[]},{questionId:"q",values:[]}]},error:/invalid question answer values/},
];
for (const entry of authorityCases) assert.throws(()=>hooks.runtimePlan(entry.context,entry.operation),entry.error);
assertRuntimeOperation({...bound,activeTurnId:"turn"},{...submit,activeTurnId:"turn"});
assertRuntimeOperation({...bound,activeTurnId:"turn",activeTurns:[{threadId:"own",turnId:"turn"}]},cancel);
assertRuntimeOperation({...bound,pendingRequest:{requestId:"pending",method:"permission",params:null}},{kind:"answer",requestId:"pending",answer:{kind:"cancel"}});
assertRuntimeOperation(bound,{kind:"answerQuestion",threadId:"own",itemId:"item",answers:[{questionId:"q",values:["yes"]}]});
assertRuntimeOperation({...bound,threadParents:[{threadId:"child",parentThreadId:"own"}]},{kind:"readHistory",threadId:"child"});
assertRuntimeOperation({...bound,threadParents:[{threadId:"child",parentThreadId:"own"}]},{kind:"backgroundProcesses",threadId:"child",action:{kind:"list",cursor:null}});
assertRuntimeOperation(ctx,{kind:"forkThread",sourceThreadId:"own",turnId:"turn"});
console.log("✓ ACP VM operation authority matches native submit, interrupt, thread, request and question boundaries");
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
assertPlanRefusal(cache.runtimePlan(cachedContexts[1],fresh));
cache.runtimePlan(cachedContexts[0],fresh);
const replacement = {...cachedContexts[256],epoch:2,bindingNonce:"replacement"};
initializeCached(replacement);
assertPlanRefusal(cache.runtimePlan(cachedContexts[256],fresh));
cache.runtimePlan(replacement,fresh);
cache.runtimePlan(cachedContexts[2],fresh);
for (let index=257;index<520;index++) initializeCached({...ctx,tabId:`cache-tab-${index}`});
assertPlanRefusal(cache.runtimePlan(cachedContexts[0],fresh));
console.log("✓ ACP cache eviction, same-tab epoch replacement and active-turn cancel authority");
console.log(`ACP lifecycle: ${names.length} fixture scenarios and binding regressions passed`);

const permissionFixture:Fixture=JSON.parse(fs.readFileSync(new URL("permission-allow.json",root),"utf8"));
const permissionContext={...permissionFixture.operations[2].context!,tabId:"permission-authority"};
const setup={...permissionContext,threadId:undefined,activeTurnId:undefined,activeTurns:[],pendingRequest:undefined};
const initialized=hooks.runtimePlan(setup,init);
hooks.runtimeResponse(setup,init,initialized.state,{kind:"success",value:{protocolVersion:1,agentCapabilities:{},authMethods:[]}});
const opening=hooks.runtimePlan(setup,open);
hooks.runtimeResponse(setup,open,opening.state,{kind:"success",value:{sessionId:"session-fixture"}});
for (const answer of [{kind:"choice",values:["foreign"]},{kind:"choice",values:["allow","reject"]},{kind:"text",text:"allow"}] as const) {
  assert.throws(()=>hooks.runtimePlan(permissionContext,{kind:"answer",requestId:"permission-fixture",answer}));
}
for (const changed of [{...permissionContext,epoch:2},{...permissionContext,bindingNonce:"stale"}]) {
  assertPlanRefusal(hooks.runtimePlan(changed,{kind:"answer",requestId:"permission-fixture",answer:{kind:"choice",values:["allow"]}}));
}
const request=permissionContext.pendingRequest!;
assert.deepEqual(plain(hooks.runtimeReduce(permissionContext,null,[{kind:"request",requestId:"foreign",method:request.method,params:{...(request.params as Record<string,JsonValue>),sessionId:"foreign"}}])).events,[]);
assert.equal(hooks.runtimeReduce(permissionContext,null,[{kind:"request",requestId:"valid",method:request.method,params:request.params}]).events[0]?.kind,"prompt");
console.log("✓ ACP permission options and stale session/epoch/nonce boundaries");
assert.throws(()=>hooks.runtimePlan({...permissionContext,threadId:"foreign"},{kind:"answer",requestId:"permission-fixture",answer:{kind:"cancel"}}));

for (const kind of ["agent_message_chunk","agent_thought_chunk","user_message_chunk","tool_call","plan","extension-update",...(["session/request_permission","fs/read_text_file","fs/write_text_file","terminal/create","terminal/output","terminal/wait_for_exit","terminal/kill","terminal/release"])]) {
  const instance=loadHooks();
  const initial={...ctx,tabId:`first-signal-${kind}`,threadId:undefined,activeTurnId:undefined,activeTurns:[]};
  const initializing=instance.runtimePlan(initial,init);
  instance.runtimeResponse(initial,init,initializing.state,{kind:"success",value:{protocolVersion:1,agentCapabilities:{},authMethods:[]}});
  const opening=instance.runtimePlan(initial,open);
  instance.runtimeResponse(initial,open,opening.state,{kind:"success",value:{sessionId:"own"}});
  const context={...initial,threadId:"own"};
  instance.runtimePlan(context,submit);
  const frame:RuntimeFrame=kind.includes("/") ? {kind:"request",requestId:"first",method:kind,params:{sessionId:"own"}} :
    {kind:"notification",method:"session/update",params:{sessionId:"own",update:{sessionUpdate:kind,content:{type:"text",text:"opaque"},toolCallId:"opaque-tool",entries:[]}}};
  const first=instance.runtimeReduce(context,null,[frame]);
  assert.deepEqual(plain(first.events.filter(event=>event.kind === "turnState")),[{kind:"turnState",threadId:"own",turnId:"own:turn:1",state:"working"}],kind);
  const repeated=instance.runtimeReduce(context,first.state,[frame]);
  assert.deepEqual(plain(repeated.events.filter(event=>event.kind === "turnState")),[],`${kind} starts once`);
  const after={...context,activeTurnId:"own:turn:1",activeTurns:[{threadId:"own",turnId:"own:turn:1"}]};
  assert.deepEqual(plain(instance.runtimeReduce(after,repeated.state,[frame]).events.filter(event=>event.kind === "turnState")),[],`${kind} stays started after host publication`);
}
console.log("✓ ACP every first own-session update/request starts exactly once");
