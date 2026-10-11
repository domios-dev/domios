import assert from "node:assert/strict";

class MemoryStorage {
  private values = new Map<string, string>();
  getItem(key: string): string | null { return this.values.get(key) ?? null; }
  setItem(key: string, value: string): void { this.values.set(key, value); }
  removeItem(key: string): void { this.values.delete(key); }
}

async function main(): Promise<void> {
  const originalFetch = globalThis.fetch;
  const originalSetTimeout = globalThis.setTimeout;
  const originalClearTimeout = globalThis.clearTimeout;
  const storage = new MemoryStorage();
  const browser = Object.assign(new EventTarget(), {
    location: { protocol: "http:", host: "127.0.0.1:7686" },
    localStorage: storage,
    sessionStorage: storage,
  });
  const documentTarget = Object.assign(new EventTarget(), { hidden: false });
  Object.assign(globalThis, { window: browser, document: documentTarget, localStorage: storage, sessionStorage: storage });
  storage.setItem("codeterm-capability:web-reconnect-probe", "true");
  let nextTimer = 0;
  const deadlines = new Map<number, { callback: () => void; delay: number }>();
  let pendingWait: (() => void) | undefined;
  const waiting = () => new Promise<void>((resolve) => { pendingWait = resolve; });
  globalThis.setTimeout = ((callback: () => void, delay: number) => {
    const id = ++nextTimer;
    deadlines.set(id, { callback, delay });
    return id;
  }) as unknown as typeof setTimeout;
  globalThis.clearTimeout = ((id: number) => deadlines.delete(id)) as unknown as typeof clearTimeout;
  const streams: ReadableStreamDefaultController<Uint8Array>[] = [];
  let requestArrived: (() => void) | undefined;
  const requested = () => new Promise<void>((resolve) => { requestArrived = resolve; });
  browser.addEventListener("codeterm-event-stream-disconnected", () => pendingWait?.());
  let refusal = false;
  globalThis.fetch = async (_input, options) => {
    assert.notEqual(options?.method, "HEAD");
    assert.equal(new Headers(options?.headers).get("Authorization"), "Bearer fixture-token");
    if (refusal) { requestArrived?.(); return new Response(null, { status: 503 }); }
    const stream = new ReadableStream<Uint8Array>({
      start(controller) { streams.push(controller); },
    });
    requestArrived?.();
    return new Response(stream, { status: 200 });
  };
  const api = await import("./webApi");
  api.setTokenFor(api.apiHost(), "fixture-token");
  const events = await import("./webEvents");
  storage.removeItem("codeterm-capability:web-reconnect-probe");
  assert.equal(events.webReconnectEnabled(), false);
  storage.setItem("codeterm-capability:web-reconnect-probe", "true");
  assert.equal(events.webReconnectEnabled(), true);
  const lifecycle = await import("./lifecycle");
  const { TERMINAL_VIEWER_RECONNECTED_EVENT } = await import("./terminalViewerEvents");
  let reconnects = 0;
  browser.addEventListener(TERMINAL_VIEWER_RECONNECTED_EVENT, () => reconnects++);
  let frameArrived: (() => void) | undefined;
  const received = () => new Promise<void>((resolve) => { frameArrived = resolve; });
  const unlisten = events.webListen("fixture-frame", () => frameArrived?.());
  const stop = lifecycle.mountWebLifecycle();
  const frame = new TextEncoder().encode('data: {"event_type":"fixture-frame","data":null}\n\n');
  try {
    let delivery = received();
    streams[0].enqueue(frame);
    await delivery;
    const failure = waiting();
    streams[0].error(new Error("offline"));
    await failure;
    const connection = requested();
    browser.dispatchEvent(new Event("online"));
    await connection;
    assert.equal(reconnects, 0, "opening HTTP is not readiness");
    delivery = received();
    streams[1].enqueue(frame);
    await delivery;
    assert.equal(reconnects, 1);
    const secondFailure = waiting();
    streams[1].error(new Error("server restarted"));
    await secondFailure;
    const visibleConnection = requested();
    documentTarget.hidden = true;
    documentTarget.dispatchEvent(new Event("visibilitychange"));
    documentTarget.hidden = false;
    documentTarget.dispatchEvent(new Event("visibilitychange"));
    await visibleConnection;
    assert.equal(reconnects, 1);
    delivery = received();
    streams[2].enqueue(frame);
    await delivery;
    assert.equal(reconnects, 2);
    const finalFailure = waiting();
    streams[2].error(new Error("offline again"));
    await finalFailure;
    assert.equal([...deadlines.values()].some((entry) => entry.delay < 30_000), false);
    refusal = true;
    const rejected = waiting();
    browser.dispatchEvent(new Event("online"));
    await rejected;
    assert.equal(reconnects, 2);
    refusal = false;
    const eofReady = requested();
    browser.dispatchEvent(new Event("online"));
    await eofReady;
    const eofConnection = requested();
    streams[3].close();
    await eofConnection;
    assert.equal(streams.length, 5, "clean EOF reconnects without a timer or a frame");
    events.__resetWebEventsForTest();
    assert.equal(deadlines.size, 0);
    browser.dispatchEvent(new Event("online"));
    assert.equal(streams.length, 5);
    console.log("Reconnect runtime event barriers passed");
  } finally {
    stop();
    unlisten();
    events.__resetWebEventsForTest();
    globalThis.fetch = originalFetch;
    globalThis.setTimeout = originalSetTimeout;
    globalThis.clearTimeout = originalClearTimeout;
  }
}

let completed = false;
process.once("beforeExit", () => {
  if (!completed && !process.exitCode) {
    console.error("Reconnect runtime test ended before its event barriers completed");
    process.exitCode = 1;
  }
});
void main().then(() => { completed = true; }).catch((error: unknown) => {
  console.error(error);
  process.exitCode = 1;
});
