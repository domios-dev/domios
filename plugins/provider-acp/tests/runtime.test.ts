import assert from "node:assert/strict";
import fs from "node:fs";
import vm from "node:vm";
import {createTranscriptState,reduceTranscript,projectTranscript} from "../../../packages/chat-core/src/transcript";
import { runtimeJson } from "./runtimeJson";
import { foldRow } from "./runtimeRows";
import { assertRuntimeOperation, assertRuntimePlan, assertRuntimeReduction } from "./runtimeAuthority";
import type {AgentRuntime, AgentEvent, ChatMsg, JsonValue, Operation, RpcReply, RuntimeAction, RuntimeContext, RuntimeFrame, RuntimePlan, RuntimeReduction, RuntimeStep, StdioAgentRuntimeCapability} from "@codeterm/plugin-sdk";

interface Fixture {
  schemaVersion:1;
  context:RuntimeContext;
  operations:{operation:Operation; context?:RuntimeContext}[];
  rpc:({direction:"outbound"; operation:number; action:RuntimeAction}
    | {direction:"inbound"; operation:number; reply:RpcReply}
    | {direction:"inbound"; frame:RuntimeFrame; context?:RuntimeContext}
    | {direction:"inbound"; frames:RuntimeFrame[]; context?:RuntimeContext})[];
  expected:{results:{operation:number; step:RuntimeStep}[]; events:AgentEvent[]; planErrors?:{operation:number; code:string}[]};
}
function loadHooks():AgentRuntime {
  const sandbox = vm.createContext({module:{exports:{}}, exports:{}, host:new Proxy({}, {get:()=>{throw new Error("Runtime accessed host");}})});
  sandbox.exports = sandbox.module.exports;
  vm.runInContext(fs.readFileSync(new URL("../plugin.js", import.meta.url), "utf8"), sandbox);
  const metrics={characters:0,scanned:0,watch:false};
  sandbox.__serializationMetrics__=metrics;
  vm.runInContext(`{
    const stringify=JSON.stringify;
    const codePointAt=String.prototype.codePointAt;
    String.prototype.codePointAt=function(...args) {
      if (__serializationMetrics__.watch) __serializationMetrics__.scanned++;
      return Reflect.apply(codePointAt,this,args);
    };
    JSON.stringify=function(...args) {
      const result=Reflect.apply(stringify,JSON,args);
      __serializationMetrics__.characters+=typeof result === "string" ? result.length : 0;
      return result;
    };
  }`,sandbox);
  const hooks=sandbox.module.exports.default as AgentRuntime;
  return {...hooks,
    runtimePlan(ctx,op) { assertRuntimeOperation(ctx,op);const plan=runtimeJson(hooks.runtimePlan(ctx,op),"runtimePlan");assertRuntimePlan(ctx,op,plan);return plan; },
    runtimeResponse(ctx,op,state,reply) { assertRuntimeOperation(ctx,op);const step=runtimeJson(hooks.runtimeResponse(ctx,op,state,reply),"runtimeResponse");if (step.kind === "continue") assertRuntimePlan(ctx,op,step.plan);
      if (step.kind === "complete") assert.ok(!step.events?.some(event=>event.kind === "sendEvidence"),"correlated response cannot confirm delivery");
      return step; },
    runtimeReduce(ctx,state,frames) {
      const before={characters:metrics.characters,scanned:metrics.scanned};
      const track=ctx.tabId === "batch-text-1024" && frames.length === 1024;
      metrics.watch=track;
      let reduction:RuntimeReduction;
      try { reduction=runtimeJson(hooks.runtimeReduce(ctx,state,frames),"runtimeReduce"); }
      finally { metrics.watch=false; }
      assertRuntimeReduction(reduction);
      if (track) {
        const serialized=metrics.characters-before.characters, scanned=metrics.scanned-before.scanned;
        const allowed=4*JSON.stringify(reduction).length+65536;
        assert.ok(serialized+scanned<=allowed,`1024-frame drain serialized ${serialized} characters and scanned ${scanned} code points; four output passes allow ${allowed}`);
        console.log(`✓ ACP batch work: ${serialized} serialized + ${scanned} scanned; cap ${allowed}`);
      }
      return reduction;
    },
  };
}
for (const invalid of [undefined,()=>null,Symbol("opaque"),1n,NaN,Infinity,-Infinity])
  assert.throws(()=>runtimeJson({state:{nested:[invalid]},events:[]},"runtimeReduce"),/non-JSON/);
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
assert.equal(declaration.capabilities.agentRuntime.envelope,"jsonrpc2");
assert.deepEqual(descriptor.agentRuntime, declaration.capabilities.agentRuntime);
assert.deepEqual(declaration.capabilities.agentRuntime.launchProfiles["gemini-cli"].clientCapabilities,{fs:{readTextFile:false}});
// Gemini CLI prunes unresumable chats of its process directory on each start and loads
// system settings only from admin-owned paths, so its process runs from a host-owned directory.
assert.equal(declaration.capabilities.agentRuntime.launchProfiles["gemini-cli"].hostWorkingDirectory,true);
assert.equal(declaration.capabilities.agentRuntime.launchProfiles["gemini-cli"].settingsFiles,undefined);
for (const [id,profile] of Object.entries(declaration.capabilities.agentRuntime.launchProfiles)) {
  if (id !== "gemini-cli") assert.equal((profile as {clientCapabilities?:unknown}).clientCapabilities,undefined);
}
const root = new URL("./fixtures/runtime/",import.meta.url);
const names = fs.readdirSync(root).filter(name=>name.endsWith(".json")).sort();
assert.ok(descriptor.scenarios.length > 0);
assert.equal(new Set(descriptor.scenarios).size,descriptor.scenarios.length);
assert.deepEqual(names,[...descriptor.scenarios].sort());
function replayFixture(name:string, splitFrames=false):AgentEvent[] {
  const fixture:Fixture = JSON.parse(fs.readFileSync(new URL(name,root),"utf8"));
  assert.equal(fixture.schemaVersion,1);
  for (const entry of fixture.operations) assertRuntimeOperation(entry.context ?? fixture.context,entry.operation);
  const plans = new Map<number,RuntimePlan>();
  const results:{operation:number; step:RuntimeStep}[] = [];
  const events:AgentEvent[] = [];
  let state:JsonValue = null;
  let maxStateBytes=0, maxReductionBytes=0, maxReductionEvents=0;
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
      const frames="frames" in record ? record.frames : [record.frame];
      for (const batch of splitFrames ? frames.map(frame=>[frame]) : [frames]) {
        const reduction = plain(hooks.runtimeReduce(record.context ?? fixture.context,state,batch));
        maxReductionBytes=Math.max(maxReductionBytes,Buffer.byteLength(JSON.stringify(reduction)));
        maxReductionEvents=Math.max(maxReductionEvents,reduction.events.length);
        state = reduction.state;
        maxStateBytes=Math.max(maxStateBytes,Buffer.byteLength(JSON.stringify(state)));
        assert.ok(maxStateBytes<65536,`${name}: reducer state below 64 KiB`);
        events.push(...reduction.events);
      }
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
  if (name === "batch-byte-limit.json" || name === "batch-event-limit.json") {
    const rows=new Map<string,ChatMsg>();
    for (const event of events) foldRow(rows,event);
    assert.equal(rows.size,1);
    const content=[...rows.values()][0].content;
    assert.ok(content.endsWith("healthy"),"post-limit continuation has the visible row offset");
    if (name === "batch-byte-limit.json") {
      assert.ok(content.includes("[ACP row truncated: runtime reduction byte limit]"));
      assert.ok(maxReductionBytes>4*1024*1024-4096,"fixture reaches the serialization boundary");
      assert.ok(maxReductionBytes<=4*1024*1024-1024,"fixed margin remains for native serialization");
    } else {
      assert.equal(maxReductionEvents,1024);
      assert.equal(content,"z".repeat(1022)+"healthy");
      assert.ok(events.some(event=>event.kind === "notice" && event.message.includes("runtime reduction event limit")));
    }
  }
  if (name.startsWith("long-stream-") || (name.startsWith("oversize-") || name.startsWith("large-"))) {
    const rows=new Map<string,ChatMsg>();
    for (const event of events) foldRow(rows,event);
    assert.equal(rows.size,1);
    const final=[...rows.values()][0].content;
    if (name.startsWith("long-stream-")) assert.equal(final,("x".repeat(4092)+"🌍").repeat(128));
    else {
      const delivered=fixture.rpc.flatMap(record=>"frame" in record && record.frame.kind === "notification" && record.frame.method === "session/update"
        ? [((record.frame.params as {update:{content:{text:string}}}).update.content.text)] : []).join("");
      assert.equal(final,delivered,"every delivered text character reaches the host row");
      assert.ok(!final.includes("[ACP row truncated:"));
      assert.equal(events.filter(event=>event.kind === "chatItem" || event.kind === "chatItemAppend").length,fixture.rpc.filter(record=>"frame" in record).length,"one text event per update");
      for (const event of events) if (event.kind === "chatItem" || event.kind === "chatItemAppend") {
        const content=event.kind === "chatItem" ? event.item.content : event.content;
        assert.equal(content.isWellFormed(),true,"each event ends on a code-point boundary");
      }
    }
    assert.ok(events.some(event=>event.kind === "turnState" && event.state === "idle"
      && (name !== "long-stream-cancel.json" || event.detail === "interrupted")));
    console.log(`✓ ACP ${name}: final row ${Buffer.byteLength(final)} bytes; max state ${maxStateBytes} bytes`);
  }
  if (name === "tool-output-retention.json") {
    const rows=new Map<string,ChatMsg>();
    for (const event of events) foldRow(rows,event);
    assert.equal(rows.size,3);
    assert.deepEqual([...rows.values()].map(row=>[row.seq,row.toolTitle,row.toolStatus,row.toolResult?.length]),
      [[1,"Title 0","completed",15900],[2,"Title 1","interrupted",15900],[3,"Title 2","interrupted",15900]]);
    assert.deepEqual(rows.get("large-0")?.toolInput,{command:"opaque"});
  }
  if (name === "two-turns-restart-third.json") {
    const rows=new Map<string,ChatMsg>();
    for (const event of events) foldRow(rows,event);
    assert.deepEqual([...rows.values()].map(row=>row.content),["first","one","second","two","third","three"]);
    const authority={sourceSessionId:"session-fixture",sessionBindingNonce:"restored-launch"};
    const restored=reduceTranscript(createTranscriptState(),{kind:"delta",authority,range:null,
      rows:[...rows.values()].map((row,seq)=>({id:row.id,type:row.type === "user" ? "user" as const : "assistant" as const,content:row.content,seq,sourceSessionId:"session-fixture"})),
    }).state;
    const sends=events.filter(event=>event.kind === "sendEvidence").map((event,index)=>({
      id:event.sendId,content:["first","second","third"][index],sentAtMs:index+1,
      matchedRow:{ordinal:1,source:{source_session_id:"session-fixture",record:{kind:"structured" as const,thread_id:"session-fixture",item_id:event.itemId!}}},
    }));
    const projected=projectTranscript(restored,sends);
    assert.equal(projected.pending.length,0,"confirmed local sends stay reconciled after restore");
    assert.equal(projected.rows.length,6,"one row per user send across restart");
    assert.equal(projected.echoes.size,3);
  }
  console.log(`✓ ACP ${name}`);
  return events;
}
const fixtureFailures:string[]=[];
for (const name of names) {
  try {
    const batch=replayFixture(name);
    if (name === "batch-row-determinism.json") assert.deepEqual(batch,replayFixture(name,true),"batching preserves exact events, implicit row ids and seq");
  }
  catch (error) {
    const failure=`${name}: ${String(error)}`;
    fixtureFailures.push(failure);
    console.error(`✗ ACP ${failure}`);
  }
}
assert.deepEqual(fixtureFailures,[],"every shared fixture must obey runtime JSON and authority rules");
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
assert.deepEqual(plain(hooks.runtimePlan(bound,submit).action),{kind:"request",call:{method:"session/prompt",params:{sessionId:"own",prompt:[{type:"text",text:"Startup instructions"},{type:"text",text:"hello"}]},target:{threadId:"own",turnId:"own:turn:1"}}});
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
function permissionPrompt(toolCall:JsonValue) {
  const params={...(request.params as Record<string,JsonValue>),toolCall};
  const event=hooks.runtimeReduce(permissionContext,null,[{kind:"request",requestId:"readable",method:request.method,params}]).events.find(event=>event.kind === "prompt");
  assert.ok(event && event.kind === "prompt");
  return {params,prompt:event.prompt};
}
const nativePath="C:\\Users\\Owner\\allow-test.txt";
const diffPermission=permissionPrompt({toolCallId:"diff",title:"Create empty file allow-test.txt",kind:"edit",locations:[{path:nativePath}],
  content:[{type:"diff",path:nativePath,oldText:"",newText:"ALLOWED\r\nsecond\r\n"}]});
