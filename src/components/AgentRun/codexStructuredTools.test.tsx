import { act, useState } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, describe, expect, it } from "vitest";
import { reduceToolEvents, type NormalizedChatMessage } from "@codeterm/chat-core";
import { ToolCallBubble } from "../../../packages/chat-ui/src/ToolCallBubble";
import { MessageBubble } from "../../../packages/chat-ui/src/MessageBubble";
import fixture from "../../../src-tauri/src/service/codex_rpc/normalize/fixtures/mixed-turn.json";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let root: Root | undefined;
let host: HTMLElement | undefined;
afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
});

async function render(node: React.ReactNode) {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root?.render(node));
  return host;
}

function Timeline({ messages }: { messages: NormalizedChatMessage[] }) {
  return reduceToolEvents(messages).map((item) => (
    <MessageBubble key={item.kind === "tool" ? item.event.id : item.message.id}
      message={item.kind === "tool" ? item.event.message : item.message} item={item} />
  ));
}

function Call({ message }: { message: NormalizedChatMessage }) {
  const [expanded, setExpanded] = useState(false);
  return <ToolCallBubble message={message} expanded={expanded} onToggle={setExpanded} />;
}

describe("Codex structured chat wire contract", () => {
  it("renders the Rust live/history golden messages in order with command outcome and output", async () => {
    const view = await render(<Timeline messages={fixture.messages as NormalizedChatMessage[]} />);
    const card = view.querySelector('[data-testid="chat-tool-call"]');
    expect(card).not.toBeNull();
    const cardText = card?.textContent ?? "";
    expect(view.textContent?.indexOf("Checking blueprints")).toBeLessThan(view.textContent?.indexOf(cardText) ?? -1);
    expect(view.textContent?.indexOf(cardText)).toBeLessThan(view.textContent?.indexOf("Two blueprints") ?? -1);
    expect(view.querySelector('[aria-label="success"]')).not.toBeNull();
    const toggle = view.querySelector<HTMLButtonElement>('[aria-label="Show full command"]');
    expect(toggle).not.toBeNull();
    await act(async () => toggle?.click());
    expect(view.textContent).toContain("blueprint one");
    expect(view.textContent).toContain("blueprint two");
  });

  it("shows MCP name, arguments, status and collapsed formatted output through the existing card", async () => {
    const message: NormalizedChatMessage = { id:"mcp", type:"tool_call", content:"", toolId:"mcp",
      toolKind:"generic", toolName:"codeterm · skill", toolTitle:"codeterm · skill", toolSummary:"name: style",
      toolInput:{name:"style"}, toolError:false, toolResult:'{"count":2,"body":"skill content"}' };
    const view = await render(<Call message={message} />);
    expect(view.textContent).toContain("codeterm · skill");
    expect(view.textContent).toContain("name: style");
    expect(view.textContent).not.toContain("skill content");
    const toggle = view.querySelector<HTMLButtonElement>('[aria-label="Expand tool details"]');
    await act(async () => toggle?.click());
    expect(view.textContent).toContain('"count": 2');
    expect(view.textContent).toContain("skill content");
  });

  it("groups consecutive MCP calls using their turn identity", () => {
    const messages: NormalizedChatMessage[] = [
      {id:"start",type:"tool_flow",content:"start",toolFlowId:"turn"},
      ...["one","two"].map((id): NormalizedChatMessage => ({id,type:"tool_call",content:"",toolId:id,
        toolName:`server · ${id}`,toolTitle:`server · ${id}`,toolKind:"generic",toolSummary:"key: value",toolFlowId:"turn",toolError:false})),
      {id:"end",type:"tool_flow",content:"end",toolFlowId:"turn"},
    ];
    const items = reduceToolEvents(messages);
    expect(items).toHaveLength(1);
    expect(items[0].kind === "tool" && items[0].event.stream?.members.map((member) => member.toolId)).toEqual(["one","two"]);
  });

  it("opens command groups as a parsed tree without a raw block", async () => {
    const message: NormalizedChatMessage = {id:"group-default",type:"tool_call",content:"",toolId:"group-default",
      toolName:"commandExecution",toolKind:"command",toolSummary:"cd /repo; git status; git diff",toolInput:{command:"cd /repo; git status; git diff"},toolError:false,toolResult:"working tree clean"};
    const view = await render(<Timeline messages={[message]} />);
    const groupToggle = view.querySelector<HTMLButtonElement>('[data-testid="chat-command-group-toggle"]');
    expect(groupToggle?.getAttribute("aria-expanded")).toBe("true");
    expect(view.querySelector('[data-testid="raw-command-disclosure"]')).toBeNull();
    expect(view.querySelector('[data-testid="message-raw-content"]')).toBeNull();
    expect(view.textContent).not.toContain("working tree clean");
    await act(async () => groupToggle?.click());
    expect(groupToggle?.getAttribute("aria-expanded")).toBe("false");
  });
});

describe("structured command batches", () => {
  const batch = ["(Get-Culture).Name", "Get-Location", "Get-Date"].map((command, index): NormalizedChatMessage => ({
    id: `batch-${index}`, type: "tool_call", content: "", toolId: `batch-${index}`, toolKind: "command", toolFlowId: "batch-turn",
    toolInput: { command: `pwsh -Command '${command}'`, commandActions: [{command}], commandBatchId: "batch-turn" },
    toolDetail: `pwsh -Command '${command}'`, toolError: false, toolExitCode: 0, toolResult: `result ${index}`,
  }));
  it("shows the parsed command in a standalone row", async () => {
    const view = await render(<Call message={batch[0]} />);
    expect(view.querySelector('[data-testid="toolcard-summary"]')?.textContent).toBe("(Get-Culture).Name");
    expect(view.querySelector('[data-testid="toolcard-summary"]')?.textContent).not.toContain("pwsh");
  });
  it("renders parallel command items in one existing expanded command group", async () => {
    const view = await render(<Timeline messages={batch} />);
    expect(view.querySelectorAll('[data-testid="chat-command-group"]')).toHaveLength(1);
    expect(view.querySelector('[data-testid="chat-command-group-title"]')?.textContent).toBe("(Get-Culture).Name");
    expect(view.textContent).toContain("3 commands");
    expect(view.querySelectorAll('[data-testid="chat-command-tree-line"]')).toHaveLength(3);
    expect(view.querySelector('[data-testid="chat-command-group-toggle"]')?.getAttribute("aria-expanded")).toBe("true");
    expect(view.querySelector('[data-testid="raw-command-disclosure"]')).toBeNull();
    expect(view.textContent).not.toContain("pwsh");
  });
  it("keeps command batches separated by assistant text", async () => {
    const view = await render(<Timeline messages={[...batch, {id:"between-batches",type:"assistant",content:"Next commands"},
      ...batch.map((message) => ({...message,id:`later-${message.id}`,toolId:`later-${message.toolId}`}))]} />);
    expect(view.querySelectorAll('[data-testid="chat-command-group"]')).toHaveLength(2);
    expect(view.textContent).toContain("Next commands");
  });
});
