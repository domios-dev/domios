import { afterEach, expect, it, vi } from "vitest";
import type { StreamEnd, TransportClient } from "./transportClient";

afterEach(() => { vi.useRealTimers(); });

async function connect() {
  vi.resetModules();
  vi.useFakeTimers();
  const output = await import("./outputStream");
  const connections: NonNullable<Parameters<TransportClient["subscribe"]>[2]>[] = [];
  output.bindOutputStreamTransport({
    origin: () => "http://daemon.test",
    subscribe(_path, _handler, options) { connections.push(options!); return () => {}; },
  } as TransportClient);
  const stop = output.subscribeTabOutput("mesh:peer/tab", { onOutput() {}, onTruncated() {}, onEnded() {} });
  return { connections, stop };
}

it.each(["eof", "error"] as const)("backs off after repeated real %s disconnects", async reason => {
  const { connections, stop } = await connect();
  try {
    connections[0].onEnd!({ reason });
    expect(connections).toHaveLength(1);
    await vi.advanceTimersByTimeAsync(249);
    expect(connections).toHaveLength(1);
    await vi.advanceTimersByTimeAsync(1);
    expect(connections).toHaveLength(2);
    connections[1].onEnd!({ reason });
    await vi.advanceTimersByTimeAsync(499);
    expect(connections).toHaveLength(2);
    await vi.advanceTimersByTimeAsync(1);
    expect(connections).toHaveLength(3);
    connections[0].onEnd!({ reason });
    await vi.advanceTimersByTimeAsync(30_000);
    expect(connections).toHaveLength(3);
  } finally { stop(); }
});

it("cancels a pending retry when the last terminal detaches", async () => {
  const { connections, stop } = await connect();
  connections[0].onEnd!({ reason: "error" });
  stop();
  await vi.advanceTimersByTimeAsync(30_000);
  expect(connections).toHaveLength(1);
});

it.each<StreamEnd>([{ reason: "http", status: 400 }, { reason: "abort" }])("does not retry $reason", async end => {
  const { connections, stop } = await connect();
  try {
    connections[0].onEnd!(end);
    await vi.advanceTimersByTimeAsync(30_000);
    expect(connections).toHaveLength(1);
  } finally { stop(); }
});