assert.equal(diffPermission.prompt.question.split("\n")[0],`Writes ${nativePath} (+2/-0 lines)`);
const hiddenTarget="C:\\work\\actual.txt";
const hiddenPermission=permissionPrompt({toolCallId:"hidden-diff",kind:"edit",title:"Benign file",
  rawInput:{path:"C:\\work\\benign.txt"},content:[
    {type:"content",content:{type:"text",text:"untrusted prose ".repeat(2000)}},
    {type:"content",content:{type:"text",text:"more prose ".repeat(2000)}},
    {type:"content",content:{type:"text",text:"extra prose ".repeat(2000)}},
    {type:"diff",path:hiddenTarget,oldText:"before\n",newText:"after\n"},
  ]});
assert.equal(hiddenPermission.prompt.question.split("\n")[0],`Writes ${hiddenTarget} (+1/-1 lines)`);
assert.ok(hiddenPermission.prompt.question.length <= 2048);
const longTarget="C:\\work\\"+"x".repeat(2100)+".txt";
const longHeader=permissionPrompt({toolCallId:"long-target",kind:"edit",title:"untrusted",content:[{type:"diff",path:longTarget,oldText:"",newText:"x"}]}).prompt.question;
assert.equal(longHeader,`Writes ${longTarget} (+1/-0 lines)\n`);
assert.match(diffPermission.prompt.question,/Create empty file allow-test.txt\nKind: edit/);
assert.ok(diffPermission.prompt.question.includes(`Path: ${nativePath}`));
assert.ok(diffPermission.prompt.question.includes(`Diff: ${nativePath} (+2 added, -0 removed lines)\n+ ALLOWED\n+ second`));
assert.deepEqual(plain(diffPermission.prompt.options),plain((hooks.runtimeReduce(permissionContext,null,[{kind:"request",requestId:"original",method:request.method,params:request.params}]).events[0] as Extract<AgentEvent,{kind:"prompt"}>).prompt.options));
const diffAnswer=hooks.runtimePlan({...permissionContext,pendingRequest:{...request,params:diffPermission.params}},
  {kind:"answer",requestId:request.requestId,answer:{kind:"choice",values:["allow"]}});
