import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, describe, expect, it } from "vitest";
import {
  reduceToolEvents,
  stabilizeTimelineItems,
  type AgentActivity,
  type NormalizedChatMessage,
  type ToolEvent,
} from "@codeterm/chat-core";
import { MessageBubble } from "../MessageBubble";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const CODEX = "codex-session";
const CLAUDE = "claude-session";
const OTHER = "other-session";

function boundary(state: "start" | "end", flow: string, sourceSessionId = CODEX): NormalizedChatMessage {
  return { id: `${sourceSessionId}-${state}-${flow}`, type: "tool_flow", content: state, toolFlowId: flow, provider: "codex", sourceSessionId };
}

function call(id: string, title: string, patch: Partial<NormalizedChatMessage> = {}): NormalizedChatMessage {
  return {
    id, type: "tool_call", content: title, provider: "codex", sourceSessionId: CODEX, toolName: "exec", toolId: id,
    toolKind: "command", toolSummary: title, toolDetail: `${title} --exact`, toolTitle: title, toolInput: { raw: `script ${id}` }, ...patch,
  };
}

function result(toolId: string, output: string): NormalizedChatMessage {
  return { id: `${toolId}-result`, type: "tool_result", content: "", sourceSessionId: CODEX, toolId, toolResult: output, toolError: false };
}

function thought(id: string, content: string, patch: Partial<NormalizedChatMessage> = {}): NormalizedChatMessage {
  return { id, type: "thinking", content, collapsed: true, provider: "codex", sourceSessionId: CODEX, ...patch };
}

function agent(id: string, activity: AgentActivity, patch: Partial<NormalizedChatMessage> = {}): NormalizedChatMessage {
  const fields = {
    agent_id: activity.agentId, agent_path: activity.agentPath, agent_state: activity.state,
    message_from: activity.messageFrom, message_to: activity.messageTo,
  };
  const toolInput = Object.fromEntries(Object.entries(fields).filter(([, value]) => value !== undefined));
  const summary = activity.agentPath ?? activity.agentId ?? `${activity.messageFrom} → ${activity.messageTo}`;
  return {
    id, type: "tool_call", content: summary, collapsed: true, toolName: "agent_activity", toolKind: "agent", toolSummary: summary,
    toolInput, toolError: false, provider: "codex", sourceSessionId: CODEX, ...patch,
  };
}

function Timeline({ messages }: { messages: NormalizedChatMessage[] }) {
  return <>{reduceToolEvents(messages).map((item) => {
    const message = item.kind === "message" ? item.message : item.event.message;
    const key = item.kind === "message" ? `m:${message.id}` : `t:${item.event.id}`;
    return <MessageBubble key={key} message={message} item={item.kind === "tool" ? item : undefined} />;
  })}</>;
}

const mounted: { root: Root; host: HTMLElement }[] = [];

afterEach(async () => {
  for (const { root, host } of mounted.splice(0)) {
    await act(async () => root.unmount());
    host.remove();
  }
});

async function render(messages: NormalizedChatMessage[]): Promise<HTMLElement> {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  mounted.push({ root, host });
  await act(async () => root.render(<Timeline messages={messages} />));
  return host;
}

async function click(element: Element | null | undefined) {
  expect(element).toBeTruthy();
  await act(async () => (element as HTMLElement).click());
}

function neutralMarkup(host: HTMLElement): string {
  return host.innerHTML.replace(/\s(?:id|aria-controls)="[^"]*"/g, "");
}

function all(scope: ParentNode, testId: string): HTMLElement[] {
  return [...scope.querySelectorAll<HTMLElement>(`[data-testid="${testId}"]`)];
}

function firstStream(messages: NormalizedChatMessage[]): ToolEvent {
  const item = reduceToolEvents(messages).find((entry) => entry.kind === "tool" && entry.event.stream);
  if (item?.kind !== "tool") throw new Error("expected a stream");
  return item.event;
}

