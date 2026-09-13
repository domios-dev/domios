import { act, useState } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, describe, expect, it } from "vitest";
import { reduceToolEvents, type NormalizedChatMessage, type TimelineItem, type ToolEvent } from "@codeterm/chat-core";
import { ToolCallBubble } from "../../../packages/chat-ui/src/ToolCallBubble";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const CLAUDE_SESSION = "claude-session";
const CODEX_SESSION = "codex-session";

function claudeCall(id: string, command: string, patch: Partial<NormalizedChatMessage> = {}): NormalizedChatMessage {
  const description = patch.toolDescription;
  return {
    id, type: "tool_call", content: command, provider: "claude", sourceSessionId: CLAUDE_SESSION,
    toolName: "Bash", toolId: id, toolKind: "command", toolSummary: command, toolDetail: command, toolArgs: command,
    toolInput: { command, ...(description ? { description } : {}) }, ...patch,
  };
}

function result(toolId: string, output: string, sourceSessionId: string): NormalizedChatMessage {
  return { id: `${toolId}-result`, type: "tool_result", content: "", sourceSessionId, toolId, toolResult: output, toolError: false };
}

function codexCall(id: string, title: string, patch: Partial<NormalizedChatMessage> = {}): NormalizedChatMessage {
  return {
    id, type: "tool_call", content: title, provider: "codex", sourceSessionId: CODEX_SESSION, toolName: "exec", toolId: id,
    toolKind: "generic", toolSummary: title, toolDetail: title, toolTitle: title, toolInput: { raw: `script ${id}` }, ...patch,
  };
}

function boundary(state: "start" | "end", turnId: string): NormalizedChatMessage {
  return { id: `flow-${state}-${turnId}`, type: "tool_flow", content: state, toolFlowId: turnId, provider: "codex", sourceSessionId: CODEX_SESSION };
}

function Row({ item }: { item: TimelineItem }) {
  const [expanded, setExpanded] = useState(false);
  if (item.kind === "message") return <p data-testid="timeline-message">{item.message.content}</p>;
  return <ToolCallBubble event={item.event} expanded={expanded} onToggle={setExpanded} />;
}

function Timeline({ messages }: { messages: NormalizedChatMessage[] }) {
  return <>{reduceToolEvents(messages).map((item) => (
    <Row key={item.kind === "message" ? item.message.id : item.event.message.id} item={item} />
  ))}</>;
}

let root: Root | null = null;
let host: HTMLElement | null = null;

afterEach(async () => {
  const mounted = root;
  if (mounted) await act(async () => mounted.unmount());
  host?.remove();
  root = null;
  host = null;
});

async function render(messages: NormalizedChatMessage[]): Promise<HTMLElement> {
  if (!host) {
    host = document.createElement("div");
    document.body.append(host);
    root = createRoot(host);
  }
  const mounted = root as Root;
  await act(async () => mounted.render(<Timeline messages={messages} />));
  return host;
}

async function click(element: Element | null | undefined) {
  expect(element).toBeTruthy();
  await act(async () => (element as HTMLElement).click());
}

function streams(scope: ParentNode): HTMLElement[] {
  return [...scope.querySelectorAll<HTMLElement>('[data-testid="chat-tool-stream"]')];
}

function actionLabels(stream: HTMLElement): string[] {
  return [...stream.querySelectorAll('[data-testid="chat-tool-stream-member"]')].map((member) => member.textContent?.trim() ?? "");
}

function streamEvent(messages: NormalizedChatMessage[], index = 0): ToolEvent {
  const item = reduceToolEvents(messages).filter((entry) => entry.kind === "tool" && entry.event.stream)[index];
  if (item?.kind !== "tool") throw new Error("expected a stream");
  return item.event;
}

const lintOutput = "lint ok\nno warnings";
const linked = [
  claudeCall("tu-1", "npm test -- --run", { toolDescription: "Run unit tests", toolFlowId: "msg_parallel" }),
  claudeCall("tu-2", "npm run lint", { toolDescription: "Lint sources", toolFlowId: "msg_parallel" }),
  claudeCall("tu-3", "npx tsc --noEmit", { toolDescription: "Type-check", toolFlowId: "msg_parallel" }),
];
const linkedResults = [
  result("tu-1", "tests passed", CLAUDE_SESSION),
  result("tu-2", lintOutput, CLAUDE_SESSION),
  result("tu-3", "no type errors", CLAUDE_SESSION),
];

