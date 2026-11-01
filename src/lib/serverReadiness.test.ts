import { expect, it, vi } from "vitest";
import { currentStartupPhase, startupPhases, waitForDesktopReady } from "./serverReadiness";
import { createTransportClient, type TransportClient } from "./transportClient";

function fixture() {
  const close = vi.fn();
  const subscribe = vi.fn<TransportClient["subscribe"]>(() => close);
  const controller = new AbortController();
  return { close, subscribe, controller };
}

it("waits for the readiness event and disposes once without retries", async () => {
  const f = fixture();
  const complete = vi.fn();
  const ready = waitForDesktopReady(f, f.controller.signal).then(complete);
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
  await waitForDesktopReady({ subscribe }, new AbortController().signal);
  expect(close).toHaveBeenCalledOnce();
});

it("releases the desktop while optional service areas are still starting", async () => {
  const f = fixture();
  const ready = waitForDesktopReady(f, f.controller.signal);
  await f.subscribe.mock.calls[0][1]("startup-state", {
    state: "starting", desktop_ready: true, routes_ready: false,
    phases: { service: "starting", plans: "starting" },
  });
  await ready;
  expect(f.close).toHaveBeenCalledOnce();
});

it("rejects startup failure, unsupported daemons and cancellation without reconnecting", async () => {
  for (const reason of ["failed", "http", "abort"]) {
    const f = fixture();
    const ready = waitForDesktopReady(f, f.controller.signal);
    const verdict = expect(ready).rejects.toBeDefined();
    if (reason === "failed") await f.subscribe.mock.calls[0][1]("startup-state", { state: "failed" });
    if (reason === "http") f.subscribe.mock.calls[0][2]?.onEnd?.({ reason: "http", status: 404 });
    if (reason === "abort") f.controller.abort();
    await verdict;
    expect(f.close).toHaveBeenCalledOnce();
    expect(f.subscribe).toHaveBeenCalledOnce();
  }
});

it("resubscribes after listener replacement and accepts the new terminal snapshot", async () => {
  const f = fixture();
  const complete = vi.fn();
  const ready = waitForDesktopReady(f, f.controller.signal).then(complete);
  await f.subscribe.mock.calls[0][1]("startup-state", { state: "starting" });
  f.subscribe.mock.calls[0][2]?.onEnd?.({ reason: "eof" });
  await Promise.resolve();
  expect(f.subscribe).toHaveBeenCalledTimes(2);
  expect(complete).not.toHaveBeenCalled();
  await f.subscribe.mock.calls[0][1]("startup-state", { state: "failed" });
  await f.subscribe.mock.calls[1][1]("startup-state", { state: "ready" });
  await ready;
  expect(complete).toHaveBeenCalledOnce();
  expect(f.close).toHaveBeenCalledTimes(2);
});

it("cancels between listener generations without opening another subscription", async () => {
  const f = fixture();
  const ready = waitForDesktopReady(f, f.controller.signal);
  const verdict = expect(ready).rejects.toBeDefined();
  f.subscribe.mock.calls[0][2]?.onEnd?.({ reason: "eof" });
  f.controller.abort();
  await verdict;
  expect(f.subscribe).toHaveBeenCalledOnce();
});

it("rejects an older daemon's HTML fallback without reconnecting forever", async () => {
  const controller = new AbortController();
  const fetcher = vi.fn(async () => {
    if (fetcher.mock.calls.length > 1) controller.abort();
    return new Response("<html>Packaged frontend</html>", { headers: { "content-type": "text/html" } });
  });
  const client = createTransportClient({ origin: "http://127.0.0.1:7786", token: "fixture-token", fetch: fetcher });
  await expect(waitForDesktopReady(client, controller.signal)).rejects.toThrow("not an event stream");
  expect(fetcher).toHaveBeenCalledOnce();
});

it.each(startupPhases)("reports the first unfinished phase in boot order: %s", phase => {
  const index = startupPhases.indexOf(phase);
  const phases = Object.fromEntries([...startupPhases].reverse().map(name =>
    [name, startupPhases.indexOf(name) < index ? "ready" : "starting"]));
  expect(currentStartupPhase({ state: "starting", phases })).toBe(phase);
});

it("never invents a phase for terminal or incomplete snapshots", () => {
  for (const snapshot of [null, {}, { state: "starting" }, { state: "starting", phases: null },
    { state: "ready", phases: { service: "starting" } }, { state: "failed", phases: { plans: "starting" } }]) {
    expect(currentStartupPhase(snapshot)).toBeNull();
  }
});

it("publishes phase progress across listener replacement and clears it on readiness", async () => {
  const f = fixture();
  const progress = vi.fn();
  const ready = waitForDesktopReady(f, f.controller.signal, progress);
  await f.subscribe.mock.calls[0][1]("startup-state", { state: "starting", phases: { states: "starting", plans: "starting" } });
  f.subscribe.mock.calls[0][2]?.onEnd?.({ reason: "eof" });
  await Promise.resolve();
  await f.subscribe.mock.calls[0][1]("startup-state", { state: "starting", phases: { providers: "starting" } });
  await f.subscribe.mock.calls[1][1]("startup-state", { state: "starting", phases: { states: "ready", plans: "starting" } });
  await f.subscribe.mock.calls[1][1]("startup-state", { state: "ready" });
  await ready;
  expect(progress.mock.calls).toEqual([["states"], ["plans"], [null]]);
});