assert.deepEqual(plain(diffAnswer.action),{kind:"reply",requestId:request.requestId,result:{kind:"success",value:{outcome:{outcome:"selected",optionId:"allow"}}}});
const executePermission=permissionPrompt({toolCallId:"execute",title:"Run a command",kind:"execute",rawInput:{command:"type nul > x.txt"}});
assert.match(executePermission.prompt.question,/Kind: execute\nCommand: type nul > x.txt/);
const deletePermission=permissionPrompt({toolCallId:"delete",title:"Delete file",kind:"delete",locations:[{path:nativePath}]});
assert.ok(deletePermission.prompt.question.includes(`Kind: delete\nPath: ${nativePath}`));
const replacementPermission=permissionPrompt({toolCallId:"replace",title:"Replace file",kind:"edit",
  content:[{type:"diff",path:nativePath,oldText:"before\n",newText:"after\n"}]});
assert.match(replacementPermission.prompt.question,/\(\+1 added, -1 removed lines\)\n- before\n\+ after/);
const textPermission=permissionPrompt({toolCallId:"fetch",title:"Fetch resource",kind:"fetch",content:[{type:"content",content:{type:"text",text:"Resource preview"}}]});
assert.match(textPermission.prompt.question,/Resource preview/);
const largePermission=permissionPrompt({toolCallId:"large",title:"Large edit",kind:"edit",content:Array.from({length:8},()=>({type:"diff",path:nativePath+"x".repeat(450),
  oldText:"old\n".repeat(20000),newText:"😀".repeat(20000)}))});
