import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import { MessageBubble } from "@codeterm/chat-ui";
import type { NormalizedChatMessage } from "@codeterm/chat-core";
import { ChatWorkingRow } from "./ChatWorkingRow";
import { ChatWindowRow } from "./ChatWindowRow";
import { chatLiveOperationLabel, chatWorkingRowMessage } from "./chatTranscriptActivity";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement | undefined;
let root: Root | undefined;

afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
  root = undefined;
  host = undefined;
  vi.restoreAllMocks();
});

const operation = (toolStatus: NonNullable<NormalizedChatMessage["toolStatus"]>): NormalizedChatMessage => ({
  id: "operation",
  type: "tool_call",
  content: "",
  toolId: "operation",
  toolKind: "command",
  toolSummary: "gh run view 39149606573 --repo rollacode/codeterm",
  toolDetail: "gh run view 39149606573 --repo rollacode/codeterm --job 1",
  toolInput: { cmd: "gh run view 39149606573 --repo rollacode/codeterm --job 1" },
  toolStatus,
  toolStartedAtMs: 1_000,
  ...(toolStatus === "completed" ? { toolCompletedAtMs: 5_000 } : {}),
});

async function renderOperation(message: NormalizedChatMessage) {
  host ??= document.body.appendChild(document.createElement("div"));
  root ??= createRoot(host);
  const liveActivityLabel = message.toolStatus === "running" ? chatLiveOperationLabel(message) : undefined;
  await act(async () => root?.render(
    <ChatWindowRow key={message.id} message={message} liveActivityLabel={liveActivityLabel}>
      <MessageBubble message={message} />
    </ChatWindowRow>,
  ));
  return host;
}

it("keeps one accessible tool row and settles the same card in place", async () => {
  const clock = vi.spyOn(Date, "now").mockReturnValue(6_000);
  const running = await renderOperation(operation("running"));
  const row = running.querySelector<HTMLElement>('[data-chat-window-row="operation"]');
  const card = running.querySelector('[data-testid="chat-tool-call"]');
  expect(row?.getAttribute("role")).toBe("status");
  expect(row?.getAttribute("aria-live")).toBe("polite");
  expect(row?.getAttribute("aria-label")).toContain("gh run view");
  expect(running.querySelectorAll('[data-chat-window-row="operation"]')).toHaveLength(1);
  expect(running.querySelectorAll('[data-testid="chat-tool-call"]')).toHaveLength(1);
  expect(running.querySelector('[data-testid="toolcard-summary"]')?.textContent).toContain("gh run view");
  expect(running.querySelector('[role="timer"]')?.textContent).toBe("Running · 5s");
  const expand = running.querySelector<HTMLButtonElement>('button[aria-label="Show full command"]');
  expect(expand).not.toBeNull();
  await act(async () => expand?.click());
  const revealRawCommand = running.querySelector<HTMLButtonElement>('[data-testid="raw-command-disclosure"] button');
  expect(revealRawCommand?.textContent).toBe("Raw command");
  await act(async () => revealRawCommand?.click());
  expect(running.querySelector('[data-testid="message-raw-content"]')?.textContent)
    .toContain("gh run view 39149606573 --repo rollacode/codeterm --job 1");

  const completed = await renderOperation(operation("completed"));
  expect(completed.querySelector('[data-chat-window-row="operation"]')).toBe(row);
  expect(completed.querySelector('[data-testid="chat-tool-call"]')).toBe(card);
  expect(completed.querySelectorAll('[data-testid="chat-tool-call"]')).toHaveLength(1);
  expect(completed.querySelector('[data-chat-window-row="operation"]')?.hasAttribute("aria-live")).toBe(false);
  expect(completed.querySelector('[data-testid="chat-tool-call"]')?.getAttribute("data-tool-status")).toBe("success");
  expect(completed.querySelector('[role="timer"]')?.textContent).toBe("completed · 4s");
  clock.mockRestore();
});

it("rebuilds one live operation row from restored transcript state", async () => {
  const first = await renderOperation(operation("running"));
  expect(first.querySelectorAll('[data-testid="chat-tool-call"]')).toHaveLength(1);
  await act(async () => root?.unmount());
  root = createRoot(host!);
  const restored = await renderOperation(operation("running"));
  expect(restored.querySelectorAll('[data-chat-window-row="operation"]')).toHaveLength(1);
  expect(restored.querySelectorAll('[data-testid="chat-tool-call"]')).toHaveLength(1);
  expect(restored.querySelector('[data-chat-window-row="operation"]')?.getAttribute("role")).toBe("status");
});

it("renders a compact accessible Working tail row", async () => {
  const onContentLayout = vi.fn();
  const message = chatWorkingRowMessage("tab");
  host = document.body.appendChild(document.createElement("div"));
  root = createRoot(host);
  await act(async () => root?.render(
    <ChatWindowRow message={message} liveActivityLabel="Working">
      <ChatWorkingRow onContentLayout={onContentLayout} />
    </ChatWindowRow>,
  ));
  const row = host.querySelector(`[data-chat-window-row="${message.id}"]`);
  expect(row?.getAttribute("role")).toBe("status");
  expect(row?.getAttribute("aria-live")).toBe("polite");
  expect(host.querySelector('[data-testid="chat-working-row"]')?.textContent).toBe("Working…");
  expect(host.querySelector('[data-testid="chat-working-row"] .activity-dots')?.getAttribute("aria-hidden")).toBe("true");
  expect(onContentLayout).toHaveBeenCalledTimes(1);
});
