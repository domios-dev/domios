import { afterEach, expect, it, vi } from "vitest";
import type { TransportClient } from "./transportClient";

afterEach(() => { vi.useRealTimers(); });

async function connect() {
  vi.resetModules();
  vi.useFakeTimers();
  const output = await import("./outputStream");
  const transport = await import("./remoteTabTransport");
  const connections: Array<{
    handler: Parameters<TransportClient["subscribe"]>[1];
    options: NonNullable<Parameters<TransportClient["subscribe"]>[2]>;
  }> = [];
  output.bindOutputStreamTransport({
    origin: () => "http://daemon.test",
    subscribe(_path, handler, options) { connections.push({ handler, options: options! }); return () => {}; },
  } as TransportClient);
  const stop = output.subscribeTabOutput("mesh:peer/tab", { onOutput() {}, onTruncated() {}, onEnded() {} });
  const recover = () => transport.configureRemoteTabTransport({
    enabled: true, host: "daemon.test", port: 7686, token: "recovered", cors: false,
  });
  return { connections, stop, recover };
}

it.each([
  { reason: "eof" as const, progress: false },
  { reason: "error" as const, progress: false },
  { reason: "error" as const, progress: true },
])("parks $reason with progress=$progress until an explicit recovery event", async ({ reason, progress }) => {
  const { connections, stop, recover } = await connect();
  try {
    if (progress) await connections[0].handler("message", { tab_id: "tab", boot: "boot", offset: 1, b64: "QQ" });
    connections[0].options.onEnd!({ reason });
    await vi.advanceTimersByTimeAsync(60_000);
    expect(connections).toHaveLength(1);
    recover();
    expect(connections).toHaveLength(2);
    expect(connections[1].options.signal!.aborted).toBe(false);
    connections[0].options.onEnd!({ reason: "error" });
    expect(connections).toHaveLength(2);
    expect(connections[1].options.signal!.aborted).toBe(false);
  } finally { stop(); }
});

it("does not revive a detached terminal on a later recovery event", async () => {
  const { connections, stop, recover } = await connect();
  stop();
  expect(connections[0].options.signal!.aborted).toBe(true);
  connections[0].options.onEnd!({ reason: "abort" });
  recover();
  await vi.advanceTimersByTimeAsync(60_000);
  expect(connections).toHaveLength(1);
});

it("parks a refused stream until an explicit recovery event", async () => {
  const { connections, stop, recover } = await connect();
  try {
    connections[0].options.onEnd!({ reason: "http", status: 400 });
    await vi.advanceTimersByTimeAsync(60_000);
    expect(connections).toHaveLength(1);
    recover();
    expect(connections).toHaveLength(2);
  } finally { stop(); }
});
