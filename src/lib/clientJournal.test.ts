import { afterEach, expect, test, vi } from "vitest";
const send = vi.hoisted(() => vi.fn());
vi.mock("./daemonBootstrap", () => ({ baseDaemonTransport: () => ({ localJson: send }) }));
afterEach(() => { vi.resetModules(); send.mockReset(); });
test("connection recovery flushes retained loss and records no message content", async () => {
  send.mockRejectedValueOnce(new Error("offline")).mockResolvedValue({ ok: true });
  const { recordClientHealth } = await import("./clientJournal");
  recordClientHealth({ event: "connection-lost", elapsedMs: 0 });
  await send.mock.results[0].value.catch(() => {});
  let restored!: () => void;
  const flushed = new Promise<void>(resolve => { restored = resolve; });
  send.mockImplementation(async () => { if (send.mock.calls.length === 3) restored(); return { ok: true }; });
  recordClientHealth({ event: "connection-restored", elapsedMs: 2500 });
  await flushed;
  expect(send.mock.calls.slice(1).map(call => call[2])).toEqual([
    { event: "connection-lost", elapsedMs: 0 }, { event: "connection-restored", elapsedMs: 2500 },
  ]);
});
test("diagnostic flood is bounded", async () => {
  send.mockRejectedValue(new Error("offline"));
  const { recordClientHealth } = await import("./clientJournal");
  for (let i = 0; i < 1000; i++) recordClientHealth({ event: "preview-failed", elapsedMs: 1 });
  await send.mock.results[0].value.catch(() => {});
  expect(send).toHaveBeenCalledTimes(1);
});
