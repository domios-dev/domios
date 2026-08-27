import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import { afterEach, expect, finish, it, vi } from "../../test/standaloneTest";

interface StreamHarness {
  controller: ReadableStreamDefaultController<Uint8Array>;
  requests: string[];
}

interface MountedChat {
  host: HTMLDivElement;
  root: Root;
}

let bridge: typeof import("../../lib/bridge");
let webApi: typeof import("../../lib/webApi");
let webEvents: typeof import("../../lib/webEvents");
let transcriptStore: typeof import("./transcriptStore");
let useChatSession: typeof import("./useChatSession")["useChatSession"];

function installStream(): StreamHarness {
  const requests: string[] = [];
  let controller!: ReadableStreamDefaultController<Uint8Array>;
  vi.stubGlobal("fetch", async (input: RequestInfo | URL) => {
    requests.push(String(input));
    return new Response(new ReadableStream<Uint8Array>({
      start(next) {
        controller = next;
      },
    }));
  });
  return { get controller() { return controller; }, requests };
}

function emitChat(
  stream: StreamHarness,
  eventTabId: string,
  payloadTabId: string,
  content: string,
): void {
  const frame = {
    boot: "web-chat-boot",
    seq: 1,
    event_type: `tab-chat-msg-${eventTabId}`,
    data: {
      tab_id: payloadTabId,
      source_session_id: "session-1",
      session_binding_nonce: "binding-1",
      messages: [{ id: `${eventTabId}-message`, type: "assistant", content, timestamp: "1" }],
    },
  };
  stream.controller.enqueue(new TextEncoder().encode(`data: ${JSON.stringify(frame)}\n\n`));
}

async function mountChat(tabId: string): Promise<MountedChat> {
  bridge.__setInvokeForTest(async (command) => {
    if (command === "chat_start") {
      return {
        text: "",
        start_offset: 0,
        offset: 0,
        pending: false,
        messages: [],
        source_session_id: "session-1",
        session_binding_nonce: "binding-1",
        subscription_id: "subscription-1",
      } as never;
    }
    if (command === "chat_send_status") return [] as never;
    return {} as never;
  });

  function Harness() {
    const chat = useChatSession({
      tabId,
      agentSessionId: "session-1",
      sessionBindingNonce: "binding-1",
    });
    return <output aria-label="Conversation">{chat.enrichedMessages.map(({ msg }) => msg.content).join("|")}</output>;
  }

  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => {
    root.render(<Harness />);
    await Promise.resolve();
    await Promise.resolve();
  });
  return { host, root };
}

async function settleDelivery(): Promise<void> {
  await act(async () => {
    await Promise.resolve();
    await Promise.resolve();
  });
}

async function main(): Promise<void> {
  const chatWindow = new Window({ url: "http://codeterm.test/" });
  Object.defineProperty(globalThis, "IS_REACT_ACT_ENVIRONMENT", {
    configurable: true,
    value: true,
  });
  Object.defineProperty(globalThis, "window", { configurable: true, value: chatWindow });
  Object.defineProperty(globalThis, "document", { configurable: true, value: chatWindow.document });
  Object.defineProperty(globalThis, "navigator", { configurable: true, value: chatWindow.navigator });
  Object.defineProperty(globalThis, "localStorage", { configurable: true, value: chatWindow.localStorage });
  Object.defineProperty(globalThis, "sessionStorage", { configurable: true, value: chatWindow.sessionStorage });
  Object.defineProperty(globalThis, "HTMLElement", { configurable: true, value: chatWindow.HTMLElement });
  Object.defineProperty(globalThis, "Node", { configurable: true, value: chatWindow.Node });
  Object.defineProperty(globalThis, "Event", { configurable: true, value: chatWindow.Event });
  Object.defineProperty(globalThis, "CustomEvent", { configurable: true, value: chatWindow.CustomEvent });

  bridge = await import("../../lib/bridge");
  webApi = await import("../../lib/webApi");
  webEvents = await import("../../lib/webEvents");
  transcriptStore = await import("./transcriptStore");
  ({ useChatSession } = await import("./useChatSession"));

  afterEach(async () => {
    bridge.__setInvokeForTest(null);
    bridge.__setListenForTest(null);
    sessionStorage.clear();
    webEvents.__resetWebEventsForTest();
    transcriptStore.clearTranscriptWindows();
    webApi.clearRelayTarget();
    localStorage.clear();
    document.body.replaceChildren();
    vi.unstubAllGlobals();
  });

  it("renders a local-tab message delivered by the web SSE listen branch", async () => {
    webApi.setTokenFor(webApi.selfHost(), "local-token");
    const stream = installStream();
    const mounted = await mountChat("local-tab");

    expect(stream.requests[0]).toBe("http://codeterm.test/api/events/stream");
    emitChat(stream, "local-tab", "local-tab", "local web arrival");
    await settleDelivery();

    expect(mounted.host.textContent).toContain("local web arrival");
    await act(async () => mounted.root.unmount());
  });

  it("renders a peer-tab message through the web mesh-dropdown relay stream", async () => {
    webApi.setTokenFor(webApi.selfHost(), "local-token");
    webApi.setRelayTarget("peer-a", "Peer A");
    const stream = installStream();
    const mounted = await mountChat("peer-local-tab");

    expect(stream.requests[0]).toContain("/api/mesh/relay-stream?peer=peer-a&path=%2Fapi%2Fevents%2Fstream");
    emitChat(stream, "peer-local-tab", "peer-local-tab", "relay web arrival");
    await settleDelivery();

    expect(mounted.host.textContent).toContain("relay web arrival");
    await act(async () => mounted.root.unmount());
  });

  it("renders an attached composite-tab message through subscribeRemoteTabEvent", async () => {
    webApi.setTokenFor(webApi.selfHost(), "local-token");
    const stream = installStream();
    const mounted = await mountChat("mesh:peer-b/remote-tab");

    expect(stream.requests[0]).toContain("peer=peer-b");
    expect(stream.requests[0]).toContain("tab=mesh%3Apeer-b%2Fremote-tab");
    emitChat(stream, "remote-tab", "remote-tab", "attached mesh arrival");
    await settleDelivery();

    expect(mounted.host.textContent).toContain("attached mesh arrival");
    await act(async () => mounted.root.unmount());
  });

  it("keeps a local desktop listener on the Tauri branch", () => {
    expect(bridge.localListenBranch(false)).toBe("tauri");
    expect(bridge.localListenBranch(true)).toBe("web");
  });

  finish();
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
