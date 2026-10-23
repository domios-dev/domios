import { expect, it } from "vitest";
import { canRetryChatStart, createChatStartFlight } from "./chatStartFlight";

it("deduplicates one subscription and serializes different subscriptions per tab", async () => {
  const run = createChatStartFlight();
  let finish!: (value: string) => void;
  let starts = 0;
  const start = () => { starts++; return new Promise<string>(resolve => { finish = resolve; }); };
  const first = run("tab", "subscription", start);
  const same = run("tab", "subscription", start);
  const next = run("tab", "other", async () => { starts++; return "next"; });
  await Promise.resolve();
  expect(starts).toBe(1);
  finish("ready");
  expect(await first).toBe("ready");
  expect(await same).toBe("ready");
  expect(await next).toBe("next");
  expect(starts).toBe(2);
  expect(await run("tab", "new", async () => "reattached")).toBe("reattached");
});

it("one consumer leaving cannot abort the shared snapshot of another consumer", async () => {
  const run = createChatStartFlight();
  const left = new AbortController();
  const retained = new AbortController();
  let shared!: AbortSignal;
  let finish!: (value: string) => void;
  const start = (signal: AbortSignal) => { shared = signal; return new Promise<string>(resolve => { finish = resolve; }); };
  const first = run("tab", "subscription", start, left.signal);
  const second = run("tab", "subscription", start, retained.signal);
  const firstResult = expect(first).rejects.toBe("left");
  await Promise.resolve();
  left.abort("left");
  expect(shared.aborted).toBe(false);
  finish("snapshot");
  await firstResult;
  expect(await second).toBe("snapshot");
});

it("an aborted last consumer cannot poison a subsequent same-subscription start", async () => {
  const run = createChatStartFlight();
  const controller = new AbortController();
  const first = run("tab", "subscription", signal => new Promise<string>((_, reject) => {
    signal.addEventListener("abort", () => reject(signal.reason), { once: true });
  }), controller.signal);
  const rejected = expect(first).rejects.toBe("left");
  await Promise.resolve();
  controller.abort("left");
  const next = run("tab", "subscription", async () => "fresh");
  await rejected;
  expect(await next).toBe("fresh");
});

it("releases failed starts and only permits explicit or lifecycle retries", async () => {
  const run = createChatStartFlight();
  await expect(run("tab", "failed", async () => { throw new Error("launch failed"); })).rejects.toThrow("launch failed");
  for (const cause of ["recover_gap", "pending_recover_gap", "resume_fallback"]) {
    expect(canRetryChatStart("failed", cause)).toBe(false);
  }
  for (const cause of ["retry", "authority_changed", "history_settled", "resync:epoch_changed",
    "tab_activated", "window_activated", "resync:connected", "listener_owned"]) {
    expect(canRetryChatStart("failed", cause)).toBe(true);
  }
  expect(await run("tab", "retry", async () => "ready")).toBe("ready");
});
