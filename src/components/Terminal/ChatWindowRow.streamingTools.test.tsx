import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it } from "vitest";
import { materializeToolEvent, type NormalizedChatMessage } from "@codeterm/chat-core";
import { MessageBubble } from "@codeterm/chat-ui";
import { ChatWindowRow } from "./ChatWindowRow";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement | undefined;
let root: Root | undefined;
let sequence = 0;

beforeEach(() => { sequence++; });

afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
  host = undefined;
  root = undefined;
});

const command = "opaque_runner opaque_a && opaque_runner opaque_b";
const part = (patch: Partial<NormalizedChatMessage> = {}): NormalizedChatMessage => ({
  id: `opaque-part-${sequence}`,
  type: "tool_call",
  content: "",
  toolId: `opaque-call-${sequence}`,
  toolName: "bash",
  toolKind: "command",
  toolSummary: "Command",
  ...patch,
});

async function renderPart(message: NormalizedChatMessage) {
  host ??= document.body.appendChild(document.createElement("div"));
  root ??= createRoot(host);
  const event = materializeToolEvent(message)!;
  await act(async () => root!.render(
    <ChatWindowRow key={message.id} message={message}>
      <MessageBubble message={message} item={{ kind: "tool", event }} />
    </ChatWindowRow>,
  ));
  return host;
}

it.each([
  ["group", command],
  ["reader", "cat opaque_file"],
])("retains the tool row and card shell as opaque %s arguments stream", async (_kind, input) => {
  const first = await renderPart(part());
  const row = first.querySelector("[data-chat-window-row]");
  const card = first.querySelector<HTMLElement>('[data-testid="chat-tool-call"]');
  const disclosure = first.querySelector<HTMLButtonElement>('button[aria-label="Show full command"]');
  expect(card).not.toBeNull();
  expect(disclosure?.getAttribute("aria-expanded")).toBe("false");

  for (const patch of [
    { toolInput: { command: "opaque_runner" }, toolSummary: "opaque_runner" },
    { toolInput: { command: input }, toolSummary: input, toolDetail: input },
    { toolInput: { command: input }, toolSummary: input, toolDetail: input, toolResult: "opaque_output", toolError: false },
  ]) {
    const next = await renderPart(part(patch));
    expect(next.querySelector("[data-chat-window-row]")).toBe(row);
    expect(next.querySelector('[data-testid="chat-tool-call"]')).toBe(card);
    expect(next.querySelector('button[aria-label="Show full command"]')).toBe(disclosure);
    expect(disclosure?.getAttribute("aria-expanded")).toBe("false");
    expect(next.querySelector('[data-testid="chat-command-group"]')).toBeNull();
    expect(next.querySelector("p, pre")).toBeNull();
  }

  await act(async () => disclosure!.click());
  expect(disclosure?.getAttribute("aria-expanded")).toBe("true");
  if (input === command) {
    expect(host?.querySelector('[data-testid="chat-command-group"]')?.textContent).toContain("2 commands");
  } else {
    expect(host?.querySelector('[data-testid="toolcard-read-path"]')?.textContent).toBe(input);
  }
});

it("keeps a group collapsed when its full input arrives with the first pending part", async () => {
  const initial = part({ toolInput: { command }, toolSummary: command, toolDetail: command });
  const first = await renderPart(initial);
  const card = first.querySelector('[data-testid="chat-tool-call"]');
  expect(first.querySelector('[data-testid="chat-command-group"]')).toBeNull();
  await renderPart({ ...initial, toolResult: "opaque_done", toolError: false });
  expect(host?.querySelector('[data-testid="chat-tool-call"]')).toBe(card);
  expect(host?.querySelector('button[aria-expanded="false"]')).not.toBeNull();
  expect(host?.querySelector('[data-testid="chat-command-group"]')).toBeNull();
});
