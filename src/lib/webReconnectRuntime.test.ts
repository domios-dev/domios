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
    if (delay < 30_000) pendingWait?.();
    return id;
  }) as unknown as typeof setTimeout;
  globalThis.clearTimeout = ((id: number) => deadlines.delete(id)) as unknown as typeof clearTimeout;
  const streams: ReadableStreamDefaultController<Uint8Array>[] = [];
  let requestArrived: (() => void) | undefined;
  const requested = () => new Promise<void>((resolve) => { requestArrived = resolve; });
  const probes: Array<(response: Response) => void> = [];
  globalThis.fetch = async (_input, options) => {
    if (options?.method === "HEAD") {
      const response = new Promise<Response>((resolve) => probes.push(resolve));
      requestArrived?.();
      return response;
    }
    const stream = new ReadableStream<Uint8Array>({
      start(controller) {
        streams.push(controller);
        options?.signal?.addEventListener("abort", () => {
          try { controller.error(new Error("aborted")); } catch { /* already closed */ }
        }, { once: true });
      },
    });
    requestArrived?.();
    return new Response(stream, { status: 200 });
  };
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
    const probe = requested();
    browser.dispatchEvent(new Event("online"));
    await probe;
    assert.equal(streams.length, 1);
    assert.equal(reconnects, 0);
    browser.dispatchEvent(new Event("online"));
    assert.equal(probes.length, 1);
    const connection = requested();
    probes[0](new Response(null, { status: 200 }));
    await connection;
    delivery = received();
    streams[1].enqueue(frame);
    await delivery;
    assert.equal(reconnects, 1);
    documentTarget.hidden = true;
    documentTarget.dispatchEvent(new Event("visibilitychange"));
    documentTarget.hidden = false;
    documentTarget.dispatchEvent(new Event("visibilitychange"));
    assert.equal(streams.length, 2);
    const secondFailure = waiting();
    streams[1].error(new Error("server restarted"));
    await secondFailure;
    const visibleProbe = requested();
    documentTarget.hidden = true;
    documentTarget.dispatchEvent(new Event("visibilitychange"));
    documentTarget.hidden = false;
    documentTarget.dispatchEvent(new Event("visibilitychange"));
    await visibleProbe;
    assert.equal(streams.length, 2);
    assert.equal(reconnects, 1);
    const visibleConnection = requested();
    probes[1](new Response(null, { status: 200 }));
    await visibleConnection;
    delivery = received();
    streams[2].enqueue(frame);
    await delivery;
    assert.equal(reconnects, 2);
    const finalFailure = waiting();
    streams[2].error(new Error("offline again"));
    await finalFailure;
    const deadlineProbe = requested();
    const retryDeadline = [...deadlines.values()].find((entry) => entry.delay < 30_000);
    assert.ok(retryDeadline);
    retryDeadline.callback();
    await deadlineProbe;
    assert.equal(streams.length, 3);
    assert.equal(reconnects, 2);
    const rejectedProbe = waiting();
    probes[2](new Response(null, { status: 503 }));
    await rejectedProbe;
    assert.equal(reconnects, 2);
    events.__resetWebEventsForTest();
    assert.equal(deadlines.size, 0);
    browser.dispatchEvent(new Event("online"));
    assert.equal(streams.length, 3);
    console.log("16 passed, 0 failed");
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
