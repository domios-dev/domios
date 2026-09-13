import React from "react";
import { describe, it } from "vitest";
import { materializeToolEvent, reduceToolEvents } from "@codeterm/chat-core";
import type { NormalizedChatMessage } from "@codeterm/chat-core";
import { ToolCallBubble } from "../ToolCallBubble";
import { render } from "./cardTestSetup";

function call(id: string, patch: Partial<NormalizedChatMessage> = {}): NormalizedChatMessage {
  return {
    id,
    type: "tool_call",
    content: "",
    toolName: "Bash",
    toolId: `${id}-tool`,
    toolKind: "command",
    toolSummary: "printf output",
    toolDetail: "printf output",
    ...patch,
  };
}

function result(id: string, patch: Partial<NormalizedChatMessage> = {}): NormalizedChatMessage {
  return {
    id,
    type: "tool_result",
    content: "output",
    toolId: "call-tool",
    ...patch,
  };
}

function reducedEvent(messages: NormalizedChatMessage[]) {
  const item = reduceToolEvents(messages)[0];
  if (!item || item.kind !== "tool") throw new Error("expected a tool event");
  return item.event;
}

async function renderMessages(messages: NormalizedChatMessage[], expanded = false) {
  const event = reducedEvent(messages);
  return render(<ToolCallBubble event={event} expanded={expanded} onToggle={() => {}} />);
}

