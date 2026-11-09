import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";
import type { ReactNode } from "react";
import { beginChatSendRetry, addChatSend, setChatSendDraft, failChatSendRequest, removeChatSend, observeDeliveryStatus, readChatSends,
  useChatSends, __setChatSendRepositoryForTest } from "./chatSendStore";
import { deliveryState, type DeliverySend } from "./chatDeliveryModel";
import type { NormalizedChatMessage } from "@codeterm/chat-core";
import type { EnrichedChatEntry } from "./chatEnrichment";
import { ChatTranscriptRow, type ChatTranscriptRowContext } from "./ChatTranscriptRow";
import type { MessageRowHandlers } from "./ChatMessageRow";
import { recordDroppedUploads } from "./DroppedUploadNotice";
import { MemoryChatSendRepository } from "./chatSendRepository.testSupport";
import { __setInvokeForTest } from "../../lib/bridge";
import * as audio from "./voice/audio";
import * as attachmentHandoff from "./meshAttachmentHandoff";

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

it("omitted attachment retry stays sending during preparation without recovery controls and sends once", async () => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const tabId = "omitted-preparation";
  __setChatSendRepositoryForTest(new MemoryChatSendRepository());
  let finishPreparation!: (draft: attachmentHandoff.AttachmentDraft) => void;
  const preparation = new Promise<attachmentHandoff.AttachmentDraft>(resolve => { finishPreparation = resolve; });
  const prepare = vi.spyOn(attachmentHandoff, "composeAttachmentDraft").mockReturnValue(preparation);
  const store = vi.spyOn(audio, "storeBlob").mockResolvedValue("/opaque/recovered.png");
  const requests: Array<Record<string, unknown> | undefined> = [];
  let requestObserved!: () => void;
  const sent = new Promise<void>(resolve => { requestObserved = resolve; });
  __setInvokeForTest(async <T,>(command: string, args?: Record<string, unknown>) => {
    expect(command).toBe("chat_send_text");
    requests.push(args);
    requestObserved();
    return { tab_id: tabId, send_id: "retry-tx", correlationId: args?.clientToken, state: "delivered",
      error: null, sequence: 1, retained_from: 1, ledger_id: "ledger" } as T;
  });
  addChatSend(tabId, { id: "original", correlationId: "original", content: "earlier", sentAtMs: 1 });
  recordDroppedUploads(tabId, "original", [{ id: 1, name: "recovered.png", blob: new Blob(["image"], { type: "image/png" }),
    state: "failed", failure: { message: "interrupted", retryable: true } }]);
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  function Harness() {
    return useChatSends(tabId).map(send => {
      const message: NormalizedChatMessage = { id: send.id, type: "user", content: send.content };
      const entry: EnrichedChatEntry = { msg: message, item: { kind: "message", message }, origin: undefined };
      return <ChatTranscriptRow key={send.id} entry={entry} reports={[]} asks={[]} ariaLabel="Attachment send"
        selected={false} joinNext={false} selecting={false} cursor={false} streaming={false} latestFailedToolCall={false}
        forkable={false} revertible={false} context={{ ...context, tabId }} deliverySend={send} deliveryView={deliveryState(send)} />;
    });
  }
  try {
    await act(async () => root.render(<Harness />));
    const retry = host.querySelector<HTMLButtonElement>('button[aria-label="Retry omitted attachments"]');
    expect(retry).not.toBeNull();
    await act(async () => retry!.click());
    expect(prepare).toHaveBeenCalledTimes(1);
    const send = readChatSends(tabId).find(item => item.id !== "original")!;
    expect(send.correlationId).toBe(send.id);
    expect(deliveryState(send)).toBe("sending");
    const bubble = host.querySelector(`[data-message-id="${send.id}"]`)!;
    expect(bubble.querySelector('[role="status"][data-state="sending"]')).not.toBeNull();
    expect([...bubble.querySelectorAll("button")].map(button => button.textContent)).not.toContain("Remove");
    expect([...bubble.querySelectorAll("button")].map(button => button.textContent)).not.toContain("Edit and resend");
    expect(requests).toHaveLength(0);
    await act(async () => retry!.click());
    await act(async () => { finishPreparation({ composed: "prepared attachment", transcribeFailed: false }); await sent; });
    expect(requests).toHaveLength(1);
    expect(requests[0]?.clientToken).toBe(send.correlationId);
    expect(readChatSends(tabId)).toHaveLength(2);
    expect(deliveryState(readChatSends(tabId).find(item => item.id === send.id)!)).toBe("delivered");
  } finally {
    await act(async () => root.unmount()); host.remove();
    prepare.mockRestore(); store.mockRestore(); __setInvokeForTest(null); __setChatSendRepositoryForTest(null);
  }
});

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

