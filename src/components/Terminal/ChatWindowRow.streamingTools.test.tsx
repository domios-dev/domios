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
  sourceSessionId: `opaque-session-${sequence}`,
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

async function remountPart(message: NormalizedChatMessage) {
  await act(async () => root!.unmount());
  root = createRoot(host!);
  return renderPart(message);
}

function expectUniqueDisclosureTargets(view: HTMLElement) {
  const identified = Array.from(view.querySelectorAll<HTMLElement>("[id]"));
  expect(new Set(identified.map((element) => element.id)).size).toBe(identified.length);
  for (const control of view.querySelectorAll("[aria-controls]")) {
    for (const id of control.getAttribute("aria-controls")!.split(/\s+/)) {
      expect(identified.filter((element) => element.id === id)).toHaveLength(1);
    }
  }
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
    expect(host?.querySelector('[role="group"][aria-label="2 commands"]')).not.toBeNull();
    expect(host?.querySelectorAll('[data-testid="chat-command-tree-line"]')).toHaveLength(2);
  } else {
    expect(host?.querySelector('[data-testid="toolcard-read-path"]')?.textContent).toBe(input);
  }
});

it.each([
  ["read", "cat opaque_file", '[data-testid="toolcard-read-path"]'],
  ["search", "rg opaque_pattern opaque_file", '[data-testid="toolcard-result"]'],
  ["action", "domios docs OPAQUE_TOPIC", '[data-testid="codeterm-action-detail"]'],
])("owns one disclosure frame for an expanded streamed %s command", async (_kind, input, detailSelector) => {
  const initial = part();
  const first = await renderPart(initial);
  const row = first.querySelector("[data-chat-window-row]");
  const card = first.querySelector('[data-testid="chat-tool-call"]');
  const disclosure = first.querySelector<HTMLButtonElement>('button[aria-label="Show full command"]')!;
  const completed = { ...initial, toolInput: { command: input }, toolSummary: input, toolDetail: input, toolResult: "opaque_result", toolError: false };
  await renderPart(completed);
  await act(async () => disclosure.click());
  expect(host?.querySelectorAll('[data-testid="chat-tool-call"]')).toHaveLength(1);
  expect(host?.querySelector(detailSelector)).not.toBeNull();
  expectUniqueDisclosureTargets(host!);

  const rawDisclosure = host!.querySelector<HTMLButtonElement>('[data-testid="raw-command-disclosure"] > button')!;
  await act(async () => rawDisclosure.click());
  expectUniqueDisclosureTargets(host!);
  expect(host?.querySelector('[data-testid="message-raw-content"]')?.textContent).toBe(input);
  await act(async () => rawDisclosure.click());
  expect(disclosure.getAttribute("aria-expanded")).toBe("true");
  expect(host?.querySelector('[data-testid="message-raw-content"]')).toBeNull();
  expectUniqueDisclosureTargets(host!);

  await act(async () => disclosure.click());
  expect(disclosure.getAttribute("aria-expanded")).toBe("false");
  expect(host?.querySelector("[data-chat-window-row]")).toBe(row);
  expect(host?.querySelector('[data-testid="chat-tool-call"]')).toBe(card);
  await remountPart(completed);
  expect(host?.querySelector('button[aria-label="Show full command"]')?.getAttribute("aria-expanded")).toBe("false");
  expect(host?.querySelector(detailSelector)).toBeNull();
});

it("retains a collapsed streamed presentation through settled remounts and transport rekeying", async () => {
  const initial = part();
  await renderPart(initial);
  const completed = { ...initial, toolInput: { command }, toolSummary: command, toolDetail: command, toolResult: "opaque_done", toolError: false };
  await renderPart(completed);
  const rekeyed = { ...completed, id: "opaque-rekeyed-part" };
  await remountPart(rekeyed);
  expect(host?.querySelector('[data-testid="chat-tool-call"]')).not.toBeNull();
  expect(host?.querySelector('[data-testid="chat-command-group"]')).toBeNull();
  expect(host?.querySelector('[data-testid="chat-command-tree"]')).toBeNull();
  expect(host?.querySelector('button[aria-label="Show full command"]')?.getAttribute("aria-expanded")).toBe("false");
  expect(host?.querySelector("[data-chat-window-row]")?.getAttribute("data-chat-window-row")).toBe(rekeyed.id);
});

it("restores explicit user expansion of the retained shell after remount", async () => {
  const initial = part();
  await renderPart(initial);
  const completed = { ...initial, toolInput: { command }, toolSummary: command, toolDetail: command, toolResult: "opaque_done", toolError: false };
  await renderPart(completed);
  await act(async () => host!.querySelector<HTMLButtonElement>('button[aria-label="Show full command"]')!.click());
  await remountPart(completed);
  expect(host?.querySelectorAll('[data-testid="chat-tool-call"]')).toHaveLength(1);
  expect(host?.querySelector('button[aria-label="Hide full command"]')?.getAttribute("aria-expanded")).toBe("true");
  expect(host?.querySelector('[data-testid="chat-command-group"]')).toBeNull();
  expect(host?.querySelector('[role="group"][aria-label="2 commands"]')).not.toBeNull();
});

