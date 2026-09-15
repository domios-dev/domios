import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, describe, expect, it } from "vitest";
import { reduceToolEvents, type NormalizedChatMessage } from "@codeterm/chat-core";
import { MessageBubble } from "../MessageBubble";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const SESSION = "claude-session";
const CALL = "toolu_014GV5zWvScrS4JGfgwQCbCq";
const AGENT = "af66d7d8e4a99a6bb";
const DESCRIPTION = "Find all enums in snake-demo";
const RESULT = "**Enums found in snake-demo:**\n\n1. **Direction** (snake.js:7-12)";
const ACTIONS = [
  { id: "toolu_ls", kind: "command", summary: "ls -la demo", detail: "ls -la demo", description: "List files in the snake-demo directory", status: "success" },
  { id: "toolu_snake", kind: "read", summary: "demo/snake.js", detail: "demo/snake.js", status: "success" },
  { id: "toolu_check", kind: "read", summary: "demo/check.js", detail: "demo/check.js", status: "success" },
];

function agentRow(id: string, input: Record<string, unknown>): NormalizedChatMessage {
  const summary = typeof input.agent_description === "string" ? input.agent_description : AGENT;
  return {
    id, type: "tool_call", content: summary, collapsed: true, toolName: "agent_activity", toolKind: "agent", toolSummary: summary,
    toolFlowId: CALL, toolInput: { agent_call_id: CALL, ...input }, toolError: false, provider: "claude", sourceSessionId: SESSION,
  };
}

const started = agentRow("sub-start", { agent_state: "started", agent_description: DESCRIPTION, agent_model: "Haiku" });
const launched = agentRow("sub-launch", { agent_id: AGENT, agent_state: "running", agent_description: DESCRIPTION, agent_model: "Haiku" });
const notified = (id: string, patch: Record<string, unknown> = {}) => agentRow(id, {
  agent_id: AGENT, agent_state: "completed", agent_result: RESULT, agent_tokens: 40881, agent_tool_uses: 3,
  agent_duration_ms: 8141, agent_model: "Haiku", agent_actions: ACTIONS, ...patch,
});
const userTurn: NormalizedChatMessage = { id: "user-1", type: "user", content: "thanks", seq: 9, sourceSessionId: SESSION };

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

function all(scope: ParentNode, testId: string): HTMLElement[] {
  return [...scope.querySelectorAll<HTMLElement>(`[data-testid="${testId}"]`)];
}

function text(scope: ParentNode, testId: string): string | null | undefined {
  return scope.querySelector(`[data-testid="${testId}"]`)?.textContent;
}

async function expand(card: HTMLElement) {
  const toggle = card.querySelector<HTMLButtonElement>("button[aria-expanded]");
  expect(toggle).toBeTruthy();
  // Expansion persists per card across renders, so an earlier test may have left it open.
  if (toggle?.getAttribute("aria-expanded") !== "true") await act(async () => toggle?.click());
  expect(toggle?.getAttribute("aria-expanded")).toBe("true");
}

describe("Claude sub-agent card", () => {
  it("renders one card that stays running until the agent's own notification", async () => {
    const view = await render([started, launched, userTurn]);
    const cards = all(view, "chat-agent-activity");
    expect(cards).toHaveLength(1);
    expect(text(cards[0], "chat-agent-title")).toBe(DESCRIPTION);
    expect(text(cards[0], "chat-agent-header-state")).toBe("running");
    expect(text(cards[0], "chat-agent-facts")).toBe("Haiku");
    await expand(cards[0]);
    expect(all(cards[0], "chat-agent-detail")).toHaveLength(0);
  });

  it("completes the same card with usage, expands to the sidechain actions and result, and shows no task marker", async () => {
    const view = await render([started, launched, userTurn, notified("sub-notice")]);
    const cards = all(view, "chat-agent-activity");
    expect(cards).toHaveLength(1);
    const [card] = cards;
    expect(text(card, "chat-agent-header-state")).toBe("completed");
    expect(text(card, "chat-agent-facts")).toBe("Haiku · 8.1s · 3 tool uses · 40.9k tokens");
    expect(view.textContent).not.toContain("Task completed");
    expect(view.textContent).not.toContain("task-notification");

    await expand(card);
    expect(all(card, "chat-agent-row").map((row) => row.dataset.agentState)).toEqual(["completed"]);
    const [stream] = all(card, "chat-tool-stream");
    expect(stream?.dataset.flowActive).toBe("false");
    expect(all(stream, "chat-tool-stream-member").map((member) => member.textContent?.trim())).toEqual([
      "List files in the snake-demo directory", "demo/snake.js", "demo/check.js",
    ]);
    expect(text(card, "chat-agent-result")).toBe(RESULT);
  });

  it("updates the same card when a resumed agent notifies again", async () => {
    const view = await render([
      started, launched, notified("sub-notice-1"), userTurn,
      notified("sub-notice-2", { agent_tokens: 52000, agent_tool_uses: 5, agent_duration_ms: 12000 }),
    ]);
    const cards = all(view, "chat-agent-activity");
    expect(cards).toHaveLength(1);
    expect(text(cards[0], "chat-agent-facts")).toBe("Haiku · 12.0s · 5 tool uses · 52.0k tokens");
  });

  it("degrades to notification data when the sidechain was unreadable", async () => {
    const view = await render([started, launched, notified("sub-bare", { agent_actions: undefined, agent_model: undefined })]);
    const [card] = all(view, "chat-agent-activity");
    expect(text(card, "chat-agent-facts")).toBe("Haiku · 8.1s · 3 tool uses · 40.9k tokens");
    await expand(card);
    expect(all(card, "chat-tool-stream")).toHaveLength(0);
    expect(text(card, "chat-agent-result")).toBe(RESULT);
  });

  it("marks an action the finished agent never resolved as having no result", async () => {
    const pending = [{ ...ACTIONS[0], status: "pending" }];
    const view = await render([started, launched, notified("sub-pending", { agent_actions: pending })]);
    const [card] = all(view, "chat-agent-activity");
    await expand(card);
    expect(all(card, "chat-tool-stream-no-result").length).toBeGreaterThan(0);
  });
});
