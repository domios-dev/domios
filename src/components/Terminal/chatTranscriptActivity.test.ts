import assert from "node:assert/strict";
import { test } from "node:test";
import type { NormalizedChatMessage } from "@codeterm/chat-core";
import type { ProviderDescriptor } from "../../lib/types";
import { chatLiveOperationFullLabel, chatLiveOperationLabel, composerRunningTool, projectChatTranscriptActivity, projectVisibleChatTools } from "./chatTranscriptActivity";
import { conversationEntries } from "./chatPagingModel";

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

test("the composer names no running tool once its runtime is down", () => {
  const rows = [tool("done", "completed"), tool("cut-off", "running")];
  assert.equal(composerRunningTool(rows, "degraded"), undefined);
  assert.equal(composerRunningTool(rows, "unloaded"), undefined);
  assert.equal(composerRunningTool(rows, "loaded")?.id, "cut-off");
  assert.equal(composerRunningTool(rows, null)?.id, "cut-off");
  assert.equal(composerRunningTool([tool("done", "interrupted")], "loaded"), undefined);
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

test("projects short calls as completed rows and keeps a visible command in its slot through settlement", () => {
  const earlier = tool("previous", "completed");
  const running = tool("call", "running");
  const completed = { ...tool("call", "completed"), toolDurationMs: 310, toolResult: "done" };
  const start = projectVisibleChatTools([earlier, running], new Map(), 1000);
  assert.deepEqual(start.rows, [earlier]);
  assert.equal(start.nextChangeAt, 1300);

  const fast = projectVisibleChatTools([earlier, completed], start.running, 1299);
  assert.deepEqual(fast.rows, [earlier, completed]);
  assert.equal(fast.nextChangeAt, Infinity);

  const delta = { ...running, toolResult: "line 1\nline 2" };
  const visible = projectVisibleChatTools([earlier, delta], start.running, 1300);
  assert.deepEqual(visible.rows.map(row => row.id), ["previous", "call"]);
  assert.equal(visible.rows[1].toolResult, delta.toolResult);
  assert.equal(visible.rows[1].toolStartedAtMs, 1000);
  const hold = projectVisibleChatTools([earlier, completed], visible.running, 1310);
  assert.equal(hold.rows[1], visible.rows[1]);
  assert.equal(hold.nextChangeAt, 1700);
  assert.deepEqual(projectVisibleChatTools([earlier, completed], hold.running, 1700).rows, [earlier, completed]);
});

test("a running edit keeps its group slot while details arrive and when it completes", () => {
  const edit = { ...tool("edit", "completed"), toolKind: "edit" as const, toolFlowId: "turn",
    fileDiffs: [{ path: "first.ts", additions: 0, deletions: 0, hunks: [] }], toolSummary: "first.ts" };
  const running = { ...edit, id: "second", toolStatus: "running" as const,
    fileDiffs: undefined, toolSummary: "second.ts", toolLocations: [{ path: "second.ts" }] };
  const pending = projectVisibleChatTools([edit, running], new Map(), 0);
  const visible = projectVisibleChatTools([edit, running], pending.running, 300);
  const before = conversationEntries(pending.rows, undefined, undefined, true);
  const during = conversationEntries(visible.rows, undefined, undefined, true);
  const done = { ...running, toolStatus: "completed" as const,
    fileDiffs: [{ path: "second.ts", additions: 0, deletions: 0, hunks: [] }] };
  const after = conversationEntries(projectVisibleChatTools([edit, done], visible.running, 700).rows, undefined, undefined, true);
  for (const [rows, expected] of [[before, ["edit"]], [during, ["edit", "second"]], [after, ["edit", "second"]]] as const) {
    assert.equal(rows.length, 1);
    assert.equal(rows[0].msg.id, "edit");
    assert.equal(rows[0].item.kind, "tool");
    if (rows[0].item.kind === "tool") {
      assert.equal(rows[0].item.event.id, "edit:edits");
      assert.deepEqual(rows[0].item.event.editRun?.map(member => member.id), expected);
    }
  }
  assert.equal(projectChatTranscriptActivity(during, "working", runtimeProvider, undefined, false).label, "Editing second.ts");
});