assert.ok(largePermission.prompt.question.length <= 2048);
assert.ok(largePermission.prompt.question.endsWith("…"));
assert.ok(!/[\uD800-\uDBFF](?![\uDC00-\uDFFF])|(?<![\uD800-\uDBFF])[\uDC00-\uDFFF]/u.test(largePermission.prompt.question));
assert.equal(permissionPrompt({toolCallId:"minimal"}).prompt.question,"Tool permission requested\nAllow this tool call?");
assert.equal(permissionPrompt({toolCallId:"other",title:"Opaque tool",kind:"other"}).prompt.question,"Opaque tool\nAllow this tool call?");
function declaredPermission(toolCall:JsonValue, options?:JsonValue) {
  const params={...(request.params as Record<string,JsonValue>),toolCall,...(options ? {options} : {})};
  const event=hooks.runtimeReduce(permissionContext,null,[{kind:"request",requestId:"declared",method:request.method,params}]).events.find(event=>event.kind === "prompt");
  assert.ok(event && event.kind === "prompt");
  return event;
}
const declared=declaredPermission({toolCallId:"declared",kind:"edit"});
assert.deepEqual(declared.prompt.options.map(option=>option.label),(request.params as {options:{name:string}[]}).options.map(option=>option.name));
assert.deepEqual(plain(declared.permission),{action:"write",options:(request.params as {options:{optionId:string;kind:string}[]}).options.map(option=>({value:option.optionId,decision:option.kind}))});
for (const [kind,action] of [["read","read"],["search","read"],["edit","write"],["delete","write"],["move","write"],["execute","command"],["fetch","other"],["think","other"],["other","other"],[undefined,"other"]] as const)
  assert.equal(declaredPermission({toolCallId:`kind-${kind}`,...(kind ? {kind} : {})}).permission?.action,action,String(kind));
