import assert from "node:assert/strict";
import { test } from "node:test";
import { backgroundCommandName, backgroundOwnsTool, backgroundWorkElapsed, composerBackgroundWorkBubble, composerBackgroundWorkItems, LONG_RUNNING_WORK_MS, withProviderBackgroundWork } from "./composerBackgroundWorkModel";
import type { NormalizedChatMessage } from "@codeterm/chat-core";

const now = Date.parse("2026-10-17T12:00:00.000Z");
test("provider background commands keep their task kind and own-session identity", () => {
  const messages: NormalizedChatMessage[] = [{
    id: "shell-result", type: "tool_call", content: "", sourceSessionId: "own-session",
    toolKind: "command", toolInput: { command: "node fixture.mjs" }, toolId: "call", toolCallSourceId: "source",
  }, {
    id: "shell-outcome", type: "tool_result", content: "", sourceSessionId: "own-session",
    toolId: "call", toolCallSourceId: "source", toolPairing: "exact", toolBackgroundTaskId: "shell",
  }];
  const items = withProviderBackgroundWork([], { shell: "", child: "Inspect" }, "own-session", {}, messages);
  assert.deepEqual(items.map(item => [item.kind, item.name]), [["shell", "fixture.mjs"], ["agent", "Inspect"]]);
  assert.equal(items[0].command, "node fixture.mjs");
  assert.equal(items[0].processId, undefined);
  assert.equal(withProviderBackgroundWork([], { shell: "" }, "foreign-session", {}, messages)[0].kind, "agent");
});
test("PTY background roster remains visible without runtime agent_activity rows and drains on completion", () => {
  const items = withProviderBackgroundWork([], { child: "Inspect source" }, "own-session");
  assert.equal(composerBackgroundWorkBubble(items)?.count, 1);
  assert.equal(items[0].status, "running");
  assert.equal(items[0].elapsed, "");
  assert.equal(items[0].transcriptThreadId, undefined);
  assert.equal(withProviderBackgroundWork(items, { child: "Inspect source" }, "own-session").length, 1);
  assert.deepEqual(withProviderBackgroundWork([], {}, "own-session"), []);
  assert.deepEqual(withProviderBackgroundWork([], { child: "Foreign" }), []);
});
const agent = (id: string, ageMs: number, toolInput: Record<string, unknown>, patch: Partial<NormalizedChatMessage> = {}): NormalizedChatMessage => ({
  id,
  type: "tool_call",
  content: "",
  timestamp: new Date(now - ageMs).toISOString(),
  sourceSessionId: "root",
  toolKind: "agent",
  toolName: "agent_activity",
  toolInput,
  ...patch,
});

const process = {threadId:"root",processId:"process-1",itemId:"terminal-item",command:"node build.mjs",cwd:"C:/work",osPid:42};

test("agent descriptions survive later activity and take priority over opaque paths", () => {
  const items = composerBackgroundWorkItems([
    agent("start", 30_000, { agent_call_id: "call", agent_state: "started", agent_description: "Inspect source" }),
    agent("running", 20_000, { agent_call_id: "call", agent_id: "a743f86e11053a44a", agent_path: "a743f86e11053a44a", agent_state: "running" }),
  ], [], now, { subagentsVisible: true, rootThreadId: "root" });
  assert.equal(items[0].name, "Inspect source");
});

test("maps running terminals and capable-provider subagents into one counted bubble", () => {
  const items = composerBackgroundWorkItems([
    agent("agent-item", LONG_RUNNING_WORK_MS + 1, {
      agent_id:"child-thread",agent_call_id:"call-1",agent_state:"running",agent_description:"Inspect the launch path",agent_model:"model-x",agent_reasoning_effort:"high",agent_binding_nonce:"nonce",agent_runtime_epoch:1,
    }),
    {id:"terminal-item",type:"tool_result",content:"build output",timestamp:new Date(now - LONG_RUNNING_WORK_MS - 2).toISOString()},
  ], [process], now, { subagentsVisible: true, rootThreadId: "root", authority:{rootThreadId:"root",bindingNonce:"nonce",epoch:1} });

  assert.deepEqual(items.map(({kind,threadId}) => [kind,threadId]), [["terminal","root"],["agent","child-thread"]]);
  assert.equal(items[0].outputTail,"build output");
  assert.equal(items[0].elapsed,"15s");
  assert.equal(items[1].prompt,"Inspect the launch path");
  assert.equal(items[1].model,"model-x");
  assert.equal(items[1].effort,"high");
  assert.equal(items[1].elapsed,"15s");
  assert.equal(items[1].status,"running");
  assert.deepEqual(composerBackgroundWorkBubble(items), { count: 2, items });
});

