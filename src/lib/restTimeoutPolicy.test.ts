import { describe, expect, it, vi } from "vitest";
import { REST_READ_TIMEOUT_MS, REST_WRITE_TIMEOUT_MS, isLongPollRequest, restRequestTimeoutMs, withRestRequestSignal } from "./restTimeoutPolicy";
import { createTransportClient } from "./transportClient";

describe("REST timeout policy", () => {
  it("bounds reads and writes, leaves long-polls open, and lets an explicit caller value win", () => {
    expect(restRequestTimeoutMs("GET", "/api/mesh/peers")).toBe(REST_READ_TIMEOUT_MS);
    expect(restRequestTimeoutMs("head", "/api/health")).toBe(REST_READ_TIMEOUT_MS);
    expect(restRequestTimeoutMs("POST", "/api/tabs/t/chat-send")).toBe(REST_WRITE_TIMEOUT_MS);
    expect(restRequestTimeoutMs("POST", "/api/mesh/relay-to")).toBe(REST_WRITE_TIMEOUT_MS);
    expect(REST_WRITE_TIMEOUT_MS).toBeGreaterThan(120_000);
    expect(restRequestTimeoutMs("GET", "/api/tabs/t/agent/switch-provider/op?wait=true")).toBeNull();
    expect(restRequestTimeoutMs("GET", "/api/tabs/t/agent/switch-provider/op?wait=false")).toBe(REST_READ_TIMEOUT_MS);
    expect(restRequestTimeoutMs("GET", "/api/pty/p/read")).toBeNull();
    expect(restRequestTimeoutMs("GET", "/api/mesh/peers", 5)).toBe(5);
    expect(restRequestTimeoutMs("POST", "/api/tabs/t/chat-send", null)).toBeNull();
    expect(isLongPollRequest("/api/tabs/t/pty/read?offset=4")).toBe(true);
    expect(isLongPollRequest("/api/tabs/t/output")).toBe(false);
  });

  it("combines the caller's signal with the deadline", async () => {
    const caller = new AbortController();
    await withRestRequestSignal("GET", "/api/mux/state", { signal: caller.signal }, async (signal) => {
      expect(signal).not.toBe(caller.signal);
      caller.abort("left");
      expect(signal?.aborted).toBe(true);
      expect(signal?.reason).toBe("left");
    });
    await withRestRequestSignal("GET", "/api/pty/p/read", { signal: caller.signal }, async (signal) => expect(signal).toBe(caller.signal));
    await withRestRequestSignal("GET", "/api/pty/p/read", undefined, async (signal) => expect(signal).toBeUndefined());
  });

  it("leaves no pending timer or caller listener after many settled, failed and aborted calls", async () => {
    vi.useFakeTimers();
    try {
      const attached = new Set<unknown>();
      const countedSignal = () => {
        const controller = new AbortController();
        const { signal } = controller;
        const add = signal.addEventListener.bind(signal);
        const remove = signal.removeEventListener.bind(signal);
        signal.addEventListener = ((type: string, listener: EventListenerOrEventListenerObject, options?: AddEventListenerOptions | boolean) => {
          attached.add(listener);
          add(type, listener, options);
          if (typeof options === "object" && options.once) add(type, () => attached.delete(listener), { once: true });
        }) as AbortSignal["addEventListener"];
        signal.removeEventListener = ((type: string, listener: EventListenerOrEventListenerObject) => {
          attached.delete(listener);
          remove(type, listener);
        }) as AbortSignal["removeEventListener"];
        return controller;
      };
      const pending: Array<() => void> = [];
      let n = 0;
      const fetch = vi.fn(async (_input: RequestInfo | URL, init?: RequestInit) => {
        const kind = n++ % 3;
        if (kind === 1) throw new TypeError("network down");
        if (kind === 2) return new Promise<Response>((_resolve, reject) => {
          if (init?.signal?.aborted) reject(init.signal.reason);
          init?.signal?.addEventListener("abort", () => reject(init.signal?.reason), { once: true });
        });
        return Response.json({ ok: true });
      });
      const client = createTransportClient({ origin: "http://127.0.0.1:7685", token: "fixture", fetch });
      const calls = Array.from({ length: 300 }, (_, i) => {
        const caller = countedSignal();
        if (i % 3 === 2) pending.push(() => caller.abort("left"));
        const opts = { signal: caller.signal };
        return i % 2 ? client.json("GET", "/api/mux/state", undefined, opts) : client.localJson("POST", "/api/workspaces/w/focus", {}, opts);
      });
      const settled = Promise.allSettled(calls);
      await vi.waitFor(() => expect(fetch).toHaveBeenCalledTimes(300));
      await vi.waitFor(() => expect(vi.getTimerCount()).toBe(100));
      for (const abort of pending) abort();
      const results = await settled;
      expect(results.filter((r) => r.status === "fulfilled")).toHaveLength(100);
      expect(results.filter((r) => r.status === "rejected" && r.reason === "left")).toHaveLength(100);
      expect(vi.getTimerCount()).toBe(0);
      expect(attached.size).toBe(0);
    } finally {
      vi.useRealTimers();
    }
  });

  it("gives every json() and localJson() call a deadline unless it is a long-poll or opted out", async () => {
    const signals = new Map<string, AbortSignal | null | undefined>();
    const fetch = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      signals.set(new URL(String(input)).pathname, init?.signal);
      return Response.json({ ok: true });
    });
    const client = createTransportClient({ origin: "http://127.0.0.1:7685", token: "fixture", fetch });
    await client.json("GET", "/api/mesh/peers");
    await client.localJson("POST", "/api/workspaces/w/focus");
    await client.json("GET", "/api/pty/p/read");
    await client.json("POST", "/api/tabs/t/chat-send", {}, { timeoutMs: null });
    expect(signals.get("/api/mesh/peers")).toBeInstanceOf(AbortSignal);
    expect(signals.get("/api/workspaces/w/focus")).toBeInstanceOf(AbortSignal);
    expect(signals.get("/api/pty/p/read")).toBeUndefined();
    expect(signals.get("/api/tabs/t/chat-send")).toBeUndefined();
  });

  it("rejects a stalled call when its deadline fires", async () => {
    const fetch = vi.fn((_input: RequestInfo | URL, init?: RequestInit) => new Promise<Response>((_resolve, reject) => {
      init?.signal?.addEventListener("abort", () => reject(init.signal?.reason), { once: true });
    }));
    const client = createTransportClient({ origin: "http://127.0.0.1:7685", token: "fixture", fetch });
    await expect(client.json("GET", "/api/mesh/peers", undefined, { timeoutMs: 1 })).rejects.toMatchObject({ name: "TimeoutError" });
  });
});
