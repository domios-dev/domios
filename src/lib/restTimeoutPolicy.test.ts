import { describe, expect, it, vi } from "vitest";
import { REST_READ_TIMEOUT_MS, REST_WRITE_TIMEOUT_MS, isLongPollRequest, restRequestSignal, restRequestTimeoutMs } from "./restTimeoutPolicy";
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

  it("combines the caller's signal with the deadline", () => {
    const caller = new AbortController();
    const signal = restRequestSignal("GET", "/api/mux/state", { signal: caller.signal });
    expect(signal).not.toBe(caller.signal);
    caller.abort("left");
    expect(signal?.aborted).toBe(true);
    expect(restRequestSignal("GET", "/api/pty/p/read", { signal: caller.signal })).toBe(caller.signal);
    expect(restRequestSignal("GET", "/api/pty/p/read")).toBeUndefined();
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