describe("Claude tool activity", () => {
  it("renders linked described calls as one compact stream whose title rolls as descriptions arrive", async () => {
    const view = await render([linked[0]]);
    expect(streams(view)).toHaveLength(0);

    await render(linked.slice(0, 2));
    const [stream] = streams(view);
    expect(streams(view)).toHaveLength(1);
    expect(view.querySelectorAll('[data-testid="chat-tool-call"]')).toHaveLength(0);
    expect(stream.querySelector('[data-testid="chat-tool-stream-title"]')?.textContent).toBe("Lint sources");
    expect(stream.dataset.flowActive).toBe("true");
    expect(stream.style.maxHeight).toBe("200px");
    expect(stream.querySelector('[data-testid="chat-tool-stream-mini"]')?.textContent).toContain("Run unit tests");

    await render(linked);
    expect(streams(view)[0].querySelector('[data-testid="chat-tool-stream-title"]')?.textContent).toBe("Type-check");

    await render([...linked, ...linkedResults]);
    const settled = streams(view)[0];
    expect(settled.dataset.flowActive).toBe("false");
    expect(settled.querySelector('[data-testid="chat-tool-stream-mini"]')).toBeNull();
    expect(settled.style.maxHeight).toBe("");
  });

  it("expands to every action and opens the exact command, input, and output of the selected one", async () => {
    const messages = [...linked, ...linkedResults];
    const view = await render(messages);
    const [stream] = streams(view);
    const toggle = stream.querySelector<HTMLButtonElement>('button[aria-label="Expand 3 actions"]');
    await click(toggle);
    expect(toggle?.getAttribute("aria-expanded")).toBe("true");
    expect(actionLabels(stream)).toEqual(["Run unit tests", "Lint sources", "Type-check"]);

    await click(stream.querySelector('button[aria-label="Show Lint sources"]'));
    const card = stream.querySelector<HTMLElement>('[data-testid="chat-tool-call"]');
    expect(card?.querySelector('[data-testid="toolcard-summary"]')?.textContent).toBe("Lint sources");
    expect(card?.querySelector('[data-testid="toolcard-result"]')?.textContent).toContain("no warnings");
    await click([...(card?.querySelectorAll("button") ?? [])].find((button) => button.textContent?.includes("Show raw command")));
    expect(card?.querySelector('[data-testid="message-raw-content"]')?.textContent).toBe("npm run lint");

    const member = streamEvent(messages).stream?.members[1];
    expect(member?.message.toolInput).toEqual({ command: "npm run lint", description: "Lint sources" });
    expect(member?.detail).toBe("npm run lint");
    expect(member?.result).toBe(lintOutput);
  });

  it("keeps described calls without a shared flow as rows that show the description instead of the command", async () => {
    const view = await render([
      claudeCall("solo-1", "git status --short", { toolDescription: "Check worktree", toolFlowId: "msg_a" }),
      claudeCall("solo-2", "git log -1", { toolDescription: "Show last commit", toolFlowId: "msg_b" }),
      claudeCall("solo-3", "pwd", { toolDescription: "Print directory" }),
    ]);
    expect(streams(view)).toHaveLength(0);
    const summaries = [...view.querySelectorAll('[data-testid="toolcard-summary"]')].map((node) => node.textContent);
    expect(summaries).toEqual(["Check worktree", "Show last commit", "Print directory"]);
    expect(view.querySelector('[data-testid="chat-tool-call"]')?.textContent).toContain("command");
  });

  it("keeps undescribed calls on today's row title and expansion even when they share a flow", async () => {
    const view = await render([
      claudeCall("read-1", "src/a.ts", { toolName: "Read", toolKind: "read", toolFlowId: "msg_x", toolInput: { file_path: "src/a.ts" } }),
      claudeCall("grep-1", "TODO", { toolName: "Grep", toolKind: "search", toolFlowId: "msg_x", toolInput: { pattern: "TODO" } }),
    ]);
    expect(streams(view)).toHaveLength(0);
    const cards = [...view.querySelectorAll<HTMLElement>('[data-testid="chat-tool-call"]')];
    expect(cards).toHaveLength(2);
    expect(cards[0].querySelector('[data-testid="toolcard-summary"]')?.textContent).toBe("src/a.ts");
    const toggle = cards[0].querySelector<HTMLButtonElement>("button[aria-controls]");
    expect(toggle?.getAttribute("aria-expanded")).toBe("false");
    await click(toggle);
    expect(toggle?.getAttribute("aria-expanded")).toBe("true");
  });
});