it("a failed routed send offers no Retry and tells the owner how to resend it", async () => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  const send: DeliverySend = {
    id: "routed-failure", content: "opaque", sentAtMs: 1, correlationId: "routed",
    status: { send_id: "routed-failure", state: "failed", error: "launch_degraded_before_write", tab_id: "tab", ledger_id: "ledger",
      sequence: 1, retained_from: 1, correlationId: "routed", system_send: { content: "opaque", sent_at_ms: 1 } },
  };
  const message: NormalizedChatMessage = { id: send.id, content: send.content, type: "user" };
  const row: EnrichedChatEntry = { msg: message, item: { kind: "message", message }, origin: { kind: "system", context: "CLI" } };
  const retrySend = vi.fn();
  try {
    await act(async () => root.render(<ChatTranscriptRow entry={row} reports={[]} asks={[]} ariaLabel="Routed send" selected={false}
      joinNext={false} selecting={false} cursor={false} streaming={false} latestFailedToolCall={false} forkable={false} revertible={false}
      context={{ ...context, retrySend }} deliverySend={send} deliveryView="failed" />));
    expect(Array.from(host.querySelectorAll("button")).some(button => button.textContent === "Retry")).toBe(false);
    expect(host.querySelector('.ct-delivery[data-state="failed"] .ct-delivery__label:not(.sr-only)')?.textContent).toContain("domios send");
  } finally { await act(async () => root.unmount()); host.remove(); }
});

it("shows a refusal with an explicit Retry and removes it when delivery wins", async () => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  const send: DeliverySend = {
    id: "busy-row", content: "opaque", sentAtMs: 1, correlationId: "busy-correlation",
    draft: { text: "opaque", attachments: [], recordedVoiceAttachments: [], capsules: {}, reply: null },
    status: { send_id: "busy-request", state: "failed", error: "launch_ended_before_write", tab_id: "tab",
      ledger_id: "ledger", sequence: 1, retained_from: 1, correlationId: "busy-correlation" },
  };
  const message: NormalizedChatMessage = { id: send.id, content: send.content, type: "user" };
  const row: EnrichedChatEntry = { msg: message, item: { kind: "message", message }, origin: undefined };
  const retrySend = vi.fn();
  const render = (deliveryView: "failed" | "uncertain" | "delivered", deliverySend = send) =>
    <ChatTranscriptRow entry={row} reports={[]} asks={[]} ariaLabel="Own send" selected={false} joinNext={false}
      selecting={false} cursor={false} streaming={false} latestFailedToolCall={false} forkable={false} revertible={false}
      context={{ ...context, retrySend }} deliverySend={deliverySend} deliveryView={deliveryView} />;
  try {
    await act(async () => root.render(render("failed")));
    expect(deliveryState(send)).toBe("failed");
    expect(host.querySelector('[role="status"]')).not.toBeNull();
    expect(retrySend).not.toHaveBeenCalled();
    const retry = Array.from(host.querySelectorAll("button")).find(button => button.textContent === "Retry");
    expect(retry).toBeDefined();
    await act(async () => retry!.click());
    expect(retrySend).toHaveBeenCalledExactlyOnceWith(send.id);
    addChatSend("busy-retry", send);
    setChatSendDraft("busy-retry", send.id, send.draft!);
    observeDeliveryStatus({ ...send.status!, tab_id: "busy-retry" });
    const fresh = beginChatSendRetry("busy-retry", send.id);
    expect(fresh?.id).toBe(send.id);
    expect(fresh?.correlationId).not.toBe(send.correlationId);
    expect(readChatSends("busy-retry")).toHaveLength(1);
    expect(beginChatSendRetry("busy-retry", send.id)).toBeUndefined();
    await act(async () => root.render(render("uncertain", { ...send, status: { ...send.status!, state: "submit_unconfirmed", error: null } })));
    expect(host.textContent).not.toContain("Retry");
    await act(async () => root.render(render("delivered", { ...send, status: { ...send.status!, state: "delivered", error: null } })));
    expect(host.querySelector('[role="status"]')).toBeNull();
  } finally { await act(async () => root.unmount()); host.remove(); removeChatSend("busy-retry", send.id); }
});
