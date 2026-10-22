import { expect, it, vi } from "vitest";
import { waitForServerReady } from "./serverReadiness";
import type { TransportClient } from "./transportClient";

function fixture() {
  const close = vi.fn();
  const subscribe = vi.fn<TransportClient["subscribe"]>(() => close);
  const controller = new AbortController();
  return { close, subscribe, controller };
}

it("waits for the readiness event and disposes once without retries", async () => {
  const f = fixture();
  const complete = vi.fn();
  const ready = waitForServerReady(f, f.controller.signal).then(complete);
  const deliver = f.subscribe.mock.calls[0][1];
  await deliver("startup-state", { state: "starting" });
  expect(complete).not.toHaveBeenCalled();
  await deliver("other-event", { state: "ready" });
  expect(complete).not.toHaveBeenCalled();
  await deliver("startup-state", { state: "ready" });
  await ready;
  expect(complete).toHaveBeenCalledOnce();
  expect(f.subscribe).toHaveBeenCalledOnce();
  expect(f.subscribe.mock.calls[0][0]).toBe("/api/startup/events");
  expect(f.close).toHaveBeenCalledOnce();
});

it("accepts an initial ready snapshot delivered synchronously", async () => {
  const close = vi.fn();
  const subscribe: TransportClient["subscribe"] = (_path, deliver) => {
    void deliver("startup-state", { state: "ready" });
    return close;
  };
  await waitForServerReady({ subscribe }, new AbortController().signal);
  expect(close).toHaveBeenCalledOnce();
});

it("rejects startup failure, stream exit and cancellation without reconnecting", async () => {
  for (const reason of ["failed", "eof", "abort"]) {
    const f = fixture();
    const ready = waitForServerReady(f, f.controller.signal);
    const verdict = expect(ready).rejects.toBeDefined();
    if (reason === "failed") await f.subscribe.mock.calls[0][1]("startup-state", { state: "failed" });
    if (reason === "eof") f.subscribe.mock.calls[0][2]?.onEnd?.({ reason: "eof" });
    if (reason === "abort") f.controller.abort();
    await verdict;
    expect(f.close).toHaveBeenCalledOnce();
    expect(f.subscribe).toHaveBeenCalledOnce();
  }
});