const unnamed=declaredPermission({toolCallId:"unnamed"},[{optionId:"opaque-a",name:"",kind:"allow_always"},{optionId:"opaque-b",name:"",kind:"reject_once"}]);
assert.deepEqual(unnamed.prompt.options.map(option=>option.label),["Always allow","Reject"]);
console.log("✓ ACP readable permission summaries retain native paths and bounded previews; labels hide protocol ids; declared kinds reach the host");
console.log("✓ ACP permission options and stale session/epoch/nonce boundaries");
const advertisedModes={currentModeId:"opaque-default",availableModes:[{id:"opaque-default",name:"Default"},{id:"opaque-approve-all",name:"Approve all"}]};
function openWithModes(tabId:string, providerSettings:JsonValue, modes:JsonValue=advertisedModes) {
  const context={...ctx,tabId,providerSettings};
  const plan=hooks.runtimePlan(context,init);
  hooks.runtimeResponse(context,init,plan.state,{kind:"success",value:{protocolVersion:1,agentCapabilities:{},authMethods:[]}});
  const opened=hooks.runtimePlan(context,fresh);
  return {context,step:hooks.runtimeResponse(context,fresh,opened.state,{kind:"success",value:{sessionId:`${tabId}-session`,modes}})};
}
function loadedMode(step:RuntimeStep):string|undefined {
  assert.ok(step.kind === "complete" && step.result.kind === "threadLoaded",JSON.stringify(step));
  return step.kind === "complete" && step.result.kind === "threadLoaded" ? step.result.settings.mode : undefined;
}
const fullAccess=openWithModes("full-access-mode",{fullAccessMode:"opaque-approve-all"});
assert.ok(fullAccess.step.kind === "continue" && fullAccess.step.plan.action.kind === "request");
if (fullAccess.step.kind !== "continue" || fullAccess.step.plan.action.kind !== "request") throw new Error("full access must request its declared mode");
assert.deepEqual(plain(fullAccess.step.plan.action.call.method),"session/set_mode");
assert.deepEqual(plain(fullAccess.step.plan.action.call.params),{sessionId:"full-access-mode-session",modeId:"opaque-approve-all"});
assert.equal(loadedMode(hooks.runtimeResponse(fullAccess.context,fresh,fullAccess.step.plan.state,{kind:"success",value:{}})),"opaque-approve-all");
const refusedMode=openWithModes("full-access-refused",{fullAccessMode:"opaque-approve-all"});
if (refusedMode.step.kind !== "continue") throw new Error("full access must request its declared mode");
assert.equal(loadedMode(hooks.runtimeResponse(refusedMode.context,fresh,refusedMode.step.plan.state,{kind:"error",error:{code:-32602,message:"opaque refusal"}})),"opaque-default","a refused mode keeps the session loaded");
assert.equal(loadedMode(openWithModes("interactive-mode",{}).step),"opaque-default","an interactive launch keeps the agent's mode");
assert.equal(loadedMode(openWithModes("undeclared-mode",{fullAccessMode:"opaque-unadvertised"}).step),"opaque-default","an unadvertised mode is never requested");
assert.equal(loadedMode(openWithModes("already-full",{fullAccessMode:"opaque-approve-all"},{...advertisedModes,currentModeId:"opaque-approve-all"}).step),"opaque-approve-all");
console.log("✓ ACP full-access launches enter the profile's declared approve-everything mode only when the agent advertises it");
const chosenMode=openWithModes("chosen-mode",{sessionMode:"opaque-approve-all"});
if (chosenMode.step.kind !== "continue" || chosenMode.step.plan.action.kind !== "request") throw new Error("a chosen mode must be re-entered");
assert.deepEqual(plain(chosenMode.step.plan.action.call.params),{sessionId:"chosen-mode-session",modeId:"opaque-approve-all"},"an interactive launch re-enters the owner's chosen mode");
const chosenOverFull=openWithModes("chosen-over-full",{sessionMode:"opaque-plan",fullAccessMode:"opaque-approve-all"},{currentModeId:"opaque-approve-all",availableModes:[{id:"opaque-plan",name:"Plan"},{id:"opaque-approve-all",name:"Approve all"}]});
if (chosenOverFull.step.kind !== "continue" || chosenOverFull.step.plan.action.kind !== "request") throw new Error("a chosen mode must win");
assert.deepEqual(plain(chosenOverFull.step.plan.action.call.params),{sessionId:"chosen-over-full-session",modeId:"opaque-plan"},"the owner's chosen mode wins over the profile's full-access mode");
assert.equal(loadedMode(openWithModes("chosen-unadvertised",{sessionMode:"opaque-gone"}).step),"opaque-default","a chosen mode the agent no longer offers is never requested");
assert.equal(loadedMode(openWithModes("chosen-already",{sessionMode:"opaque-default"}).step),"opaque-default","a chosen mode already in effect sends nothing");
console.log("✓ ACP re-enters the owner's chosen session mode on load, ahead of the profile default");
const selectableModels={currentModelId:"opaque-m1",availableModels:[{modelId:"opaque-m1",name:"First"},{modelId:"opaque-m2",name:"Second"}]};
function selectedThenFullAccess(tabId:string, resumed:boolean):string[] {
  const context={...ctx,tabId,threadId:resumed ? `${tabId}-session` : undefined,providerSettings:{fullAccessMode:"opaque-approve-all"}};
  const plan=hooks.runtimePlan(context,init);
  hooks.runtimeResponse(context,init,plan.state,{kind:"success",value:{protocolVersion:1,agentCapabilities:{loadSession:true},authMethods:[]}});
  const op:Operation=resumed ? {kind:"resumeThread",threadId:`${tabId}-session`,instructions:"",settings:{model:"opaque-m2"}} : {kind:"openThread",instructions:"",settings:{model:"opaque-m2"}};
  const opening=hooks.runtimePlan(context,op);
  const methods=[opening.action.kind === "request" ? opening.action.call.method : opening.action.kind];
  let step=hooks.runtimeResponse(context,op,opening.state,{kind:"success",value:{...(resumed ? {} : {sessionId:`${tabId}-session`}),models:selectableModels,modes:advertisedModes}});
  while (step.kind === "continue" && step.plan.action.kind === "request") {
    methods.push(step.plan.action.call.method);
    step=hooks.runtimeResponse(context,op,step.plan.state,{kind:"success",value:{}});
  }
  assert.equal(loadedMode(step),"opaque-approve-all",tabId);
  return methods;
}
assert.deepEqual(selectedThenFullAccess("full-access-new-selected",false),["session/new","session/set_model","session/set_mode"]);
assert.deepEqual(selectedThenFullAccess("full-access-resumed-selected",true),["session/load","session/set_model","session/set_mode"]);
console.log("✓ ACP new and resumed full-access sessions send set_mode once after the launch model selection");
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

