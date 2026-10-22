import assert from "node:assert/strict";
import { test } from "node:test";
import { backgroundCommandName, backgroundOwnsTool, backgroundWorkElapsed, composerBackgroundWorkBubble, composerBackgroundWorkItems, LONG_RUNNING_WORK_MS, withProviderBackgroundWork } from "./composerBackgroundWorkModel";
import type { NormalizedChatMessage } from "@codeterm/chat-core";

const now = Date.parse("2026-10-17T12:00:00.000Z");
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

test("groups every live item and reports the running count", () => {
  const items = composerBackgroundWorkItems([
    agent("a", LONG_RUNNING_WORK_MS + 1, {agent_id:"a",agent_call_id:"call-a",agent_state:"running"}),
    agent("b", LONG_RUNNING_WORK_MS + 2, {agent_id:"b",agent_call_id:"call-b",agent_state:"started"}),
    agent("c", LONG_RUNNING_WORK_MS + 3, {agent_id:"c",agent_call_id:"call-c",agent_state:"running"}),
    agent("foreign", LONG_RUNNING_WORK_MS + 4, {agent_id:"foreign",agent_call_id:"call-foreign",agent_parent_thread_id:"other-root",agent_state:"running"}),
  ], [], now, { subagentsVisible: true, rootThreadId: "root" });
  const bubble = composerBackgroundWorkBubble(items);

  assert.equal(bubble?.count, 3);
  assert.deepEqual(bubble?.items.map(({threadId}) => threadId), ["c", "b", "a"]);
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