describe("concise thoughts", () => {
  const turn = [
    boundary("start", "turn-t"),
    thought("t-lead", "Lead thought", { toolFlowId: "turn-t" }),
    call("t-a1", "npm test"),
    thought("t-r1", "**Inspect sources**\n\nReading files", { toolFlowId: "turn-t" }),
    call("t-a2", "git status"),
    thought("t-r2", "Compare output"),
    thought("t-foreign", "Foreign thought", { toolFlowId: "turn-t", sourceSessionId: OTHER }),
    thought("t-empty", "   ", { toolFlowId: "turn-t" }),
    call("t-a3", "npm run lint"),
    result("t-a1", "12 passed"),
    result("t-a2", "clean"),
    result("t-a3", "lint ok"),
    boundary("end", "turn-t"),
  ];

  it("renders non-empty same-session thoughts as one dim line between the stream actions they separate", async () => {
    const view = await render(turn);
    const [stream] = all(view, "chat-tool-stream");
    expect(all(view, "chat-tool-stream")).toHaveLength(1);
    await click(stream.querySelector('button[aria-label="Expand 3 actions"]'));

    const rows = [...stream.querySelectorAll<HTMLElement>('[data-testid="chat-tool-stream-member"], [data-testid="chat-tool-stream-thought"]')];
    expect(rows.map((row) => row.dataset.testid)).toEqual([
      "chat-tool-stream-member", "chat-tool-stream-thought", "chat-tool-stream-member", "chat-tool-stream-thought", "chat-tool-stream-member",
    ]);
    expect(all(stream, "chat-thought-line").map((line) => line.textContent)).toEqual(["Inspect sources", "Compare output"]);
    expect(all(stream, "chat-tool-stream-member").map((row) => row.textContent?.trim())).toEqual(["npm test", "git status", "npm run lint"]);
    expect(stream.textContent).not.toContain("Foreign thought");

    const outside = all(view, "chat-thought").filter((node) => !stream.contains(node));
    expect(outside.map((node) => node.querySelector('[data-testid="chat-thought-line"]')?.textContent)).toEqual(["Lead thought", "Foreign thought"]);
  });

  it("keeps exact action details and reuses the stream until a thought changes", () => {
    const event = firstStream(turn);
    expect(event.stream?.members.map((member) => member.id)).toEqual(["t-a1", "t-a2", "t-a3"]);
    expect(event.stream?.members.map((member) => member.detail)).toEqual(["npm test --exact", "git status --exact", "npm run lint --exact"]);
    expect(event.stream?.members[1].result).toBe("clean");

    const first = reduceToolEvents(turn);
    const again = stabilizeTimelineItems(first, reduceToolEvents(turn));
    const index = first.findIndex((item) => item.kind === "tool" && item.event.stream);
    expect(again[index]).toBe(first[index]);
    const edited = turn.map((message) => message.id === "t-r2" ? { ...message, content: "Compare output again" } : message);
    expect(stabilizeTimelineItems(first, reduceToolEvents(edited))[index]).not.toBe(first[index]);
  });

  it("renders nothing for empty or whitespace-only thought text", async () => {
    const view = await render([thought("empty-a", ""), thought("empty-b", " \n "), thought("empty-c", "**  **")]);
    expect(all(view, "chat-thought")).toHaveLength(0);
    expect(view.textContent).toBe("");
  });

  it("shows Claude thinking as one collapsed dim line that expands on demand", async () => {
    const view = await render([thought("claude-think", "Plan the edit carefully\nthen apply it", { provider: "claude", sourceSessionId: CLAUDE })]);
    const [line] = all(view, "chat-thought");
    const toggle = line.querySelector("button");
    expect(all(view, "chat-thought")).toHaveLength(1);
    expect(toggle?.getAttribute("aria-expanded")).toBe("false");
    expect(toggle?.className).toContain("opacity-70");
    expect(toggle?.getAttribute("style")).toContain("var(--ct-muted-fg)");
    expect(line.querySelector('[data-testid="chat-thought-line"]')?.textContent).toBe("Plan the edit carefully");
    expect(line.querySelector('[data-testid="chat-thought-text"]')).toBeNull();
    expect(line.querySelector('[role="region"]')?.hasAttribute("hidden")).toBe(true);

    await click(toggle);
    expect(toggle?.getAttribute("aria-expanded")).toBe("true");
    expect(line.querySelector('[data-testid="chat-thought-text"]')?.textContent).toBe("Plan the edit carefully\nthen apply it");
  });

  it("renders equivalent normalized Claude and Codex thoughts identically", async () => {
    const shared = thought("shared-think", "**Check the plan**\n\nDetails");
    const claude = await render([{ ...shared, provider: "claude", sourceSessionId: CLAUDE }]);
    const codex = await render([{ ...shared, provider: "codex", sourceSessionId: CODEX }]);
    expect(neutralMarkup(claude)).toBe(neutralMarkup(codex));
    expect(claude.querySelector('[data-testid="chat-thought-line"]')?.textContent).toBe("Check the plan");
  });
});

