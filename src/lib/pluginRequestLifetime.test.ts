import { afterEach, expect, it, vi } from "vitest";
import { createTransportClient } from "./transportClient";
import { createCapabilitiesTransport } from "./transport/capabilities";

vi.mock("./bridge", () => ({ invoke: vi.fn() }));
import { invoke } from "./bridge";
import { ensurePluginSnapshot, __resetPluginSnapshotStoreForTest } from "./pluginSnapshotStore";
import { subscribePluginBubble, __resetPluginBubbleCacheForTest } from "./pluginBubbleCache";

afterEach(() => {
  __resetPluginSnapshotStoreForTest();
  __resetPluginBubbleCacheForTest();
  vi.restoreAllMocks();
  vi.useRealTimers();
});

it("360 switches leave zero pending snapshot/bubble requests, timers or caller listeners", async () => {
  vi.useFakeTimers();
  const listeners = new Set<unknown>();
  const add = AbortSignal.prototype.addEventListener;
  const remove = AbortSignal.prototype.removeEventListener;
  vi.spyOn(AbortSignal.prototype, "addEventListener").mockImplementation(function (this: AbortSignal, type, listener, options) {
    listeners.add(listener);
    add.call(this, type, listener, options);
    if (typeof options === "object" && options.once) add.call(this, type, () => listeners.delete(listener), { once: true });
  });
  vi.spyOn(AbortSignal.prototype, "removeEventListener").mockImplementation(function (this: AbortSignal, type, listener, options) {
    listeners.delete(listener);
    remove.call(this, type, listener, options);
  });
  let pending = 0;
  let requests = 0;
  let bothStarted = () => {};
  const inFlight: Promise<unknown>[] = [];
  const fetch = vi.fn((_input: RequestInfo | URL, init?: RequestInit) => new Promise<Response>((_, reject) => {
    requests++;
    pending++;
    if (pending === 2) bothStarted();
    const abort = () => { pending--; reject(init?.signal?.reason); };
    if (init?.signal?.aborted) abort();
    else init?.signal?.addEventListener("abort", abort, { once: true });
  }));
  const transport = createCapabilitiesTransport(createTransportClient({ origin: "http://fixture", token: "fixture", fetch }));
  vi.mocked(invoke).mockImplementation(<T>(command: string, args?: Record<string, unknown>) => {
    const handler = transport[command as keyof typeof transport];
    if (!handler) throw new Error(command);
    const request = handler(args ?? {});
    inFlight.push(request);
    return request as Promise<T>;
  });
  for (let i = 0; i < 360; i++) {
    const started = new Promise<void>(resolve => { bothStarted = resolve; });
    const controller = new AbortController();
    const a = ensurePluginSnapshot({ force: true, signal: controller.signal });
    const b = ensurePluginSnapshot({ force: true, signal: controller.signal });
    const settled = Promise.allSettled([a, b]);
    const bubble = subscribePluginBubble("fixture", `/repo/${i}`, `tab-${i}`, true, () => {});
    await started;
    expect(pending).toBe(2);
    controller.abort();
    bubble.setActive(false);
    bubble.unsubscribe();
    await settled;
    await Promise.allSettled(inFlight.splice(0));
    expect(pending).toBe(0);
  }
  expect(requests).toBe(720);
  expect(vi.getTimerCount()).toBe(0);
  expect(listeners.size).toBe(0);
});