test("historical children without live evidence never raise a bubble", () => {
  const items = composerBackgroundWorkItems([
    agent("a", LONG_RUNNING_WORK_MS + 1, {agent_id:"a",agent_call_id:"call-a",agent_state:"running"}),
    agent("b", LONG_RUNNING_WORK_MS + 2, {agent_id:"b",agent_call_id:"call-b",agent_state:"started"}),
    agent("c", LONG_RUNNING_WORK_MS + 3, {agent_id:"c",agent_call_id:"call-c",agent_state:"running"}),
    agent("foreign", LONG_RUNNING_WORK_MS + 4, {agent_id:"foreign",agent_call_id:"call-foreign",agent_parent_thread_id:"other-root",agent_state:"running"}),
  ], [], now, { subagentsVisible: true, rootThreadId: "root" });
  const bubble = composerBackgroundWorkBubble(items);

  assert.deepEqual(items.map(({threadId, status}) => [threadId, status]), [["c", "unknown"], ["b", "unknown"], ["a", "unknown"]]);
  assert.equal(bubble, null);
});

test("filters recent, completed, and untimed work and gates subagents by provider capability", () => {
  const messages = [
    agent("recent", LONG_RUNNING_WORK_MS - 1, {agent_id:"recent",agent_call_id:"call-recent",agent_state:"running"}),
    agent("done-started", LONG_RUNNING_WORK_MS * 3, {agent_id:"done",agent_call_id:"call-done",agent_state:"running"}),
    agent("done", LONG_RUNNING_WORK_MS * 2, {agent_id:"done",agent_call_id:"call-done",agent_state:"completed"}),
    { ...agent("untimed", LONG_RUNNING_WORK_MS * 2, {agent_id:"untimed",agent_call_id:"call-untimed",agent_state:"running"}), timestamp: undefined },
    {id:"short-item",type:"tool_result",content:"short process",timestamp:new Date(now - LONG_RUNNING_WORK_MS + 1).toISOString()},
  ] as NormalizedChatMessage[];
  const terminals = [
    { ...process, processId:"short", itemId:"short-item" },
    { ...process, processId:"untimed", itemId:"missing-time" },
  ];

  assert.deepEqual(composerBackgroundWorkItems(messages, terminals, now, { subagentsVisible: true, rootThreadId: "root" }), []);
  assert.deepEqual(composerBackgroundWorkItems([agent("live", LONG_RUNNING_WORK_MS + 1, {agent_id:"live",agent_state:"running"})], [], now), []);
  assert.equal(composerBackgroundWorkBubble([]), null);
});

test("formats elapsed time without negative values", () => {
  assert.equal(backgroundWorkElapsed(-1), "0s");
  assert.equal(backgroundWorkElapsed(65_000), "1m");
  assert.equal(backgroundWorkElapsed(3_725_000), "1h 2m");
});

test("restored child activity stays unknown without elapsed time until current binding evidence arrives",()=>{
  const input={agent_id:"child",agent_state:"started",agent_binding_nonce:"old",agent_runtime_epoch:2};
  const options={subagentsVisible:true,rootThreadId:"root",authority:{rootThreadId:"root",bindingNonce:"new",epoch:3}};
  const rows=[agent("child",60_000,input)];
  assert.equal(composerBackgroundWorkItems(rows,[],now,options)[0].status,"unknown");
  assert.equal(composerBackgroundWorkItems(rows,[],now,options)[0].elapsed,"");
  const refreshed=[agent("child",60_000,{...input,agent_binding_nonce:"new",agent_runtime_epoch:3,agent_state:"running"})];
  assert.equal(composerBackgroundWorkItems(refreshed,[],now,options)[0].status,"running");
  assert.equal(composerBackgroundWorkItems(refreshed,[],now,options)[0].elapsed,"1m");
});