const controls = loadHooks();
const controlContext = {...ctx,tabId:"controls-regression"};
const controlsInitialized = controls.runtimePlan(controlContext,init);
controls.runtimeResponse(controlContext,init,controlsInitialized.state,{kind:"success",value:{protocolVersion:1,agentCapabilities:{},authMethods:[]}});
const openControl = controls.runtimePlan(controlContext,fresh);
controls.runtimeResponse(controlContext,fresh,openControl.state,{kind:"success",value:{sessionId:"own"}});
const controlBound = {...controlContext,threadId:"own"};
const commandFrame:RuntimeFrame = {kind:"notification",method:"session/update",params:{sessionId:"own",update:{sessionUpdate:"available_commands_update",availableCommands:[{name:"inspect",description:"Inspect"}]}}};
controls.runtimeReduce(controlBound,null,[commandFrame]);
const command:Operation = {kind:"submitTurn",threadId:"own",sendId:"command",input:{text:"/inspect target",attachments:[]},settings:{}};
assert.equal(controls.runtimePlan(controlBound,command).action.kind,"request");
controls.runtimeReduce(controlBound,null,[{...commandFrame,params:{sessionId:"foreign",update:{sessionUpdate:"available_commands_update",availableCommands:[{name:"foreign",description:"Foreign"}]}}}]);
assertPlanRefusal(controls.runtimePlan(controlBound,{...command,input:{text:"/foreign",attachments:[]}}));
controls.runtimeReduce(controlBound,null,[{...commandFrame,params:{sessionId:"own",update:{sessionUpdate:"available_commands_update",availableCommands:[]}}}]);
assertPlanRefusal(controls.runtimePlan(controlBound,command));
assertPlanRefusal(controls.runtimePlan({...controlBound,epoch:2},command));
const required = controls.runtimeResponse(controlBound,command,{method:"session/prompt"},{kind:"error",error:{code:-32000,message:"Authentication required"}});
assert.deepEqual(plain(required),{kind:"refused",reason:{code:"authentication_required",message:"Authentication required"},beforeAcceptance:true});
console.log("✓ ACP command revocation, foreign declaration, epoch and explicit auth-required evidence");

