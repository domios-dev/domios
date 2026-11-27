import { describe, expect, it } from "vitest";
import {
  groupToolRuns,
  hasPendingToolPrompt,
  normalizedToolIdentity,
  reduceToolEvents,
  stabilizeTimelineItems,
  toolCallSummaryLine,
  type NormalizedChatMessage,
  type TimelineItem,
  type ToolEvent,
} from "./index";

const CLAUDE_SESSION = "claude-sess-1";
const CODEX_SESSION = "codex-sess-1";
const ACP_SESSION = "acp-sess-1";

function claudeComputerCall(
  index: number,
  actionSummary: string,
  patch: Partial<NormalizedChatMessage> = {},
): NormalizedChatMessage[] {
  const id = `tu_chrome_${index}`;
  return [
    {
      id: `msg-call-${index}`,
      provider: "claude",
      sourceSessionId: CLAUDE_SESSION,
      type: "tool_call",
      content: "mcp__claude-in-chrome__computer",
      toolId: id,
      toolName: "mcp__claude-in-chrome__computer",
      toolKind: "generic",
      toolSummary: "mcp__claude-in-chrome__computer",
      toolDetail: "mcp__claude-in-chrome__computer",
      toolFlowId: `msg_api_${index}`,
      toolInput: {
        action: "left_click",
        coordinate: [200 + index * 10, 300],
        action_summary: actionSummary,
      },
      ...patch,
    },
    {
      id: `msg-result-${index}`,
      provider: "claude",
      sourceSessionId: CLAUDE_SESSION,
      type: "tool_result",
      content: "",
      toolId: id,
      toolResult: `Clicked step ${index}`,
      toolError: patch.toolError ?? false,
    },
  ];
}

function codexMcpCall(
  index: number,
  args: Record<string, unknown>,
  patch: Partial<NormalizedChatMessage> = {},
): NormalizedChatMessage {
  const id = `codex-mcp-${index}`;
  return {
    id,
    seq: index,
    provider: "codex",
    sourceSessionId: CODEX_SESSION,
    type: "tool_call",
    content: "",
    toolId: id,
    toolName: "chrome · computer",
    toolTitle: "chrome · computer",
    toolKind: "generic",
    toolFlowId: "turn-codex-1",
    toolSummary: JSON.stringify(args, null, 2),
    toolInput: args,
    toolStatus: "completed",
    toolError: false,
    toolResult: `codex output ${index}`,
    ...patch,
  };
}

function acpToolCall(
  index: number,
  patch: Partial<NormalizedChatMessage> = {},
): NormalizedChatMessage {
  const id = `acp-call-${index}`;
  return {
    id,
    seq: index,
    provider: "acp",
    sourceSessionId: ACP_SESSION,
    type: "tool_call",
    content: "",
    toolId: id,
    toolName: "read_file",
    toolTitle: `Read src/file${index}.ts`,
    toolKind: "read",
    toolLocations: [{ path: `src/file${index}.ts`, line: index * 10 }],
    toolInput: { path: `src/file${index}.ts` },
    toolStatus: "completed",
    toolError: false,
    toolResult: `contents ${index}`,
    ...patch,
  };
}

function toolItem(items: TimelineItem[], index = 0): ToolEvent {
  const item = items[index];
  if (!item || item.kind !== "tool") throw new Error(`expected tool item at index ${index}`);
  return item.event;
}

