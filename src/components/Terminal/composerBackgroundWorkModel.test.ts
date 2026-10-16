import assert from "node:assert/strict";
import { test } from "node:test";
import { composerBackgroundWorkItems, composerWorkBadgeLayout, LONG_RUNNING_WORK_MS } from "./composerBackgroundWorkModel";
import type { NormalizedChatMessage } from "@codeterm/chat-core";

const now = Date.parse("2026-10-17T12:00:00.000Z");
const message = (id: string, ageMs: number, toolInput: Record<string, unknown>, patch: Partial<NormalizedChatMessage> = {}): NormalizedChatMessage => ({
  id,
  type: "tool_call",
  content: "",
  timestamp: new Date(now - ageMs).toISOString(),
  toolKind: "agent",
  toolName: "agent_activity",
  toolInput,
  ...patch,
});

const process = {threadId:"root",processId:"process-1",itemId:"terminal-item",command:"node build.mjs",cwd:"C:/work",osPid:42};

test("maps running terminals and subagents to inspectable work items", () => {
  const items = composerBackgroundWorkItems([
    message("agent-item", LONG_RUNNING_WORK_MS + 1, {
      agent_id:"child-thread",agent_parent_thread_id:"root",agent_ordinal:1,agent_state:"running",agent_description:"Inspect the launch path",agent_model:"model-x",agent_reasoning_effort:"high",
    }),
    {id:"terminal-item",type:"tool_result",content:"build output",timestamp:new Date(now - LONG_RUNNING_WORK_MS - 2).toISOString()},
  ], [process], now);

  assert.deepEqual(items.map(({kind,threadId}) => [kind,threadId]), [["terminal","root"],["agent","child-thread"]]);
  assert.equal(items[0].outputTail,"build output");
  assert.equal(items[1].prompt,"Inspect the launch path");
  assert.equal(items[1].model,"model-x");
  assert.equal(items[1].effort,"high");
  assert.equal(items[1].status,"running");
});

test("collapses several running items to one count badge", () => {
  const items = composerBackgroundWorkItems([
    message("a", LONG_RUNNING_WORK_MS + 1, {agent_id:"a",agent_parent_thread_id:"root",agent_ordinal:1,agent_state:"running"}),
    message("b", LONG_RUNNING_WORK_MS + 2, {agent_id:"b",agent_parent_thread_id:"root",agent_ordinal:2,agent_state:"started"}),
    message("c", LONG_RUNNING_WORK_MS + 3, {agent_id:"c",agent_parent_thread_id:"root",agent_ordinal:3,agent_state:"running"}),
  ], [], now);

  assert.deepEqual(composerWorkBadgeLayout(items), {kind:"count",count:3,items});
});

test("filters recent, completed, and untimed background work", () => {
  const items = composerBackgroundWorkItems([
    message("recent", LONG_RUNNING_WORK_MS - 1, {agent_id:"recent",agent_parent_thread_id:"root",agent_ordinal:1,agent_state:"running"}),
    message("done", LONG_RUNNING_WORK_MS * 2, {agent_id:"done",agent_parent_thread_id:"root",agent_ordinal:2,agent_state:"completed"}),
    { ...message("untimed", LONG_RUNNING_WORK_MS * 2, {agent_id:"untimed",agent_parent_thread_id:"root",agent_ordinal:3,agent_state:"running"}), timestamp: undefined },
    {id:"short-item",type:"tool_result",content:"short process",timestamp:new Date(now - LONG_RUNNING_WORK_MS + 1).toISOString()},
  ], [
    { ...process, processId:"short", itemId:"short-item" },
    { ...process, processId:"untimed", itemId:"missing-time" },
  ], now);

  assert.deepEqual(items, []);
  assert.deepEqual(composerWorkBadgeLayout(items), {kind:"empty"});
});

test("keeps the current command content as the live terminal tail", () => {
  const items = composerBackgroundWorkItems([
    message("terminal-item", LONG_RUNNING_WORK_MS + 1, {}, {
      toolKind:"command",toolName:"exec_command",toolStatus:"running",content:"live output tail",
    }),
  ], [process], now);

  assert.equal(items[0]?.outputTail,"live output tail");
});
