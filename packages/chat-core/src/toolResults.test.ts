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

{
  const messages = attachToolResults([
    toolCall("exec-err", {
      toolName: "exec_command",
      toolInput: { cmd: "false" },
      toolId: "call-err",
    }),
    {
      id: "result-err",
      type: "tool_result",
      content: "command failed",
      provider: "codex",
      toolId: "call-err",
      toolError: true,
    },
  ]);
  eq(messages[0].toolError, true, "tool error propagates to attached tool_call");
}

{
  const messages = attachToolResults([
    toolCall("patch", {
      toolName: "apply_patch",
      toolId: "call-patch",
      toolInput: { patch: "*** Begin Patch" },
    }),
    {
      id: "patch-end",
      type: "tool_result",
      content: "Success. Updated the following files:\nA probe.txt\n",
      provider: "codex",
      toolId: "call-patch",
    },
    {
      id: "custom-output",
      type: "tool_result",
      content: "Exit code: 0\nOutput:\nSuccess. Updated the following files:\nA probe.txt\n",
      provider: "codex",
      toolId: "call-patch",
    },
  ]);

  eq(messages.length, 1, "duplicate lifecycle results stay on one tool row");
  eq(
    messages[0].toolResult,
    "Exit code: 0\nOutput:\nSuccess. Updated the following files:\nA probe.txt\n",
    "richer duplicate result replaces its shorter subset",
  );
}

console.log("ok - toolResults ghost-row filtering");
