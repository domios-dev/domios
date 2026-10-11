import assert from "node:assert/strict";
import test from "node:test";
import { act, createElement, useRef } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import type { Terminal } from "@xterm/xterm";
import type { ExtendedState } from "../../store/multiplexer/tree";

const peerId = "928b06dd-0631-43e2-9252-3c6573b54b8d";
const tabIds = ["0885347b", "183546de", "68f196b8", "e5de9ad9"];

test("mounted bare relay terminals open one composite stream each despite the local killswitch", async () => {
  const browser = new Window({ url: "http://gateway.test:7697" });
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  for (const [key, value] of Object.entries({
    window: browser, document: browser.document, navigator: browser.navigator,
    localStorage: browser.localStorage, sessionStorage: browser.sessionStorage,
    Event: browser.Event, CustomEvent: browser.CustomEvent,
  })) Object.defineProperty(globalThis, key, { configurable: true, value });
  const api = await import("../../lib/webApi");
  const output = await import("../../lib/outputStream");
  const { usePtyConnection } = await import("./usePtyConnection");
  const { MuxStateStoreContext } = await import("../../store/multiplexer/context");
  const { createMuxStateStore } = await import("../../store/multiplexer/selectorStore");
  const requests: URL[] = [];
  const controllers = new Map<string, ReadableStreamDefaultController<Uint8Array>>();
  const writes = new Map<string, string[]>();
  const ready = new Map<string, boolean>();
  const previousFetch = globalThis.fetch;
  api.setTokenFor(api.apiHost(), "fixture-token");
  api.setRelayTarget(peerId, "Windows");
  browser.localStorage.setItem("codeterm-web-output-stream", "off");
  globalThis.fetch = async (input) => {
    const url = new URL(String(input));
    requests.push(url);
    const tabId = url.searchParams.get("tab")?.split("/")[1];
    assert(tabId && tabIds.includes(tabId));
    return new Response(new ReadableStream<Uint8Array>({
      start(controller) { controllers.set(tabId, controller); },
    }));
  };
  const store = createMuxStateStore(() => ({ tabs: {} }) as unknown as ExtendedState);
  function Probe({ tabId }: { tabId: string }) {
    const termRef = useRef({
      buffer: { active: { type: "normal", viewportY: 0, baseY: 0 } },
      scrollToBottom() {}, scrollToLine() {},
      write(data: string | Uint8Array, done?: () => void) {
        const text = typeof data === "string" ? data : new TextDecoder().decode(data);
        if (text) writes.get(tabId)!.push(text);
        done?.();
      },
    } as unknown as Terminal);
    const connectedPtyRef = useRef<string | null>(null);
    usePtyConnection({
      termRef, connectedPtyRef, tabId, ptyId: `pty-${tabId}`, termReady: true,
      onShellReady() {}, onOutputChunk() {},
      onConnectionStateChange(value) { ready.set(tabId, value); },
    });
    return null;
  }
  const host = browser.document.createElement("div");
  const root = createRoot(host as unknown as Element);
  try {
    for (const tabId of tabIds) writes.set(tabId, []);
    await act(async () => root.render(createElement(MuxStateStoreContext.Provider, { value: store },
      tabIds.map((tabId) => createElement(Probe, { key: tabId, tabId })))));
    assert.equal(requests.length, tabIds.length);
    for (const tabId of tabIds) {
      const urls = requests.filter((url) => url.searchParams.get("tab") === `mesh:${peerId}/${tabId}`);
      assert.equal(urls.length, 1);
      assert.equal(urls[0].pathname, "/api/mesh/relay-stream");
      assert.equal(urls[0].searchParams.get("peer"), peerId);
      assert.equal(urls[0].searchParams.get("path"), "/api/output/stream");
      assert.equal(urls[0].searchParams.has("since"), false);
      assert.equal(ready.get(tabId), false);
    }
    for (const [text, offset] of [["history", 7], ["live", 11]] as const) {
      await act(async () => {
        for (const tabId of tabIds) controllers.get(tabId)!.enqueue(new TextEncoder().encode(
          `data: ${JSON.stringify({ tab_id: tabId, boot: "boot", output: text, offset })}\n\n`,
        ));
      });
    }
    for (const tabId of tabIds) {
      assert.deepEqual(writes.get(tabId), ["history", "live"]);
      assert.equal(ready.get(tabId), true);
    }
    output.resetOutputStream();
    assert.equal(requests.length, tabIds.length, "relay mode never opens a local aggregate");
  } finally {
    await act(async () => root.unmount());
    globalThis.fetch = previousFetch;
    api.clearRelayTarget();
    await browser.happyDOM.close();
  }
});