test("projects readable command names without long arguments",()=>{
  assert.equal(backgroundCommandName('pwsh -NoProfile -File "C:\\work area\\cleanup-merged.ps1" -Lane example'),"cleanup-merged.ps1");
  assert.equal(backgroundCommandName('bash -lc "node /work/build.mjs --flag value"'),"build.mjs");
  assert.equal(backgroundCommandName(`node ${"x".repeat(40)}.mjs`).length,24);
});

test("deduplicates process identity while preserving distinct jobs and exact tool ownership",()=>{
  const message:NormalizedChatMessage={id:process.itemId,type:"tool_call",content:"",timestamp:new Date(now-20_000).toISOString()};
  const items=composerBackgroundWorkItems([message],[process,process,{...process,processId:"another"}],now);
  assert.equal(items.length,2);
  assert.equal(items[0].status,"running");
  assert.equal(backgroundOwnsTool({id:process.itemId,sourceSessionId:"root"},items),true);
  assert.equal(backgroundOwnsTool({id:process.itemId,sourceSessionId:"foreign"},items),false);
  assert.equal(backgroundOwnsTool({id:"foreground"},items),false);
  assert.equal(backgroundOwnsTool(undefined,items),false);
});

test("only live work makes a bubble; unknown work rides along beside it and ended work is never shown", () => {
  const items = withProviderBackgroundWork([], { live: "Live task" }, "root", { dropped: "Dropped task" });
  assert.deepEqual(items.map(item => item.status), ["running", "unknown"]);
  assert.equal(composerBackgroundWorkBubble(items)?.count, 1);
  assert.deepEqual(composerBackgroundWorkBubble(items)?.items.map(item => item.name), ["Live task", "Dropped task"]);
  assert.equal(composerBackgroundWorkBubble(items.slice(1)), null, "without running work there is no bubble at all");
});

test("unlabelled provider tasks read as numbered internal agents and keep their raw id aside", () => {
  const items = withProviderBackgroundWork([], { bm76h90j0: "", a2: "Run the release gate", zz9: "  " }, "root");
  assert.deepEqual(items.map(item => [item.name, item.ordinal ?? null, item.rawId]), [
    ["Internal agent", 1, "bm76h90j0"],
    ["Run the release gate", null, "a2"],
    ["Internal agent", 2, "zz9"],
  ]);
});

test("own-session compaction invalidates runtime child liveness until fresh activity", () => {
  const authority = {rootThreadId:"root",bindingNonce:"nonce",epoch:1};
  const options = {subagentsVisible:true,rootThreadId:"root",authority};
  const child = agent("start", 60_000, {agent_id:"child",agent_state:"running",agent_binding_nonce:"nonce",agent_runtime_epoch:1});
  const boundary: NormalizedChatMessage = {id:"boundary",type:"system",content:"",sourceSessionId:"root",timelineMarker:{kind:"context_compaction"}};
  const invalidated = composerBackgroundWorkItems([child,boundary],[],now,options);
  assert.equal(invalidated[0].status,"unknown");
  assert.equal(invalidated[0].elapsed,"");
  assert.equal(composerBackgroundWorkBubble(invalidated),null);
  const fresh = composerBackgroundWorkItems([child,boundary,{...child,id:"fresh"}],[],now,options);
  assert.equal(fresh[0].status,"running");
  assert.equal(composerBackgroundWorkBubble(fresh)?.count,1);
  const foreign = composerBackgroundWorkItems([child,{...boundary,sourceSessionId:"foreign"}],[],now,options);
  assert.equal(foreign[0].status,"running");
});
