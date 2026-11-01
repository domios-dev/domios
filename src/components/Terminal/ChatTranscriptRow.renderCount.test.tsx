import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";
import type { ReactNode } from "react";
import { beginChatSendRetry, addChatSend, setChatSendDraft, failChatSendRequest, removeChatSend } from "./chatSendStore";
import type { DeliverySend } from "./chatDeliveryModel";
import type { NormalizedChatMessage } from "@codeterm/chat-core";
import type { EnrichedChatEntry } from "./chatEnrichment";
import { ChatTranscriptRow, type ChatTranscriptRowContext } from "./ChatTranscriptRow";
import type { MessageRowHandlers } from "./ChatMessageRow";

vi.mock("@codeterm/chat-ui", async (original) => {
  const actual = await original<typeof import("@codeterm/chat-ui")>();
  return {
    ...actual,
    MessageBubble: ({ message, userFooter }: { message: NormalizedChatMessage; userFooter?: ReactNode }) =>
      <div>{message.content}{userFooter}</div>,
  };
});

const handlers: MessageRowHandlers = {
  clipFor: () => null, partsFor: () => [], selectedFor: () => false, cursorFor: () => false, joinNextFor: () => false,
  select: () => {}, rowMouseDown: () => {}, toggle: () => {}, range: () => {}, endRange: () => {}, quote: () => {}, ask: () => {},
};

const context: ChatTranscriptRowContext = {
  tabId: "tab", promptAnswers: true, answeredInteractiveIds: new Set(), handlers,
  onContentLayout: () => {}, resolveTabName: () => undefined, resolvePeerName: () => undefined, onOpenPlan: () => {},
  resolvePlanRef: () => null, onRerunShellCommand: () => {}, onStopReminder: () => {}, onOpenJobs: () => {}, onFocusTab: () => {},
  onJumpToMessage: () => {}, onAnswerAsk: () => {}, deleteSend: () => {}, retrySend: () => {}, forkFrom: () => {}, revertTo: () => {},
};

it("offers Retry for a sendable refusal but never for an empty attachment-only draft", async () => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  const draft = { text: "", attachments: [], recordedVoiceAttachments: [], capsules: {}, reply: null };
  const send: DeliverySend = { id: "empty-refusal", content: "file1", sentAtMs: 1, correlationId: "own", requestFailed: true, draft };
  const message: NormalizedChatMessage = { id: send.id, content: send.content, type: "user" };
  const row: EnrichedChatEntry = { msg: message, item: { kind: "message", message }, origin: undefined };
  const retrySend = vi.fn();
  const render = (deliverySend: DeliverySend) => <ChatTranscriptRow entry={row} reports={[]} asks={[]}
    ariaLabel="Failed send" selected={false} joinNext={false} selecting={false} cursor={false} streaming={false}
    latestFailedToolCall={false} forkable={false} revertible={false} context={{ ...context, retrySend }}
    deliverySend={deliverySend} deliveryView="failed" />;
  try {
    await act(async () => root.render(render(send)));
    expect(Array.from(host.querySelectorAll("button")).some(button => button.textContent === "Retry")).toBe(false);
    addChatSend("empty-retry", send); setChatSendDraft("empty-retry", send.id, draft); failChatSendRequest("empty-retry", send.id);
    expect(beginChatSendRetry("empty-retry", send.id)).toBeUndefined();
    await act(async () => root.render(render({ ...send, draft: { ...draft, text: "sendable" } })));
    const retry = Array.from(host.querySelectorAll("button")).find(button => button.textContent === "Retry");
    expect(retry).toBeDefined();
    await act(async () => retry!.click());
    expect(retrySend).toHaveBeenCalledExactlyOnceWith(send.id);
  } finally { await act(async () => root.unmount()); host.remove(); removeChatSend("empty-retry", send.id); }
});
