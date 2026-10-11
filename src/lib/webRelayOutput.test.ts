import assert from "node:assert/strict";
import { finish, it, vi } from "../test/standaloneTest";

it("both shells always use output streams regardless of the obsolete killswitch", async () => {
  stubBrowser();
  const { webOutputTransportActive } = await import("./outputStream");
  try {
    for (const enabled of [false, true]) {
      assert.equal(webOutputTransportActive(true, "928b06dd-0631-43e2-9252-3c6573b54b8d", enabled), true);
      assert.equal(webOutputTransportActive(true, null, enabled), true);
      assert.equal(webOutputTransportActive(false, null, enabled), true);
    }
  } finally {
    vi.unstubAllGlobals();
  }
});

function stubBrowser(): void {
  const values = new Map<string, string>();
  const storage = {
    getItem: (key: string) => values.get(key) ?? null,
    setItem: (key: string, value: string) => values.set(key, value),
    removeItem: (key: string) => values.delete(key),
  };
  const browser = Object.assign(new EventTarget(), {
    location: { protocol: "http:", host: "gateway.test:7685" },
  });
  vi.stubGlobal("window", browser);
  vi.stubGlobal("localStorage", storage);
  vi.stubGlobal("sessionStorage", storage);
  storage.setItem("codeterm-web-token:http://gateway.test:7685", "fixture-token");
}

it("whole-peer Web terminals use tab-scoped gateway streams without cross-peer output", async () => {
  stubBrowser();
  const api = await import("./webApi");
  const output = await import("./outputStream");
  api.setTokenFor("http://gateway.test:7685", "opaque-local-token");
  const requests: URL[] = [];
  vi.stubGlobal("fetch", vi.fn(async (...args: unknown[]) => {
    const url = new URL(String(args[0]));
    requests.push(url);
    assert.equal(url.origin, "http://gateway.test:7685");
    assert.equal(url.searchParams.get("path"), "/api/output/stream");
    assert.equal((args[1] as RequestInit).headers &&
      new Headers((args[1] as RequestInit).headers).get("Authorization"), "Bearer opaque-local-token");
    return new Response(new ReadableStream({ start(controller) {
      const encode = (body: object) => new TextEncoder().encode(`data: ${JSON.stringify(body)}\n\n`);
      controller.enqueue(encode({ tab_id: "foreign-tab", output: "not-owned", offset: 99 }));
      controller.enqueue(encode({ tab_id: "same-tab", output: "owned", offset: 5, boot: "boot" }));
    } }));
  }));
  const peerA = "928b06dd-1234-4567-890a-123456789abc";
  const peerB = "827b06dd-2345-5678-901b-234567890bcd";
  const peerC = "726b06dd-3456-6789-012c-345678901cde";
  const unsubscribers: Array<() => void> = [];
  try {
    for (const [selected, tabId, expectedPeer] of [
      [peerA, "same-tab", peerA],
      [peerB, "same-tab", peerB],
      [peerB, `mesh:${peerC}/same-tab`, peerC],
    ]) {
      api.setRelayTarget(selected, selected);
      const received: string[] = [];
      let resolve!: () => void;
      const delivered = new Promise<void>((done) => { resolve = done; });
      unsubscribers.push(output.subscribeTabOutput(tabId, {
        onOutput(value) { received.push(value); resolve(); }, onTruncated() {}, onEnded() {},
      }));
      assert.equal(requests[requests.length - 1]?.searchParams.get("tab"), `mesh:${expectedPeer}/same-tab`);
      assert.equal(requests[requests.length - 1]?.searchParams.get("peer"), expectedPeer);
      assert.equal(requests[requests.length - 1]?.searchParams.has("since"), false);
      await delivered;
      assert.deepEqual(received, ["owned"]);
    }
  } finally {
    for (const unsubscribe of unsubscribers) unsubscribe();
    output.resetOutputStream();
    vi.unstubAllGlobals();
  }
});

it("HTTP 400 reaches the terminal error sink and failed output channel without busy retry", async () => {
  stubBrowser();
  const output = await import("./outputStream");
  const { onRemoteTransportState } = await import("./remoteTabTransportState");
  const failures: Error[] = [];
  const states: string[] = [];
  let requests = 0;
  vi.stubGlobal("fetch", async () => {
    requests += 1;
    return new Response(null, { status: 400 });
  });
  const tabId = "mesh:928b06dd-1234-4567-890a-123456789abc/refused";
  const stopState = onRemoteTransportState((state) => {
    if (state.ordinaryTabId === tabId && state.channel === "output") states.push(state.availability);
  });
  const unsubscribe = output.subscribeTabOutput(tabId, {
    onOutput() { throw new Error("Refused stream must not deliver output"); },
    onTruncated() {}, onEnded() {},
    onStreamError(error) { failures.push(error); },
  });
  try {
    await output.waitForRemoteOutputGenerationSettlement(tabId, output.remoteOutputSubscriptionGeneration(tabId));
    assert.equal(failures.length, 1);
    assert.match(failures[0].message, /HTTP 400/);
    assert.deepEqual(states, ["unavailable"]);
    assert.equal(requests, 1);
    assert.equal(output.remoteOutputSubscriptionSnapshot(tabId).hasController, false);
  } finally {
    unsubscribe();
    stopState();
    vi.unstubAllGlobals();
  }
});

it("peer switch aborts an aggregate opened on self before opening composite relay output", async () => {
  stubBrowser();
  const api = await import("./webApi");
  const output = await import("./outputStream");
  api.clearRelayTarget();
  const requests: Array<{ url: URL; signal: AbortSignal }> = [];
  let opened!: () => void;
  const localOpened = new Promise<void>((resolve) => { opened = resolve; });
  vi.stubGlobal("fetch", async (input: unknown, init: RequestInit) => {
    const url = new URL(String(input));
    const signal = init.signal!;
    requests.push({ url, signal });
    const response = new Response(new ReadableStream({ start(controller) {
      signal.addEventListener("abort", () => controller.error(new DOMException("Stopped", "AbortError")), { once: true });
    } }));
    if (url.pathname === "/api/output/stream") opened();
    return response;
  });
  const sink = { onOutput() {}, onTruncated() {}, onEnded() {} };
  const stopLocal = output.subscribeTabOutput("local-tab", sink);
  let stopRemote: (() => void) | undefined;
  try {
    await localOpened;
    stopLocal();
    const peerId = "928b06dd-0631-43e2-9252-3c6573b54b8d";
    api.setRelayTarget(peerId, "Windows");
    output.resetOutputStream();
    assert.equal(requests[0].signal.aborted, true);
    stopRemote = output.subscribeTabOutput("183546de", sink);
    await Promise.resolve();
    assert.equal(requests.length, 2);
    assert.equal(requests[1].url.pathname, "/api/mesh/relay-stream");
    assert.equal(requests[1].url.searchParams.get("tab"), `mesh:${peerId}/183546de`);
    assert.equal(requests[1].signal.aborted, false);
    assert.equal(requests.filter((request) => !request.signal.aborted).length, 1);
  } finally {
    stopLocal();
    stopRemote?.();
    output.resetOutputStream();
    api.clearRelayTarget();
    vi.unstubAllGlobals();
  }
});

finish();
