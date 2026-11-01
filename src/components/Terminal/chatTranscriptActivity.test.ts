import assert from "node:assert/strict";
import { test } from "node:test";
import type { NormalizedChatMessage } from "@codeterm/chat-core";
import type { ProviderDescriptor } from "../../lib/types";
import { chatLiveOperationFullLabel, chatLiveOperationLabel, projectChatTranscriptActivity } from "./chatTranscriptActivity";

const chatProvider = { transport: "chat_backend" } as Pick<ProviderDescriptor, "transport" | "agent_runtime">;
const runtimeProvider = { agent_runtime: {} } as Pick<ProviderDescriptor, "transport" | "agent_runtime">;
const ptyRuntimeProvider = { transport: "pty", agent_runtime: {} } as Pick<ProviderDescriptor, "transport" | "agent_runtime">;
const ptyProvider = { transport: "pty" } as Pick<ProviderDescriptor, "transport" | "agent_runtime">;
const tool = (id: string, toolStatus: NonNullable<NormalizedChatMessage["toolStatus"]>): NormalizedChatMessage => ({
  id,
  type: "tool_call",
  content: "",
  toolKind: "command",
  toolSummary: "gh run view 39149606573",
  toolStatus,
});

test("enables transcript activity from chat transport capabilities and leaves PTY providers alone", () => {
  assert.equal(projectChatTranscriptActivity([], "working", chatProvider, undefined, false).supported, true);
  assert.equal(projectChatTranscriptActivity([], "working", runtimeProvider, undefined, false).supported, true);
  assert.equal(projectChatTranscriptActivity([], "working", ptyProvider, { kind: "pty" }, false).supported, false);
  assert.equal(projectChatTranscriptActivity([], "working", ptyRuntimeProvider, { kind: "pty" }, true).supported, false);
  assert.equal(projectChatTranscriptActivity([], "working", undefined, { kind: "chat_backend", plugin_id: "fixture", session_id: "thread" }, false).supported, true);
  assert.equal(projectChatTranscriptActivity([], "working", undefined, undefined, true).supported, true);
});

test("marks the latest running tool in place and nothing when no tool row is live", () => {
  const projection = projectChatTranscriptActivity([
    { msg: tool("completed", "completed") },
    { msg: tool("first-running", "running") },
    { msg: tool("latest-running", "running") },
  ], "working", chatProvider, undefined, false);
  assert.deepEqual(projection, { supported: true, liveToolId: "latest-running" });

  assert.deepEqual(projectChatTranscriptActivity([], "working", chatProvider, undefined, false), { supported: true, liveToolId: null });
  assert.deepEqual(projectChatTranscriptActivity([{ msg: tool("stale", "running") }], "idle", chatProvider, undefined, false), {
    supported: true,
    liveToolId: null,
  });
});

test("restored operation labels are deterministic", () => {
  assert.equal(chatLiveOperationLabel(tool("operation", "running")), "gh run view 39149606573");
});

test("full transcript labels retain multiline commands and complete targets", () => {
  const command = "domios plugin browser goto http://127.0.0.1:14997/;\ndomios plugin browser screenshot evidence/chat-running-dark.png";
  assert.equal(chatLiveOperationFullLabel({ ...tool("cmd", "running"), toolInput: { cmd: command } }), command);
  assert.equal(chatLiveOperationFullLabel({ ...tool("read", "running"), toolKind: "read", toolInput: { file_path: "/workspace/components/Terminal/a-very-long-file-name.tsx" } }), "/workspace/components/Terminal/a-very-long-file-name.tsx");
  assert.deepEqual(projectChatTranscriptActivity([{ msg: tool("done", "completed") }], "working", runtimeProvider, undefined, false), { supported: true, liveToolId: null });
});