const cases: Array<[string, () => Promise<void>]> = [
  ["Claude Bash outcome facts stay exact on the command row", async () => {
    const host = await renderMessages([
      call("claude-success", { toolId: "call-tool", toolCallSourceId: "assistant-success" }),
      result("claude-success-result", {
        toolCallSourceId: "assistant-success",
        toolPairing: "exact",
        toolExitCode: 0,
        toolDurationMs: 1250,
      }),
    ]);
    if (host.querySelector('[data-testid="toolcard-exit-code"]')?.textContent !== "exit 0") throw new Error("Claude exit fact was not rendered");
    if (host.querySelector('[data-testid="toolcard-duration"]')?.textContent !== "1.3s") throw new Error("Claude duration fact was not rendered");
    if (host.querySelector('[data-tool-status="error"]')) throw new Error("successful Claude outcome was marked as an error");
  }],
  ["nonzero command outcomes use a quiet error mark", async () => {
    const host = await renderMessages([
      call("claude-failed", { toolId: "call-tool" }),
      result("claude-failed-result", { toolExitCode: 7 }),
    ]);
    if (!host.querySelector('[data-tool-status="error"]')) throw new Error("nonzero exit did not mark the command error");
    if (!host.querySelector('[data-testid="toolcard-exit-code"]')?.textContent?.includes("exit 7")) throw new Error("nonzero exit fact was not rendered");
  }],
  ["interrupted commands are static and background commands show a chip", async () => {
    const interrupted = await renderMessages([
      call("claude-interrupted", { toolId: "call-tool" }),
      result("claude-interrupted-result", { toolInterrupted: true }),
    ]);
    if (!interrupted.querySelector('[data-tool-status="interrupted"]')) throw new Error("interrupted command was not static");
    if (interrupted.querySelector('[data-testid="toolcard-spinner"]')) throw new Error("interrupted command still spins");
    if (!interrupted.querySelector('[data-testid="toolcard-interrupted"]')) throw new Error("interrupted chip was not rendered");

    const background = await renderMessages([
      call("claude-background", { toolId: "call-tool" }),
      result("claude-background-result", { toolBackgroundTaskId: "task-background" }),
    ]);
    if (!background.querySelector('[data-testid="toolcard-background"]')) throw new Error("background chip was not rendered");
  }],
  ["source identity pairs exact calls and refuses stale or malformed neighbors", async () => {
    const exact = reduceToolEvents([
      call("exact", { toolId: "exact-tool", toolCallSourceId: "assistant-exact" }),
      result("exact-result", { toolId: "exact-tool", toolCallSourceId: "assistant-exact", toolPairing: "exact" }),
    ]);
    if (exact.length !== 1 || exact[0].kind !== "tool" || exact[0].event.result !== "output") throw new Error("exact source identity did not pair");

    const stale = reduceToolEvents([
      call("stale-a", { toolId: undefined, toolCallSourceId: "assistant-a" }),
      call("stale-b", { toolId: undefined, toolCallSourceId: "assistant-b" }),
      result("stale-result", { toolId: undefined, toolCallSourceId: "assistant-old", toolPairing: "exact" }),
    ]);
    if (stale.length !== 2 || stale.some((item) => item.kind !== "tool" && item.message.type !== "tool_result")) throw new Error("stale source identity borrowed a neighbor");
    if (stale.some((item) => item.kind === "tool" && item.event.result)) throw new Error("stale source identity attached a result");

    const exactResultFirst = reduceToolEvents([
      result("exact-result-first", { toolId: undefined, toolCallSourceId: "assistant-late", toolPairing: "exact" }),
      call("exact-call-late", { toolId: undefined, toolCallSourceId: "assistant-late" }),
    ]);
    if (exactResultFirst.length !== 1 || exactResultFirst[0].kind !== "tool" || exactResultFirst[0].event.result !== "output") {
      throw new Error("exact source identity did not pair when the unique call arrived later");
    }

    const malformed = reduceToolEvents([
      call("malformed-call", { toolId: undefined }),
      result("malformed-result", { toolId: undefined, toolPairing: "unavailable" }),
    ]);
    if (malformed.length !== 2 || malformed[0].kind !== "tool" || malformed[0].event.result) throw new Error("malformed identity paired positionally");
  }],
  ["Codex parsed command titles and exact detail survive outcome facts", async () => {
    const exactCommand = "Get-Content -Raw 'C:/repo/SKILL.md'";
    const host = await renderMessages([
      call("codex-read", {
        toolName: "command_execution",
        toolId: "codex-tool",
        toolTitle: "Read SKILL.md",
        toolSummary: exactCommand,
        toolDetail: exactCommand,
      }),
      result("codex-read-result", { toolId: "codex-tool", toolExitCode: 0, toolDurationMs: 2400 }),
    ], true);
    if (!host.textContent?.includes("Read SKILL.md")) throw new Error("parsed_cmd title was not shown");
    if (!host.querySelector('[data-testid="raw-command-disclosure"]')) throw new Error("command detail disclosure disappeared");
    if (!host.querySelector('[data-testid="toolcard-exit-code"]')?.textContent?.includes("exit 0")) throw new Error("Codex exit fact was not rendered");
    if (!host.querySelector('[data-testid="toolcard-duration"]')?.textContent?.includes("2.4s")) throw new Error("Codex duration fact was not rendered");
  }],
  ["stream members retain their own outcome facts", async () => {
    const messages = [
      call("stream-one", { toolId: "stream-one-tool", toolFlowId: "flow", toolDescription: "first", toolError: false, toolExitCode: 0 }),
      call("stream-two", { toolId: "stream-two-tool", toolFlowId: "flow", toolDescription: "second", toolError: false, toolDurationMs: 2100 }),
    ];
    const item = reduceToolEvents(messages)[0];
    if (!item || item.kind !== "tool" || !item.event.stream) throw new Error("expected an activity stream");
    const host = await render(<ToolCallBubble event={item.event} expanded onToggle={() => {}} />);
    if (host.querySelectorAll('[data-testid="toolcard-outcome-facts"]').length !== 2) throw new Error("stream member facts were not presented");
    if (!host.textContent?.includes("exit 0") || !host.textContent.includes("2.1s")) throw new Error("stream member facts changed presentation");
  }],
  ["missing and malformed facts stay absent on ordinary successful rows", async () => {
    const event = materializeToolEvent(call("ordinary", { toolId: "ordinary-tool", toolError: false }));
    if (!event || event.status !== "success") throw new Error("ordinary successful row changed status");
    const host = await renderMessages([call("ordinary", { toolId: "ordinary-tool", toolError: false })]);
    if (host.querySelector('[data-testid="toolcard-outcome-facts"]')) throw new Error("ordinary row invented outcome facts");
    const malformed = materializeToolEvent(call("malformed", {
      toolExitCode: undefined,
      toolDurationMs: undefined,
      toolBackgroundTaskId: undefined,
    }));
    if (!malformed || malformed.exitCode !== undefined || malformed.durationMs !== undefined || malformed.backgroundTaskId !== undefined) {
      throw new Error("malformed facts were not absent");
    }
  }],
  ["equivalent normalized Claude and Codex facts render identically", async () => {
    const normalized = [
      call("normalized", { toolId: "normalized-tool", toolTitle: "command" }),
      result("normalized-result", { toolId: "normalized-tool", toolExitCode: 0, toolDurationMs: 800 }),
    ];
    const claude = await renderMessages(normalized);
    const codex = await renderMessages(normalized.map((message) => ({ ...message, id: `codex-${message.id}` })));
    if (claude.textContent !== codex.textContent) throw new Error("provider-equivalent normalized rows rendered differently");
  }],
];

describe("command facts", () => {
  for (const [name, run] of cases) it(name, run);
});
