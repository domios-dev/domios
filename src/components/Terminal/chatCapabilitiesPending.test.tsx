import assert from "node:assert/strict";
import { test } from "node:test";
import { act, createElement } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import { __setInvokeForTest, __setListenForTest } from "../../lib/bridge";
import { setAgentDescriptors } from "../../lib/agentDescriptorCache";
import { tabChatCapable } from "../../lib/chatUiMode";
import { invalidateAgentProviders, useAgentProviders } from "../../hooks/useAgentProviders";
import { chatBodyView } from "./chatBodyVisibility";
import { clearTranscriptWindows } from "./transcriptStore";
import { useChatSession } from "./useChatSession";

test("an unknown capability descriptor leaves an attached transcript visible", () => {
  setAgentDescriptors([]);
  assert.equal(tabChatCapable({ agent_type: "fixture-provider" }), true);
  assert.deepEqual(chatBodyView({ snapshotReady: true, hasTranscript: true, chatLoading: false,
    unavailable: null, bootView: { kind: "none" } }), { kind: "none" });
});

test("a pending provider catalogue does not delay chat attachment or rendered rows", async () => {
  const browser = new Window();
  Object.defineProperties(globalThis, {
    window: { configurable: true, value: browser }, document: { configurable: true, value: browser.document },
    navigator: { configurable: true, value: browser.navigator }, IS_REACT_ACT_ENVIRONMENT: { configurable: true, value: true },
  });
  let catalogRequests = 0;
  let chatRequests = 0;
  __setListenForTest(async () => () => {});
  __setInvokeForTest((async (command: string, args?: Record<string, unknown>) => {
    if (command === "list_agent_providers") { catalogRequests++; return new Promise(() => {}); }
    if (command === "chat_start") {
      chatRequests++;
      return { text: "", pending: false, start_offset: 0, offset: 100,
        source_session_id: "session", session_binding_nonce: "nonce", subscription_id: args?.subscriptionId,
        messages: [{ id: "owned-row", type: "assistant", content: "visible-content" }] };
    }
    return [];
  }) as Parameters<typeof __setInvokeForTest>[0]);
  function Harness() {
    const catalogue = useAgentProviders();
    const chat = useChatSession({ tabId: "capability-pending", agentSessionId: "session", sessionBindingNonce: "nonce" });
    const view = chatBodyView({ snapshotReady: chat.snapshotReady, hasTranscript: chat.allMessages.length > 0,
      chatLoading: chat.loading, unavailable: chat.transcriptUnavailable, bootView: { kind: "none" } });
    return createElement("output", { "data-catalogue-loading": catalogue.loading },
      view.kind === "none" ? chat.enrichedMessages.map(row => row.msg.content).join(",") : "");
  }
  const host = document.createElement("main");
  const root = createRoot(host);
  try {
    await act(async () => root.render(createElement(Harness)));
    assert.equal(catalogRequests, 1);
    assert.equal(chatRequests, 1);
    assert.equal(host.firstElementChild?.getAttribute("data-catalogue-loading"), "true");
    assert.equal(host.textContent, "visible-content");
  } finally {
    await act(async () => root.unmount());
    clearTranscriptWindows();
    invalidateAgentProviders();
    __setInvokeForTest(null);
    __setListenForTest(null);
    await browser.happyDOM.abort();
  }
});
