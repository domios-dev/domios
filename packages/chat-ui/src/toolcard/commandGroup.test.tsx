import React from "react";
import { describe, expect, it } from "vitest";
import { reduceToolEvents, type NormalizedChatMessage, type ToolEvent } from "@codeterm/chat-core";
import { toolCommandShape } from "../codeterm/commandShape";
import { ToolCallBubble } from "../ToolCallBubble";
import { act, render } from "./cardTestSetup";

const CODEX_SESSION = "01a09a2f-b88c-79b3-a9dd-83f8741f6810";
const CODEX_PIPELINE = [
  "codeterm mem secret get --name almaz-admin-clerk-secret-key | python -c \"import sys,json,httpx",
  "key = sys.stdin.read().strip()",
  "r = httpx.get('https://api.clerk.com/v1/users', headers={'Authorization': 'Bearer ' + key})",
  "print(json.dumps(r.json()[:1], indent=2))\"",
].join("\n");
const CODEX_ARGV = ["C:\\Program Files\\PowerShell\\7\\pwsh.exe", "-Command", CODEX_PIPELINE];

/** Projection of the rollout's exec custom_tool_call, its output, and the CommandExecution item that echoes it. */
function codexRollout(): NormalizedChatMessage[] {
  const session = { sourceSessionId: CODEX_SESSION, provider: "codex" };
  return [
    {
      id: "call_exec", type: "tool_call", content: "", toolName: "exec", toolId: "call_exec", toolKind: "command",
      toolSummary: CODEX_PIPELINE, toolDetail: CODEX_PIPELINE, toolTitle: "codeterm mem",
      toolInput: { raw: `text(await tools.exec_command({cmd: ${JSON.stringify(CODEX_PIPELINE)}}));` }, ...session,
    },
    { id: "call_exec:result", type: "tool_result", content: "Script completed\nOutput:\n[{\"id\": \"user_1\"}]", toolId: "call_exec", ...session },
    {
      id: "activity-cmd_4333", type: "tool_call", content: "", toolName: "command_execution", toolId: "cmd_4333", toolKind: "command",
      toolSummary: CODEX_ARGV.join(" "), toolDetail: CODEX_ARGV.join(" "), toolTitle: "C:\\Program Files\\PowerShell\\7\\pwsh.exe",
      toolInput: { command: CODEX_ARGV.join(" "), argv: CODEX_ARGV }, toolEcho: true, toolError: false,
      toolResult: "[{\"id\": \"user_1\"}]", toolExitCode: 0, toolDurationMs: 1830, ...session,
    },
  ];
}

/** Projection of a Claude jsonl Bash tool_use with its description and a paired tool_result. */
function claudeBash(command: string, description?: string): NormalizedChatMessage[] {
  const session = { sourceSessionId: "claude-session", provider: "claude" };
  return [
    {
      id: "toolu_bash", type: "tool_call", content: "", toolName: "Bash", toolId: "toolu_bash", toolKind: "command",
      toolSummary: command, toolDetail: command, ...(description ? { toolDescription: description } : {}),
      toolInput: { command, ...(description ? { description } : {}) }, toolFlowId: "msg_1", toolCallSourceId: "assistant-1", ...session,
    },
    {
      id: "toolu_bash:result", type: "tool_result", content: "3", toolId: "toolu_bash", toolCallSourceId: "assistant-1",
      toolPairing: "exact", toolExitCode: 0, toolDurationMs: 420, ...session,
    },
  ];
}

function singleEvent(messages: NormalizedChatMessage[]): ToolEvent {
  const items = reduceToolEvents(messages);
  expect(items).toHaveLength(1);
  if (items[0].kind !== "tool") throw new Error("expected one tool event");
  return items[0].event;
}

async function click(element: Element | null | undefined) {
  if (!element) throw new Error("missing clickable element");
  await act(async () => (element as HTMLElement).click());
}