describe("toolRuns pure grouping model", () => {
  it("collapses consecutive Claude MCP calls across sequential API message flows", () => {
    const messages = [
      ...claudeComputerCall(1, "Open funnel builder"),
      ...claudeComputerCall(2, "Remove checkout_viewed step"),
      ...claudeComputerCall(3, "Add pricing_viewed step"),
      ...claudeComputerCall(4, "Save funnel"),
    ];
    const items = reduceToolEvents(messages);
    expect(items).toHaveLength(1);
    const run = toolItem(items, 0);
    expect(run.id).toBe("msg-call-1:run");
    expect(run.status).toBe("success");
    expect(run.summary).toBe("mcp__claude-in-chrome__computer × 4");
    expect(run.toolRun?.toolName).toBe("mcp__claude-in-chrome__computer");
    expect(run.toolRun?.members).toHaveLength(4);
    expect(run.toolRun?.runningCount).toBe(0);
    expect(run.toolRun?.failedCount).toBe(0);
    expect(run.toolRun?.members.map(toolCallSummaryLine)).toEqual([
      "Open funnel builder",
      "Remove checkout_viewed step",
      "Add pricing_viewed step",
      "Save funnel",
    ]);
  });

  it("absorbs intervening thinking between matching calls while keeping trailing thinking outside", () => {
    const [c1, r1] = claudeComputerCall(1, "Click step 1");
    const [c2, r2] = claudeComputerCall(2, "Click step 2");
    const [c3, r3] = claudeComputerCall(3, "Click step 3");
    const messages: NormalizedChatMessage[] = [
      c1,
      r1,
      { id: "think-between", type: "thinking", content: "Checking button coordinates", sourceSessionId: CLAUDE_SESSION },
      c2,
      r2,
      { id: "think-empty", type: "thinking", content: "   ", sourceSessionId: CLAUDE_SESSION },
      c3,
      r3,
      { id: "think-after", type: "thinking", content: "Now summarize the funnel", sourceSessionId: CLAUDE_SESSION },
      { id: "assistant-done", type: "assistant", content: "Done updating the funnel.", sourceSessionId: CLAUDE_SESSION },
    ];
    const items = reduceToolEvents(messages);
    expect(items).toHaveLength(3);
    const run = toolItem(items, 0);
    expect(run.toolRun?.members.map((m) => m.id)).toEqual(["msg-call-1", "msg-call-2", "msg-call-3"]);
    expect(run.toolRun?.entries.map((e) => (e.kind === "action" ? e.event.id : `thought:${e.id}`))).toEqual([
      "msg-call-1",
      "thought:think-between",
      "msg-call-2",
      "msg-call-3",
    ]);
    expect(items[1].kind === "message" && items[1].message.id).toBe("think-after");
    expect(items[2].kind === "message" && items[2].message.id).toBe("assistant-done");
  });

  it("collapses Codex runtime MCP runs with running state while any call runs", () => {
    const runningMessages: NormalizedChatMessage[] = [
      codexMcpCall(1, { action_summary: "Navigate to settings" }),
      codexMcpCall(2, { action_summary: "Click Save" }, { toolStatus: "running", toolError: undefined, toolResult: undefined }),
    ];
    const live = reduceToolEvents(runningMessages, null, true);
    expect(live).toHaveLength(1);
    const liveRun = toolItem(live, 0);
    expect(liveRun.summary).toBe("chrome · computer × 2");
    expect(liveRun.status).toBe("pending");
    expect(liveRun.toolRun?.runningCount).toBe(1);
    expect(liveRun.toolRun?.failedCount).toBe(0);
    expect(liveRun.toolRun?.members.map(toolCallSummaryLine)).toEqual([
      "Navigate to settings",
      "Click Save",
    ]);

    const settledMessages: NormalizedChatMessage[] = [
      codexMcpCall(1, { action_summary: "Navigate to settings" }),
      codexMcpCall(2, { action_summary: "Click Save" }),
    ];
    const settled = reduceToolEvents(settledMessages, null, true);
    expect(settled).toHaveLength(1);
    expect(toolItem(settled, 0).status).toBe("success");
    expect(toolItem(settled, 0).toolRun?.runningCount).toBe(0);
  });

  it("collapses ACP tool calls on normalized toolName+kind and leaves unnamed calls ungrouped", () => {
    const acpRun = reduceToolEvents([acpToolCall(1), acpToolCall(2), acpToolCall(3)], null, true);
    expect(acpRun).toHaveLength(1);
    const grouped = toolItem(acpRun, 0);
    expect(grouped.summary).toBe("read_file × 3");
    expect(grouped.toolRun?.members.map(toolCallSummaryLine)).toEqual([
      "Read src/file1.ts",
      "Read src/file2.ts",
      "Read src/file3.ts",
    ]);

    const unnamed = reduceToolEvents([
      { id: "u-1", type: "tool_call", content: "", sourceSessionId: ACP_SESSION, toolId: "u-1", toolStatus: "completed", toolResult: "ROVER-TOOL-1" },
      { id: "u-2", type: "tool_call", content: "", sourceSessionId: ACP_SESSION, toolId: "u-2", toolStatus: "completed", toolResult: "ROVER-TOOL-2" },
    ]);
    expect(unnamed).toHaveLength(2);
    expect(unnamed.every((item) => item.kind === "tool" && !item.event.toolRun)).toBe(true);
  });

  it("groups identically regardless of provider field (normalized fields only)", () => {
    const base = [
      ...claudeComputerCall(1, "Step one"),
      ...claudeComputerCall(2, "Step two"),
    ];
    const withoutProvider = base.map((msg) => ({ ...msg, provider: undefined }));
    const withProvider = reduceToolEvents(base);
    const neutral = reduceToolEvents(withoutProvider);
    expect(neutral).toHaveLength(1);
    expect(toolItem(neutral, 0).summary).toBe(toolItem(withProvider, 0).summary);
    expect(toolItem(neutral, 0).toolRun?.members.map(toolCallSummaryLine)).toEqual(
      toolItem(withProvider, 0).toolRun?.members.map(toolCallSummaryLine),
    );
  });

  it("keeps errored calls inside the group with failure count and pinned error list", () => {
    const messages = [
      ...claudeComputerCall(1, "Click Add step"),
      ...claudeComputerCall(2, "Rename step 3", { toolError: true }),
      ...claudeComputerCall(3, "Cancel and retry"),
    ];
    const items = reduceToolEvents(messages);
    expect(items).toHaveLength(1);
    const run = toolItem(items, 0);
    expect(run.summary).toBe("mcp__claude-in-chrome__computer × 3");
    expect(run.status).toBe("success");
    expect(run.error).toBe(true);
    expect(run.toolRun?.latestStatus).toBe("success");
    expect(run.toolRun?.failedCount).toBe(1);
    expect(run.toolRun?.errors.map((err) => err.id)).toEqual(["msg-call-2"]);

    const trailingError = reduceToolEvents([
      ...claudeComputerCall(1, "Click Add step"),
      ...claudeComputerCall(2, "Save step", { toolError: true }),
    ]);
    const failedRun = toolItem(trailingError, 0);
    expect(failedRun.status).toBe("error");
    expect(failedRun.toolRun?.latestStatus).toBe("error");
    expect(failedRun.toolRun?.failedCount).toBe(1);
  });

  it("never merges different tools, different sessions, or calls separated by prose/user turns", () => {
    const differentTools = reduceToolEvents([
      ...claudeComputerCall(1, "Click button"),
      {
        id: "read-1",
        sourceSessionId: CLAUDE_SESSION,
        type: "tool_call",
        content: "",
        toolId: "read-1",
        toolName: "Read",
        toolKind: "read",
        toolSummary: "src/app.ts",
        toolError: false,
      },
      ...claudeComputerCall(2, "Click again"),
    ]);
    expect(differentTools).toHaveLength(3);
    expect(differentTools.every((item) => item.kind === "tool" && !item.event.toolRun)).toBe(true);

    const proseSplit = reduceToolEvents([
      ...claudeComputerCall(1, "Step 1"),
      ...claudeComputerCall(2, "Step 2"),
      { id: "prose", type: "assistant", content: "Moving to the next section.", sourceSessionId: CLAUDE_SESSION },
      ...claudeComputerCall(3, "Step 3"),
      ...claudeComputerCall(4, "Step 4"),
    ]);
    expect(proseSplit).toHaveLength(3);
    expect(toolItem(proseSplit, 0).summary).toBe("mcp__claude-in-chrome__computer × 2");
    expect(proseSplit[1].kind).toBe("message");
    expect(toolItem(proseSplit, 2).summary).toBe("mcp__claude-in-chrome__computer × 2");

    const crossSession = reduceToolEvents([
      codexMcpCall(1, { action_summary: "A" }, { sourceSessionId: "sess-a" }),
      codexMcpCall(2, { action_summary: "B" }, { sourceSessionId: "sess-b" }),
    ]);
    expect(crossSession).toHaveLength(2);
  });

  it("never hides a pending permission or interactive prompt inside a collapsed group", () => {
    const [c1, r1] = claudeComputerCall(1, "Open page");
    const [c2, r2] = claudeComputerCall(2, "Click form");
    const promptedCall: NormalizedChatMessage = {
      ...claudeComputerCall(3, "Submit payment")[0],
      toolId: "tu_chrome_prompt",
      id: "msg-call-prompt",
      interactive: [
        {
          type: "question",
          id: "tu_chrome_prompt-0",
          toolUseId: "tu_chrome_prompt",
          prompt: "Allow submitting this form?",
          options: [{ label: "Allow" }, { label: "Deny" }],
          multiSelect: false,
        },
      ],
    };
    const withInteractive = reduceToolEvents([c1, r1, c2, r2, promptedCall]);
    expect(withInteractive).toHaveLength(2);
    expect(toolItem(withInteractive, 0).summary).toBe("mcp__claude-in-chrome__computer × 2");
    expect(toolItem(withInteractive, 1).id).toBe("msg-call-prompt");
    expect(toolItem(withInteractive, 1).toolRun).toBeUndefined();

    const pendingCall = codexMcpCall(
      3,
      { action_summary: "Run privileged action" },
      { toolId: "call-awaiting", id: "call-awaiting", toolStatus: "running", toolError: undefined, toolResult: undefined },
    );
    const awaitingById = reduceToolEvents(
      [codexMcpCall(1, { action_summary: "A" }), codexMcpCall(2, { action_summary: "B" }), pendingCall],
      null,
      true,
      new Set(["call-awaiting"]),
    );
    expect(awaitingById).toHaveLength(2);
    expect(toolItem(awaitingById, 0).summary).toBe("chrome · computer × 2");
    expect(toolItem(awaitingById, 1).id).toBe("call-awaiting");
    expect(hasPendingToolPrompt(toolItem(awaitingById, 1), new Set(["call-awaiting"]))).toBe(true);

    const awaitingByInput = reduceToolEvents(
      [
        codexMcpCall(1, { action_summary: "A" }),
        codexMcpCall(2, { action_summary: "B" }),
        codexMcpCall(3, { action_summary: "Needs approval", awaitingApproval: true }, { toolStatus: "running", toolError: undefined, toolResult: undefined }),
      ],
      null,
      true,
    );
    expect(awaitingByInput).toHaveLength(2);
    expect(toolItem(awaitingByInput, 1).toolRun).toBeUndefined();
  });

  it("stays stable under live streaming appends and matches reload exactly", () => {
    const full = [
      ...claudeComputerCall(1, "Click 1"),
      ...claudeComputerCall(2, "Click 2"),
      ...claudeComputerCall(3, "Click 3"),
      ...claudeComputerCall(4, "Click 4"),
    ];

    const step1 = stabilizeTimelineItems(undefined, reduceToolEvents(full.slice(0, 2)));
    expect(step1).toHaveLength(1);
    expect(toolItem(step1, 0).toolRun).toBeUndefined();

    const step2 = stabilizeTimelineItems(step1, reduceToolEvents(full.slice(0, 4)));
    expect(step2).toHaveLength(1);
    expect(toolItem(step2, 0).id).toBe("msg-call-1:run");
    expect(toolItem(step2, 0).summary).toBe("mcp__claude-in-chrome__computer × 2");

    const step3 = stabilizeTimelineItems(step2, reduceToolEvents(full.slice(0, 5)));
    expect(step3).toHaveLength(1);
    expect(toolItem(step3, 0).id).toBe("msg-call-1:run");
    expect(toolItem(step3, 0).status).toBe("pending");
    expect(toolItem(step3, 0).toolRun?.runningCount).toBe(1);

    const step4 = stabilizeTimelineItems(step3, reduceToolEvents(full));
    const reloaded = reduceToolEvents(full);
    expect(step4).toEqual(reloaded);

    const withTail = stabilizeTimelineItems(step4, reduceToolEvents([
      ...full,
      { id: "tail-msg", type: "assistant", content: "Streaming reply...", sourceSessionId: CLAUDE_SESSION },
    ]));
    expect(withTail[0]).toBe(step4[0]);
  });

  it("extracts one-line summaries from action_summary, description, title, path, command, or action without raw JSON", () => {
    const makeEvent = (patch: Partial<NormalizedChatMessage>, eventPatch: Partial<ToolEvent> = {}): ToolEvent => ({
      id: "e",
      phase: "call",
      status: "success",
      kind: patch.toolKind ?? "generic",
      summary: patch.toolSummary ?? "",
      description: patch.toolDescription,
      title: patch.toolTitle,
      detail: patch.toolDetail,
      locations: patch.toolLocations,
      message: { id: "e", type: "tool_call", content: "", toolName: "tool", ...patch },
      ...eventPatch,
    });

    expect(toolCallSummaryLine(makeEvent({ toolInput: { action_summary: "Click\nSave" } }))).toBe("Click Save");
    expect(toolCallSummaryLine(makeEvent({ toolDescription: "List git status", toolInput: { command: "git status" } }))).toBe("List git status");
    expect(toolCallSummaryLine(makeEvent({ toolName: "read_file", toolTitle: "Read config.ts" }))).toBe("Read config.ts");
    expect(toolCallSummaryLine(makeEvent({ toolName: "Read", toolKind: "read", toolInput: { file_path: "src/index.ts" } }))).toBe("src/index.ts");
    expect(toolCallSummaryLine(makeEvent({ toolName: "Bash", toolKind: "command", toolDetail: "git diff --stat", toolInput: { command: "git diff --stat" } }))).toBe("git diff --stat");
    expect(toolCallSummaryLine(makeEvent({ toolName: "chrome · computer", toolSummary: '{"x":1}', toolInput: { action: "screenshot" } }))).toBe("screenshot");
    expect(toolCallSummaryLine(makeEvent({ toolName: "chrome · computer", toolSummary: '{"x":1}', toolInput: { x: 1 } }))).toBeUndefined();
    expect(normalizedToolIdentity(makeEvent({ toolName: "   " }))).toBeNull();
    expect(groupToolRuns([])).toEqual([]);
  });
});