describe("sub-agent activity card", () => {
  const tree = [
    boundary("start", "turn-g"),
    agent("g-1", { agentId: "thread-w1", agentPath: "/root/worker_a", state: "started" }, { toolFlowId: "turn-g" }),
    agent("g-2", { agentId: "thread-w2", agentPath: "/root/worker_a/probe" }, { toolFlowId: "turn-g" }),
    agent("g-3", { messageFrom: "/root", messageTo: "/root/worker_a" }, { toolFlowId: "turn-g" }),
    agent("g-4", { messageFrom: "/root", messageTo: "/root/worker_a" }, { toolFlowId: "turn-g" }),
    agent("g-5", { agentId: "thread-w1", state: "completed" }, { toolFlowId: "turn-g" }),
    agent("g-6", { agentId: "thread-w2", state: "unknown" }, { toolFlowId: "turn-g" }),
    agent("g-7", { messageFrom: "thread-w1", messageTo: "/root" }, { toolFlowId: "turn-g" }),
  ];

  it("folds one turn's records into one collapsed card with source paths, states, and message direction", async () => {
    const view = await render(tree);
    const cards = all(view, "chat-agent-activity");
    expect(cards).toHaveLength(1);
    const [card] = cards;
    const toggle = card.querySelector<HTMLButtonElement>("button[aria-expanded]");
    const region = card.querySelector<HTMLElement>('[role="region"]');
    expect(toggle?.getAttribute("aria-expanded")).toBe("false");
    expect(toggle?.getAttribute("aria-controls")).toBe(region?.id);
    expect(region?.hasAttribute("hidden")).toBe(true);
    expect(all(card, "chat-agent-row")).toHaveLength(0);
    expect(card.querySelector('[data-testid="chat-agent-counts"]')?.textContent).toBe("2 agents · 2 messages");

    await click(toggle);
    expect(toggle?.getAttribute("aria-expanded")).toBe("true");
    expect(region?.hasAttribute("hidden")).toBe(false);
    const rows = all(card, "chat-agent-row");
    expect(rows.map((row) => row.querySelector('[data-testid="chat-agent-name"]')?.textContent)).toEqual(["worker_a", "probe"]);
    expect(rows.map((row) => row.querySelector('[data-testid="chat-agent-path"]')?.textContent)).toEqual(["/root/worker_a", "/root/worker_a/probe"]);
    expect(rows.map((row) => row.dataset.agentState)).toEqual(["completed", "unknown"]);
    expect(rows.map((row) => row.style.paddingLeft)).toEqual(["0px", "12px"]);
    const messages = all(card, "chat-agent-message");
    expect(messages.map((edge) => [
      edge.querySelector('[data-testid="chat-agent-message-from"]')?.textContent,
      edge.querySelector('[data-testid="chat-agent-message-to"]')?.textContent,
    ])).toEqual([["/root", "/root/worker_a"], ["/root/worker_a", "/root"]]);
    expect(messages[0].textContent).toContain("×2");
    expect(messages[1].textContent).not.toContain("×");
  });

  it("omits malformed and self edges, keeps unknown state unknown, and renders no card without valid records", async () => {
    const view = await render([
      agent("m-1", { messageFrom: "/root", messageTo: "/root" }),
      agent("m-2", { messageFrom: "/root" }),
      agent("m-3", { agentPath: "/root/x", state: "bogus" as AgentActivity["state"] }),
      agent("m-4", {}),
      agent("m-5", { messageFrom: " ", messageTo: "/root/x" }),
    ]);
    const [card] = all(view, "chat-agent-activity");
    await click(card.querySelector("button[aria-expanded]"));
    expect(all(card, "chat-agent-row").map((row) => row.dataset.agentState)).toEqual(["unknown"]);
    expect(all(card, "chat-agent-message")).toHaveLength(0);
    expect(card.querySelector('[data-testid="chat-agent-counts"]')?.textContent).toBe("1 agent");

    const empty = await render([agent("e-1", {}), agent("e-2", { messageFrom: "/root", messageTo: "/root" })]);
    expect(all(empty, "chat-agent-activity")).toHaveLength(0);
  });

  it("never merges another session's tree even when flow ids collide", async () => {
    const view = await render([
      agent("s-1", { agentId: "thread-a", agentPath: "/root/mine", state: "running" }, { toolFlowId: "turn-x" }),
      agent("s-2", { agentId: "thread-b", agentPath: "/root/theirs", state: "failed" }, { toolFlowId: "turn-x", sourceSessionId: OTHER }),
      agent("s-3", { messageFrom: "/root", messageTo: "/root/mine" }, { toolFlowId: "turn-x" }),
      agent("s-4", { agentId: "01a05d8b-6c0a-76b1-8921-b8a95660b790" }, { toolFlowId: "turn-x" }),
    ]);
    const cards = all(view, "chat-agent-activity");
    expect(cards).toHaveLength(2);
    for (const card of cards) await click(card.querySelector("button[aria-expanded]"));
    expect(cards.map((card) => all(card, "chat-agent-name").map((name) => name.textContent))).toEqual([
      ["mine", "01a05d8b-6c0a-76b1-8921-b8a95660b790"], ["theirs"],
    ]);
    expect(cards.map((card) => all(card, "chat-agent-message").length)).toEqual([1, 0]);
  });

  it("keeps the tool stream's order and details when agent records interleave", async () => {
    const messages = [
      boundary("start", "turn-o"),
      call("o-a1", "npm test"),
      agent("o-g1", { agentId: "thread-o", agentPath: "/root/helper", state: "started" }, { toolFlowId: "turn-o" }),
      call("o-a2", "git status"),
      result("o-a1", "12 passed"),
      result("o-a2", "clean"),
    ];
    const view = await render(messages);
    const blocks = [...view.querySelectorAll<HTMLElement>('[data-testid="chat-tool-stream"], [data-testid="chat-agent-activity"]')];
    expect(blocks.map((block) => block.dataset.testid)).toEqual(["chat-tool-stream", "chat-agent-activity"]);
    const event = firstStream(messages);
    expect(event.stream?.entries.map((entry) => entry.kind === "action" ? entry.event.id : entry.id)).toEqual(["o-a1", "o-a2"]);
    expect(event.stream?.members.map((member) => [member.detail, member.result])).toEqual([["npm test --exact", "12 passed"], ["git status --exact", "clean"]]);
  });

  it("renders equivalent normalized Claude and Codex agent records identically", async () => {
    const records = tree.slice(1);
    const claude = await render(records.map((message) => ({ ...message, provider: "claude", sourceSessionId: CLAUDE })));
    const codex = await render(records.map((message) => ({ ...message, provider: "codex", sourceSessionId: CODEX })));
    expect(neutralMarkup(claude)).toBe(neutralMarkup(codex));
  });
});