describe("compound command group", () => {
  it("renders the Codex pwsh pipeline once as one box with a CodeTerm row and a python script row", async () => {
    const host = await render(<ToolCallBubble event={singleEvent(codexRollout())} expanded={false} onToggle={() => {}} />);
    const groups = host.querySelectorAll('[data-testid="chat-command-group"]');
    expect(groups).toHaveLength(1);
    expect(host.children).toHaveLength(1);
    const group = groups[0];
    const rows = [...group.querySelectorAll<HTMLElement>('[data-testid="chat-command-group-row"]')];
    expect(rows.map((row) => row.dataset.rowRole)).toEqual(["codeterm", "script"]);
    expect([...group.querySelectorAll<HTMLElement>('[data-testid="chat-command-group-operator"]')].map((node) => node.dataset.operator)).toEqual(["pipe"]);
    expect(group.querySelector('[data-testid="chat-command-group-title"]')?.textContent).toBe("codeterm mem secret get | python (4 lines)");

    const codetermRow = rows[0];
    expect(codetermRow.querySelector('[data-testid="chat-tool-call"]')).not.toBeNull();
    expect(codetermRow.querySelector(".lucide-brain")).not.toBeNull();
    expect(codetermRow.textContent).toContain("mem");

    expect(host.querySelectorAll('[data-testid="toolcard-exit-code"]')).toHaveLength(1);
    expect(host.querySelectorAll('[data-testid="toolcard-duration"]')).toHaveLength(1);
    expect(group.querySelector('[data-testid="chat-command-group-toggle"] [data-testid="toolcard-exit-code"]')?.textContent).toBe("exit 0");
  });

  it("expands an inline script into a highlighted body with its language and line count", async () => {
    const host = await render(<ToolCallBubble event={singleEvent(codexRollout())} expanded={false} onToggle={() => {}} />);
    const scriptButton = host.querySelector<HTMLElement>('[data-testid="chat-command-group-script"]');
    expect(scriptButton?.dataset.language).toBe("python");
    expect(scriptButton?.textContent).toContain("4 lines");
    expect(host.querySelector('[data-testid="chat-command-script-body"]')).toBeNull();
    await click(scriptButton);
    const body = host.querySelector<HTMLElement>('[data-testid="chat-command-script-body"]');
    expect(body?.dataset.language).toBe("python");
    expect(body?.textContent).toContain("import sys,json,httpx");
    expect(body?.querySelector(".token")).not.toBeNull();
  });

  it("keeps the whole call's output and exact raw command on the group", async () => {
    const host = await render(<ToolCallBubble event={singleEvent(codexRollout())} expanded onToggle={() => {}} />);
    expect(host.querySelectorAll('[data-testid="toolcard-result"]')).toHaveLength(1);
    await click(host.querySelector('[data-testid="chat-command-group"] > [role="region"] [data-testid="raw-command-disclosure"] > button'));
    expect(host.querySelector('[data-testid="message-raw-content"]')?.textContent).toBe(CODEX_PIPELINE);
  });

  it("titles a Claude Bash compound by its description and joins rows with their operators", async () => {
    const command = "gh issue list --state open --json number && gh pr list || echo offline; git status";
    const host = await render(<ToolCallBubble event={singleEvent(claudeBash(command, "Count open work"))} expanded={false} onToggle={() => {}} />);
    const group = host.querySelector('[data-testid="chat-command-group"]');
    expect(group?.querySelector('[data-testid="chat-command-group-title"]')?.textContent).toBe("Count open work");
    expect([...host.querySelectorAll<HTMLElement>('[data-testid="chat-command-group-operator"]')].map((node) => node.dataset.operator)).toEqual(["and", "or", "then"]);
    expect(host.querySelectorAll('[data-testid="chat-command-group-command"]')).toHaveLength(4);
    expect(host.querySelectorAll('[data-testid="toolcard-exit-code"]')).toHaveLength(1);
  });

  it("reads a Claude heredoc as a script row", async () => {
    const command = "cd /repo && python3 - <<'PY'\nimport json\nprint(json.dumps({\"ok\": True}))\nPY";
    const host = await render(<ToolCallBubble event={singleEvent(claudeBash(command))} expanded={false} onToggle={() => {}} />);
    expect([...host.querySelectorAll<HTMLElement>('[data-testid="chat-command-group-row"]')].map((row) => row.dataset.rowRole)).toEqual(["command", "script"]);
    expect(host.querySelector('[data-testid="chat-command-group-script"]')?.textContent).toContain("2 lines");
  });

  it("falls back to today's single command row when the parser returns null", async () => {
    const command = "echo \"unterminated | python -c 'print(1)'";
    const event = singleEvent(claudeBash(command));
    expect(toolCommandShape(event).kind).toBe("single");
    const host = await render(<ToolCallBubble event={event} expanded={false} onToggle={() => {}} />);
    expect(host.querySelector('[data-testid="chat-command-group"]')).toBeNull();
    expect(host.querySelectorAll('[data-testid="chat-tool-call"]')).toHaveLength(1);
    expect(host.querySelector(".lucide-terminal")).not.toBeNull();
  });

  it("memoizes the shape per call instead of re-parsing", () => {
    const event = singleEvent(codexRollout());
    expect(toolCommandShape({ ...event })).toBe(toolCommandShape(event));
  });
});