const reattach = loadHooks();
const attachContext = {...ctx, tabId:"same-epoch-reattach"};
const attachInit = reattach.runtimePlan(attachContext, init);
reattach.runtimeResponse(attachContext, init, attachInit.state, {kind:"success",value:{protocolVersion:1,agentCapabilities:{sessionCapabilities:{resume:{}}},authMethods:[]}});
const attachOpen = reattach.runtimePlan(attachContext, fresh);
reattach.runtimeResponse(attachContext, fresh, attachOpen.state, {kind:"success",value:{sessionId:"own",modes:{currentModeId:"safe",availableModes:[{id:"safe",name:"Safe"}]}}});
const attachBound = {...attachContext,threadId:"own"};
reattach.runtimePlan(attachBound,submit);
const chunk:RuntimeFrame={kind:"notification",method:"session/update",params:{sessionId:"own",update:{sessionUpdate:"agent_message_chunk",content:{type:"text",text:"first"}}}};
const beforeAttach=reattach.runtimeReduce(attachBound,null,[chunk]);
const resume:Operation={kind:"resumeThread",threadId:"own",instructions:"replacement",settings:{}};
const local=reattach.runtimePlan(attachBound,resume);
assert.deepEqual(plain(local.action),{kind:"local"});
const loaded=reattach.runtimeResponse(attachBound,resume,local.state,{kind:"success",value:null});
assert.deepEqual(plain(loaded),{kind:"complete",result:{kind:"threadLoaded",threadId:"own",settings:{commands:[],configOptions:[],modes:[{id:"safe",name:"Safe"}],mode:"safe"}}});
const afterAttach=reattach.runtimeReduce(attachBound,beforeAttach.state,[chunk]);
const beforeItems=beforeAttach.events.filter(event=>event.kind === "chatItem");
const afterItems=afterAttach.events.filter(event=>event.kind === "chatItem" || event.kind === "chatItemAppend");
assert.ok(beforeItems.length && afterItems.length);
assert.equal(afterItems[0].kind === "chatItemAppend" && afterItems[0].itemId,beforeItems[0].item.id);
assert.equal(afterItems[0].kind === "chatItemAppend" && afterItems[0].content,"first");
assert.equal(afterItems[0].kind,"chatItemAppend");
assert.equal(afterItems[0].kind === "chatItemAppend" && afterItems[0].offset,5);
assert.throws(()=>reattach.runtimePlan(attachBound,{...resume,threadId:"foreign"}));
assertPlanRefusal(reattach.runtimePlan({...attachBound,threadId:"foreign"},{...resume,threadId:"foreign"}));
assertPlanRefusal(reattach.runtimePlan({...attachBound,epoch:attachBound.epoch+1},resume));
assertPlanRefusal(reattach.runtimePlan({...attachBound,bindingNonce:"stale-nonce"},resume));
const nextEpoch={...attachBound,epoch:attachBound.epoch+1};
const nextInit=reattach.runtimePlan(nextEpoch,init);
reattach.runtimeResponse(nextEpoch,init,nextInit.state,{kind:"success",value:{protocolVersion:1,agentCapabilities:{sessionCapabilities:{resume:{}}},authMethods:[]}});
assert.deepEqual(plain(reattach.runtimePlan(nextEpoch,resume).action),{kind:"request",call:{method:"session/resume",params:{sessionId:"own",cwd:nextEpoch.cwd,mcpServers:[]}}});
console.log("✓ ACP same-epoch local reattach preserves row identity; foreign session refused; new epoch resumes");


for (const [name, reason] of [
  ["model-option-undeclared-refused.json", "This agent declared no model option"],
  ["model-option-declared-selected.json", "Model was not declared by this agent"],
] as const) {
  const fixture:Fixture=JSON.parse(fs.readFileSync(new URL(name,root),"utf8"));
  const index=fixture.expected.planErrors![0].operation;
  const entry=fixture.operations[index];
  assert.deepEqual(plain(hooks.runtimePlan(entry.context ?? fixture.context,entry.operation)),{action:{kind:"refused",reason},state:null});
  assert.equal(fixture.rpc.some(frame=>frame.direction === "outbound" && frame.operation === index),false);
}
console.log("✓ ACP honest model refusal reasons and zero outbound frames");

for (const [name, reason] of [
  ["prompt-images-refused", "did not declare image support"],
  ["prompt-images-undeclared", "did not declare image support"],
  ["prompt-image-bytes-missing", "bytes are unavailable"],
  ["prompt-resource-refused", "attachment kind"],
] as const) {
  const fixture:Fixture=JSON.parse(fs.readFileSync(new URL(`${name}.json`,root),"utf8"));
  const entry=fixture.operations[2];
  const plan=hooks.runtimePlan(entry.context ?? fixture.context,entry.operation);
  assertPlanRefusal(plan);
  assert.ok(plan.action.kind === "refused" && plan.action.reason.includes(reason));
  assert.equal(fixture.rpc.some(frame=>frame.direction === "outbound" && frame.operation === 2),false);
}
console.log("✓ ACP unsupported attachments refuse with actionable reasons before any prompt");

for (const offered of [false,true]) {
  const instance=loadHooks();
  const context={...fixture.context,tabId:`no-replay-${offered}`,...(offered ? {hostMcp:{name:"domios",command:"/trusted/domios",args:[],env:{CODETERM_API_TOKEN:"<credential-placeholder>"}}} : {})};
  const initialized=instance.runtimePlan(context,init);
  instance.runtimeResponse(context,init,initialized.state,{kind:"success",value:{protocolVersion:1,agentCapabilities:{},authMethods:[]}});
  const opening=instance.runtimePlan(context,open);
  const error:RpcReply={kind:"error",error:{code:-32603,message:"Agent failure"}};
  if (!offered) assert.equal(instance.runtimeResponse(context,open,opening.state,error).kind,"refused");
  instance.runtimeResponse(context,open,opening.state,{kind:"success",value:{sessionId:"own"}});
  const bound={...context,threadId:"own"};
  const input:Operation={kind:"submitTurn",threadId:"own",sendId:"never-replay",input:{text:"opaque input",attachments:[]},settings:{}};
  const prompting=instance.runtimePlan(bound,input);
  assert.equal(instance.runtimeResponse(bound,input,prompting.state,error).kind,"refused");
  assert.equal(JSON.stringify(prompting.state).includes("<credential-placeholder>"),false);
}

