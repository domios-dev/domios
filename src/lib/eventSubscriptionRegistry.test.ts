import { describe, expect, it, vi } from "vitest";
import { createEventSubscriptionRegistry, eventStreamKey } from "./eventSubscriptionRegistry";
import type { TransportClient } from "./transportClient";

function fixture() {
  const closes: ReturnType<typeof vi.fn>[] = [];
  const connect = vi.fn<TransportClient["subscribe"]>(() => {
    const close = vi.fn();
    closes.push(close);
    return close;
  });
  return { connect, closes, registry: createEventSubscriptionRegistry(connect) };
}

describe("page event subscription ownership", () => {
  it("fans out N consumers over one connection and closes only after the last unsubscribe", () => {
    const f = fixture();
    const listeners = Array.from({ length: 20 }, () => vi.fn());
    const stops = listeners.map(listener => f.registry.subscribe("/api/events/stream", listener));
    expect(f.connect).toHaveBeenCalledTimes(1);
    f.connect.mock.calls[0][1]("message", { event_type: "plan-changed", data: { id: "plan" } });
    for (const listener of listeners) expect(listener).toHaveBeenCalledExactlyOnceWith("plan-changed", { id: "plan" });
    stops.slice(0, -1).forEach(stop => stop());
    expect(f.closes[0]).not.toHaveBeenCalled();
    stops[stops.length - 1]();
    stops[stops.length - 1]();
    expect(f.closes[0]).toHaveBeenCalledTimes(1);
  });
  it("shares replay and decoded consumers without losing the raw envelope", () => {
    const f = fixture();
    const raw = vi.fn();
    const decoded = vi.fn();
    f.registry.subscribe("/api/events/stream?since=boot%3A1", raw, { raw: true });
    f.registry.subscribe("/api/events/stream", decoded);
    expect(f.connect).toHaveBeenCalledTimes(1);
    const envelope = { event_type: "chat", data: "turn", boot: "boot", seq: 2 };
    f.connect.mock.calls[0][1]("message", envelope);
    expect(raw).toHaveBeenCalledExactlyOnceWith("message", envelope);
    expect(decoded).toHaveBeenCalledExactlyOnceWith("chat", "turn");
  });
  it("moves the single connection on client replacement and rejects stale callbacks", () => {
    const f = fixture();
    const listener = vi.fn();
    const activity = vi.fn();
    const end = vi.fn();
    const stop = f.registry.subscribe("/api/events/stream", listener, { onActivity: activity, onEnd: end });
    f.registry.subscribe("/api/events/stream", () => {});
    f.connect.mock.calls[0][1]("message", { boot: "current", seq: 7 });
    listener.mockClear();
    const replacement = vi.fn<TransportClient["subscribe"]>(() => vi.fn());
    f.registry.replace(replacement);
    expect(f.closes[0]).toHaveBeenCalledTimes(1);
    expect(replacement).toHaveBeenCalledTimes(1);
    expect(replacement.mock.calls[0][0]).toBe("/api/events/stream?since=current%3A7");
    f.connect.mock.calls[0][1]("message", "old");
    f.connect.mock.calls[0][2]?.onActivity?.();
    f.connect.mock.calls[0][2]?.onEnd?.({ reason: "eof" });
    expect(listener).not.toHaveBeenCalled();
    expect(activity).not.toHaveBeenCalled();
    expect(end).not.toHaveBeenCalled();
    replacement.mock.calls[0][1]("message", "new");
    expect(listener).toHaveBeenCalledExactlyOnceWith("message", "new");
    stop();
  });
  it("isolates consumer aborts and clears ended connections before consumers reconnect", () => {
    const f = fixture();
    const abort = new AbortController();
    const ended = vi.fn();
    f.registry.subscribe("/api/events/stream", () => {}, { signal: abort.signal, onEnd: ended });
    const reconnect = vi.fn(() => f.registry.subscribe("/api/events/stream", () => {}));
    f.registry.subscribe("/api/events/stream", () => {}, { onEnd: reconnect });
    abort.abort("cancelled");
    expect(ended).toHaveBeenCalledExactlyOnceWith({ reason: "abort", error: "cancelled" });
    expect(f.closes[0]).not.toHaveBeenCalled();
    f.connect.mock.calls[0][2]?.onEnd?.({ reason: "eof" });
    expect(reconnect).toHaveBeenCalledTimes(1);
    expect(f.connect).toHaveBeenCalledTimes(2);
  });
  it("resumes the last shared cursor when a consumer reconnects first after EOF", () => {
    const f = fixture();
    f.registry.subscribe("/api/events/stream", () => {}, {
      onEnd: () => { f.registry.subscribe("/api/events/stream", () => {}); },
    });
    f.connect.mock.calls[0][1]("message", { boot: "boot", seq: 42 });
    f.connect.mock.calls[0][2]?.onEnd?.({ reason: "eof" });
    expect(f.connect.mock.calls[1][0]).toBe("/api/events/stream?since=boot%3A42");
    f.connect.mock.calls[1][1]("message", { event_type: "overflow", data: { boot: "new", next_seq: 5 } });
    const next = vi.fn<TransportClient["subscribe"]>(() => () => {});
    f.registry.replace(next);
    expect(next.mock.calls[0][0]).toBe("/api/events/stream?since=new%3A4");
  });
  it("delivers to other consumers when one callback throws", () => {
    const f = fixture();
    const error = new Error("consumer failure");
    f.registry.subscribe("/api/events/stream", () => { throw error; });
    const listener = vi.fn();
    f.registry.subscribe("/api/events/stream", listener);
    expect(() => f.connect.mock.calls[0][1]("message", "turn")).toThrow(error);
    expect(listener).toHaveBeenCalledExactlyOnceWith("message", "turn");
  });
  it("keeps distinct peers and non-event streams outside the local events connection", () => {
    expect(eventStreamKey("/api/mesh/relay-stream?peer=p&path=%2Fapi%2Fevents%2Fstream&since=b%3A1"))
      .toBe("/api/mesh/relay-stream?path=%2Fapi%2Fevents%2Fstream&peer=p");
    const f = fixture();
    for (const path of ["/api/events/stream", "/api/mesh/relay-stream?peer=p&path=%2Fapi%2Fevents%2Fstream", "/api/output/stream"]) {
      f.registry.subscribe(path, () => {});
    }
    expect(f.connect).toHaveBeenCalledTimes(3);
  });
});
