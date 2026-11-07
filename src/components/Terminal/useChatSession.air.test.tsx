import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, expect, test } from "vitest";
import { authority, owner, ownerId, rows } from "../../../tests/fixtures/chat/airTranscript";
import { __setInvokeForTest, __setListenForTest } from "../../lib/bridge";
import { __setChatSendRepositoryForTest, ensureChatSendTabHydrated, forgetChatSendsInMemory, readChatSends } from "./chatSendStore";
import { MemoryChatSendRepository } from "./chatSendRepository.testSupport";
import { clearTranscriptWindows } from "./transcriptStore";
import { useChatSession } from "./useChatSession";

let mounted: Root | null = null;
afterEach(async () => {
  await act(async () => mounted?.unmount());
  mounted = null;
  clearTranscriptWindows();
  forgetChatSendsInMemory();
  __setChatSendRepositoryForTest(null);
  __setInvokeForTest(null);
  __setListenForTest(null);
  document.body.replaceChildren();
});

test("air frontend remount retains one image send while its echo races the new snapshot", async () => {
  Object.defineProperty(globalThis, "IS_REACT_ACT_ENVIRONMENT", { configurable: true, value: true });
  const repository = new MemoryChatSendRepository();
  __setChatSendRepositoryForTest(repository);
  const handlers = new Map<string, Set<(event: { payload: unknown }) => void>>();
  __setListenForTest(async <T,>(event: string, handler: (event: { payload: T }) => void) => {
    const forward = (event: { payload: unknown }) => handler({ payload: event.payload as T });
    const listeners = handlers.get(event) ?? new Set();
    listeners.add(forward);
    handlers.set(event, listeners);
    return () => { listeners.delete(forward); };
  });
  const tabId = "air-remount";
  const prefix = rows.slice(0, rows.indexOf(owner));
  let response: ((value: unknown) => void) | null = null;
  let request: Record<string, unknown> = {};
  const invokeFixture = async (command: string, args?: Record<string, unknown>): Promise<unknown> => {
    if (command === "chat_start") {
      request = args ?? {};
      return new Promise<unknown>(resolve => { response = resolve; });
    }
    if (command === "chat_send_status" || command === "chat_send_dismissals") return [];
    if (command === "chat_resume") return { kind: "resumed", deltas: [], caught_up: true };
    return {};
  };
  __setInvokeForTest(async <T,>(command: string, args?: Record<string, unknown>) => await invokeFixture(command, args) as T);
  let view: ReturnType<typeof useChatSession> | null = null;
  function Harness() {
    const current = useChatSession({ tabId, agentSessionId: authority.sourceSessionId,
      sessionBindingNonce: authority.sessionBindingNonce, active: true });
    view = current;
    return createElement("div", null, current.enrichedMessages.map(({ msg }) =>
      createElement("article", { key: msg.id, "data-row-id": msg.sourceMessageId ?? msg.id }, msg.content)));
  }
  const mount = async () => {
    const host = document.createElement("div");
    document.body.append(host);
    mounted = createRoot(host);
    await act(async () => { mounted!.render(createElement(Harness)); });
    expect(response).not.toBeNull();
  };
  const completeSnapshot = async (messages: typeof rows, start: number, end: number) => {
    await act(async () => {
      response!({ kind: "snapshot", messages, text: "", start_offset: start, offset: end,
        offset_kind: "bytes", pending: false, source_session_id: authority.sourceSessionId,
        session_binding_nonce: authority.sessionBindingNonce, subscription_id: request.subscriptionId });
      response = null;
      await ensureChatSendTabHydrated(tabId);
    });
  };
  await mount();
  await completeSnapshot(prefix, 0, 100);
  const readView = () => view as unknown as ReturnType<typeof useChatSession>;
  const prefixAssistants = prefix.filter(row => row.type === "assistant").map(row => row.id);
  expect(readView().allMessages.filter(row => row.type === "assistant").map(row => row.id)).toEqual(prefixAssistants);
  await act(async () => { readView().addOptimistic(owner.content); });
  expect(readChatSends(tabId)[0].echoBoundary?.endOffset).toBe(100);
  await act(async () => { mounted!.unmount(); });
  mounted = null;
  clearTranscriptWindows();
  forgetChatSendsInMemory();
  await mount();
  await act(async () => {
    for (const handler of handlers.get(`tab-chat-msg-${tabId}`) ?? []) handler({ payload: {
      tab_id: tabId, source_session_id: authority.sourceSessionId, session_binding_nonce: authority.sessionBindingNonce,
      start_offset: 120, offset: 140, messages: [owner],
    } });
  });
  await completeSnapshot([], 110, 120);
  expect(readView().allMessages.filter(row => row.content === owner.content).map(row => row.id)).toEqual([ownerId]);
  expect(document.querySelectorAll(`[data-row-id="${ownerId}"]`)).toHaveLength(1);
  expect(readChatSends(tabId)[0].echo).toBeDefined();
});