const {reduceStream,settleStream}=await import("../src/runtimeReduce");
const streamContext={...bound,tabId:"long-stream",claimedSends:[]};
const streamFrame=(value:string):RuntimeFrame=>({kind:"notification",method:"session/update",params:{sessionId:"own",update:{sessionUpdate:"agent_message_chunk",messageId:"long",content:{type:"text",text:value}}}});
let longState:JsonValue=null;
for (let index=0;index<128;index++) {
  const reduced=reduceStream(streamContext,longState,[streamFrame("x".repeat(4096))],{replay:0,prompt:1,loading:false});
  longState=reduced.state;
  assert.ok(JSON.stringify(longState).length<65536,"long stream state stays below 64 KiB");
}
assert.ok(settleStream(streamContext,longState,"cancelled").events.some(event=>event.kind==="turnState" && event.detail==="interrupted"));
console.log("✓ ACP bounded 512 KiB long stream and cancellation");

const projectionRows=new Map<string,ChatMsg>();
let projected:JsonValue=null;
for (const replay of [0,1,2]) {
  for (const value of ["one🌍","two","three"]) {
    const reduction=reduceStream(streamContext,projected,[streamFrame(value)],{replay,prompt:1,loading:replay>0});
    projected=reduction.state;
    for (const event of reduction.events) foldRow(projectionRows,event);
  }
  assert.equal(projectionRows.size,1);
  assert.equal([...projectionRows.values()][0].content,"one🌍twothree");
}
let interleaved:JsonValue=null;
const interleavedRows=new Map<string,ChatMsg>();
for (const frame of [streamFrame("first"),{...streamFrame("thought"),params:{sessionId:"own",update:{sessionUpdate:"agent_thought_chunk",messageId:"thought",content:{type:"text",text:"thought"}}}} as RuntimeFrame,streamFrame("last")]) {
  const reduction=reduceStream(streamContext,interleaved,[frame],{replay:0,prompt:1,loading:false});
  interleaved=reduction.state;
  for (const event of reduction.events) foldRow(interleavedRows,event);
}
assert.equal(interleavedRows.get("own:turn:1:message:agent_message_chunk:long")?.content,"firstlast");
assert.equal(interleavedRows.size,2);
console.log("✓ ACP UTF-8 offsets, explicit-id continuation, and remote load replay preserve row content and identity");

let many:JsonValue=null;
const manyRows=new Map<string,ChatMsg>();
for (let index=0;index<512;index++) {
  const original=streamFrame("part");
  const frame:RuntimeFrame={...original,params:{sessionId:"own",update:{sessionUpdate:"agent_message_chunk",messageId:index === 0 ? "long" : `message-${index}-${"a".repeat(100)}`,content:{type:"text",text:"part"}}}};
  const reduced=reduceStream(streamContext,many,[frame],{replay:0,prompt:1,loading:false});
  many=reduced.state;
  assert.ok(Buffer.byteLength(JSON.stringify(many))<65536);
  for (const event of reduced.events) foldRow(manyRows,event);
}
const continued=reduceStream(streamContext,many,[streamFrame("last"),streamFrame("!")],{replay:0,prompt:1,loading:false});
for (const event of continued.events) foldRow(manyRows,event);
assert.equal(manyRows.size,513);
assert.ok([...manyRows.values()].every(row=>!row.content.includes("[ACP row truncated")));
assert.equal(manyRows.get("own:turn:1:message:agent_message_chunk:long")?.content,"part");
assert.deepEqual([...manyRows.values()].filter(row=>row.content==="last!").length,1);
assert.ok(Buffer.byteLength(JSON.stringify(continued.state))<65536);
console.log("✓ ACP bounded identity metadata keeps every new message and continues an evicted one without overwriting it");

const userFrame=(id:string):RuntimeFrame=>({kind:"notification",method:"session/update",params:{sessionId:"own",update:{sessionUpdate:"user_message_chunk",messageId:id,content:{type:"text",text:"ask"}}}});
const agentFrame=(id:string):RuntimeFrame=>({kind:"notification",method:"session/update",params:{sessionId:"own",update:{sessionUpdate:"agent_message_chunk",messageId:id,content:{type:"text",text:"answer"}}}});
let replayed:JsonValue=null;
const replayedRows=new Map<string,ChatMsg>();
for (let turn=0;turn<600;turn++) {
  const reduced=reduceStream(streamContext,replayed,[userFrame(`user-${turn}-${"u".repeat(64)}`),agentFrame(`agent-${turn}-${"a".repeat(64)}`)],{replay:1,prompt:0,loading:true});
  replayed=reduced.state;
  for (const event of reduced.events) foldRow(replayedRows,event);
}
assert.equal(replayedRows.size,1200);
assert.ok([...replayedRows.values()].every(row=>row.content === "ask" || row.content === "answer"));
assert.ok(Buffer.byteLength(JSON.stringify(replayed))<65536);
console.log("✓ ACP long session replay keeps real rows for every turn within the identity budget");