describe("Codex tool activity", () => {
  const turnMessages = [
    boundary("start", "turn-a"),
    codexCall("call-a1", "npm test", { toolKind: "command", toolDetail: "npm test -- --run" }),
    { id: "narration", type: "assistant", content: "checking output", sourceSessionId: CODEX_SESSION } as NormalizedChatMessage,
    codexCall("call-a2", "Wait for command output"),
    codexCall("call-a3", "wait_agent", { toolName: "wait_agent", toolInput: { ids: ["agent-1"], timeout_ms: 30000 } }),
    result("call-a1", "12 passed", CODEX_SESSION),
    result("call-a2", "poll output", CODEX_SESSION),
    result("call-a3", "agent finished", CODEX_SESSION),
    boundary("end", "turn-a"),
    boundary("start", "turn-b"),
    codexCall("call-b1", "Read shot.png", { toolKind: "read", toolSummary: "/repo/shot.png" }),
    codexCall("call-b2", "git status", { toolKind: "command", toolDetail: "git status" }),
  ];

  it("groups every call of one turn into a stream with honest titles and wait follow-ups as ordinary actions", async () => {
    const view = await render(turnMessages);
    const [first, second] = streams(view);
    expect(streams(view)).toHaveLength(2);
    expect(view.querySelector('[data-testid="chat-wait-batch"]')).toBeNull();
    expect(view.querySelector('[data-testid="timeline-message"]')?.textContent).toBe("checking output");
    expect(first.querySelector('[data-testid="chat-tool-stream-title"]')?.textContent).toBe("wait_agent");
    expect(first.dataset.flowActive).toBe("false");
    expect(second.dataset.flowActive).toBe("true");

    await click(first.querySelector('button[aria-label="Expand 3 actions"]'));
    expect(actionLabels(first)).toEqual(["npm test", "Wait for command output", "wait_agent"]);
    await click(first.querySelector('button[aria-label="Show Wait for command output"]'));
    expect(first.querySelector('[data-testid="chat-tool-call"]')?.textContent).toContain("poll output");

    await click(second.querySelector('button[aria-label="Expand 2 actions"]'));
    expect(actionLabels(second)).toEqual(["Read shot.png", "git status"]);
  });

  it("keeps calls from different turns apart and preserves exact per-call payloads", () => {
    const first = streamEvent(turnMessages, 0);
    const second = streamEvent(turnMessages, 1);
    expect(first.stream?.flowId).toBe("turn-a");
    expect(second.stream?.flowId).toBe("turn-b");
    expect(first.stream?.members.map((member) => member.id)).toEqual(["call-a1", "call-a2", "call-a3"]);
    expect(first.stream?.members[0].detail).toBe("npm test -- --run");
    expect(first.stream?.members[0].result).toBe("12 passed");
    expect(first.stream?.members[2].message.toolInput).toEqual({ ids: ["agent-1"], timeout_ms: 30000 });
  });

  it("leaves calls without a proven turn as individual rows", async () => {
    const view = await render([codexCall("loose-1", "npm test"), codexCall("loose-2", "git status")]);
    expect(streams(view)).toHaveLength(0);
    expect(view.querySelectorAll('[data-testid="chat-tool-call"]')).toHaveLength(2);
  });
});

describe("stream presentation", () => {
  it("gates motion behind reduced-motion variants and uses theme tokens without blinking dots or right borders", async () => {
    const view = await render(linked.slice(0, 2));
    const [stream] = streams(view);
    const spinner = stream.querySelector('[data-testid="chat-tool-stream-spinner"]');
    const spinnerClasses = spinner?.getAttribute("class")?.split(/\s+/) ?? [];
    expect(spinnerClasses).toContain("motion-safe:animate-spin");
    expect(spinnerClasses).not.toContain("animate-spin");
    expect(stream.querySelector('[data-testid="chat-tool-stream-title"]')?.className).toContain("motion-reduce:transition-none");
    expect(stream.querySelector('[class*="animate-pulse"], [class*="animate-ping"]')).toBeNull();
    expect(stream.className.split(/\s+/)).toEqual(expect.not.arrayContaining(["border", "border-r", "border-x"]));
    expect(stream.style.borderRightWidth).toBe("");
    expect(stream.getAttribute("style")).toContain("var(--ct-");
  });
});
