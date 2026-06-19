import { attachToolResults, isRenderableChatMessage } from "./toolResults";
import type { NormalizedChatMessage } from "./types";

function eq(actual: unknown, expected: unknown, label: string) {
  const a = JSON.stringify(actual);
  const e = JSON.stringify(expected);
  if (a !== e) {
    throw new Error(`${label}: expected ${e}, got ${a}`);
  }
}

function assert(condition: unknown, label: string): asserts condition {
  if (!condition) throw new Error(label);
}

function toolCall(id: string, patch: Partial<NormalizedChatMessage> = {}): NormalizedChatMessage {
  return {
    id,
    type: "tool_call",
    content: "",
    provider: "codex",
    ...patch,
  };
}

{
  const sparse = toolCall("ghost");
  assert(!isRenderableChatMessage(sparse), "empty sparse tool call is not renderable");
}

{
  const messages = attachToolResults([
    toolCall("exec-1", {
      toolName: "exec_command",
      toolInput: { cmd: "npm test" },
      toolArgs: "npm test",
      toolId: "call-1",
    }),
    {
      id: "result-1",
      type: "tool_result",
      content: "ok",
      provider: "codex",
      toolId: "call-1",
    },
    toolCall("ghost-between-exec-rows"),
    toolCall("exec-2", {
      toolName: "exec_command",
      toolInput: { cmd: "npm run build" },
      toolArgs: "npm run build",
      toolId: "call-2",
    }),
  ]);

  eq(messages.map((message) => message.id), ["exec-1", "exec-2"], "ghost tool rows are filtered");
  eq(messages[0].toolResult, "ok", "real tool result stays attached");
  eq(messages[1].toolName, "exec_command", "real tool row stays renderable");
}

console.log("ok - toolResults ghost-row filtering");