it("preserves historical group presentation for a first-seen settled call in a different session", async () => {
  const initial = part();
  await renderPart(initial);
  const historical = { ...initial, sourceSessionId: "opaque-historical-session", toolInput: { command }, toolSummary: command, toolDetail: command, toolResult: "opaque_done", toolError: false };
  await remountPart(historical);
  expect(host?.querySelector('[data-testid="chat-tool-call"]')).toBeNull();
  expect(host?.querySelector('[data-testid="chat-command-group"]')).not.toBeNull();
  expect(host?.querySelector('[data-testid="chat-command-group-toggle"]')?.getAttribute("aria-expanded")).toBe("true");
});

it("keeps structured single-command diffs behind the retained explicit disclosure", async () => {
  const initial = part();
  const first = await renderPart(initial);
  const row = first.querySelector("[data-chat-window-row]");
  const card = first.querySelector('[data-testid="chat-tool-call"]');
  const completed = part({
    toolInput: { command: "opaque_runner opaque_file" },
    toolSummary: "opaque_runner opaque_file",
    toolDetail: "opaque_runner opaque_file",
    toolError: false,
    fileDiffs: [{
      path: "opaque_file.txt", additions: 1, deletions: 1,
      hunks: [{ oldStart: 1, oldLines: 1, newStart: 1, newLines: 1, lines: ["-opaque_old", "+opaque_new"] }],
    }],
  });
  await renderPart(completed);
  expect(host?.querySelector('[data-testid="file-diff-cards"]')).toBeNull();
  const disclosure = host!.querySelector<HTMLButtonElement>('button[aria-label="Show full command"]')!;
  await act(async () => disclosure.click());
  expect(host?.querySelector("[data-chat-window-row]")).toBe(row);
  expect(host?.querySelector('[data-testid="chat-tool-call"]')).toBe(card);
  expect(host?.querySelector('[data-testid="file-diff-cards"]')?.textContent).toContain("opaque_file.txt");
  expect(Array.from(host!.querySelectorAll('[data-testid="file-diff-line"]')).map((line) => line.textContent)).toEqual(["-opaque_old", "+opaque_new"]);
  expectUniqueDisclosureTargets(host!);
  const diffDisclosure = host!.querySelector<HTMLButtonElement>('[data-testid="file-diff-cards"] button[aria-controls]')!;
  await act(async () => diffDisclosure.click());
  expect(disclosure.getAttribute("aria-expanded")).toBe("true");
  expect(host?.querySelector('[data-testid="file-diff-line"]')).toBeNull();
  await act(async () => disclosure.click());
  expect(host?.querySelector('[data-testid="file-diff-cards"]')).toBeNull();
  expect(host?.querySelector('[data-testid="chat-tool-call"]')).toBe(card);
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

it.each([false, true])("keeps outbound message content primary when arguments start complete: %s", async (startsComplete) => {
  const sendCommand = 'domios send --tab opaque_target "opaque_outbound_body"';
  const pending = part({ toolInput: { command: sendCommand }, toolSummary: sendCommand, toolDetail: sendCommand });
  const first = await renderPart(startsComplete ? pending : part());
  const row = first.querySelector("[data-chat-window-row]");
  const card = first.querySelector('[data-testid="chat-tool-call"], [data-testid="chat-sent-to-card"]');
  await renderPart(pending);
  const body = host!.querySelector('[data-testid="chat-sent-message"]');
  expect(body).not.toBeNull();
  expect(body?.textContent).toContain("opaque_outbound_body");
  expect(body?.closest("[hidden]")).toBeNull();
  expect(host?.querySelector("[data-chat-window-row]")).toBe(row);
  expect(host?.querySelector('[data-testid="chat-tool-call"], [data-testid="chat-sent-to-card"]')).toBe(card);

  const completed = { ...pending, toolError: false };
  await renderPart(completed);
  expect(host?.querySelector('[data-testid="chat-sent-message"]')).toBe(body);
  if (!startsComplete) {
    const disclosure = host!.querySelector<HTMLButtonElement>('button[aria-label="Show full command"]')!;
    expect(disclosure.getAttribute("aria-expanded")).toBe("false");
    await act(async () => disclosure.click());
    expect(host?.querySelectorAll('[data-testid="chat-sent-message"]')).toHaveLength(1);
    expect(host?.querySelector('[data-testid="chat-sent-message"]')).toBe(body);
    expectUniqueDisclosureTargets(host!);
    await act(async () => disclosure.click());
    expect(host?.querySelector('[data-testid="chat-sent-message"]')).toBe(body);
    expect(body?.closest("[hidden]")).toBeNull();
  }
  await remountPart(completed);
  expect(host?.querySelector('[data-testid="chat-sent-message"]')?.textContent).toContain("opaque_outbound_body");
  expect(host?.querySelector('[data-testid="chat-sent-message"]')?.closest("[hidden]")).toBeNull();
});