describe("activity presentation", () => {
  it("gates expansion motion behind reduced-motion variants and uses tokens without blinking dots or right borders", async () => {
    const view = await render([
      thought("p-think", "Short thought"),
      agent("p-agent", { agentId: "thread-p", agentPath: "/root/p", state: "running" }),
    ]);
    const [card] = all(view, "chat-agent-activity");
    const [line] = all(view, "chat-thought");
    const chevrons = [card, line].map((scope) => scope.querySelector("button svg:last-child")?.getAttribute("class")?.split(/\s+/) ?? []);
    for (const classes of chevrons) {
      expect(classes).toContain("motion-safe:transition-transform");
      expect(classes).toContain("motion-reduce:transition-none");
      expect(classes).not.toContain("transition-transform");
    }
    expect(view.querySelector('[class*="animate-"]')).toBeNull();
    expect(card.className.split(/\s+/)).toEqual(expect.not.arrayContaining(["border", "border-r", "border-x"]));
    expect(card.style.borderRightWidth).toBe("");
    expect(card.getAttribute("style")).toContain("var(--ct-");

    await click(card.querySelector("button[aria-expanded]"));
    expect(card.querySelector('[data-testid="chat-agent-state"]')?.getAttribute("style")).toContain("var(--ct-yellow)");
    expect(view.querySelector('[class*="animate-"]')).toBeNull();
  });
});
