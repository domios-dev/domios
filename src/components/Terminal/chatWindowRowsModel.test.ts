import assert from "node:assert/strict";
import type { NormalizedChatMessage } from "@codeterm/chat-core";
import { chatWindowRows } from "./chatWindowRowsModel";

function thinking(id: string): NormalizedChatMessage {
  return { id, type: "thinking", content: "", collapsed: true };
}

function tool(id: string): NormalizedChatMessage {
  return {
    id,
    type: "tool_call",
    content: "",
    toolName: "commandExecution",
    toolKind: "command",
    toolSummary: "Run focused command",
    toolDetail: "Run focused command",
    toolInput: { status: "completed" },
  };
}

function thinkingRange(from: number, to: number): NormalizedChatMessage[] {
  return Array.from({ length: to - from }, (_, index) => thinking(`thinking-${from + index}`));
}

const sequence = [
  ...thinkingRange(80, 88),
  tool("tool-88"),
  thinking("thinking-89"),
  tool("tool-90"),
  thinking("thinking-91"),
  tool("tool-92"),
  thinking("thinking-93"),
  tool("tool-94"),
  thinking("thinking-95"),
  tool("tool-96"),
  thinking("thinking-97"),
  thinking("thinking-98"),
  tool("tool-99"),
  thinking("thinking-100"),
  tool("tool-101"),
  { id: "user-102", type: "user", content: "Transcript fixture boundary" },
  ...thinkingRange(103, 106),
  tool("tool-106"),
  thinking("thinking-107"),
  tool("tool-108"),
  ...thinkingRange(109, 122),
  tool("tool-122"),
  thinking("thinking-123"),
  tool("tool-124"),
  thinking("thinking-125"),
  tool("tool-126"),
  thinking("thinking-127"),
  { ...tool("tool-128"), toolStatus: "running" },
  ...thinkingRange(129, 164),
  tool("tool-164"),
  ...thinkingRange(165, 175),
] satisfies NormalizedChatMessage[];

const rows = chatWindowRows(sequence.map((msg) => ({ msg })));
assert.deepEqual(rows.map(({ msg }) => msg.id), [
  "tool-88", "tool-90", "tool-92", "tool-94", "tool-96", "tool-99", "tool-101", "user-102",
  "tool-106", "tool-108", "tool-122", "tool-124", "tool-126", "tool-128", "tool-164",
]);

const visible = chatWindowRows([{ msg: { id: "thought", type: "thinking", content: "Reasoning" } }]);
const collapsed = chatWindowRows([{ msg: thinking("thought") }]);
assert.deepEqual(visible.map(({ msg }) => msg.id), ["thought"]);
assert.deepEqual(collapsed, []);

const invisibleMessages = [
  { id: "empty-assistant", type: "assistant", content: " " },
  { id: "empty-status", type: "status", content: "" },
  { id: "empty-tool-call", type: "tool_call", content: "" },
  { id: "empty-tool-result", type: "tool_result", content: "", toolResult: "unrendered" },
  { id: "internal-flow", type: "tool_flow", content: "start" },
  { id: "empty-custom", type: "custom", content: "" },
] satisfies NormalizedChatMessage[];
assert.deepEqual(chatWindowRows(invisibleMessages.map((msg) => ({ msg }))), []);

const queuedMessage = {
  id: "queued-message",
  type: "user",
  content: "queued payload",
  timelineMarker: { kind: "queued" },
} satisfies NormalizedChatMessage;
assert.deepEqual(chatWindowRows([{ msg: queuedMessage }]).map(({ msg }) => msg.id), ["queued-message"]);
