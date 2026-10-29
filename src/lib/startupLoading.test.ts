import { expect, it, vi } from "vitest";
import { createStartupLoading } from "./startupLoading";
import { createTransportClient, type TransportClient } from "./transportClient";

it("shares readiness events while areas independently remain loading", async () => {
  let publish!: Parameters<TransportClient["subscribe"]>[1];
  const close = vi.fn();
  const subscribe = vi.fn<TransportClient["subscribe"]>((_path, callback) => { publish = callback; return close; });
  const loading = createStartupLoading(subscribe);
  const plansDone = vi.fn();
  const plans = loading.wait("plans").then(plansDone);
  const memory = loading.wait("memory");
  await publish("startup-state", { state: "starting", desktop_ready: true, routes_ready: false });
  expect(plansDone).not.toHaveBeenCalled();
  await publish("startup-state", { state: "starting", routes_ready: true, subsystems: { plans: "starting", memory: "ready" } });
  await memory;
  expect(plansDone).not.toHaveBeenCalled();
  expect(close).not.toHaveBeenCalled();
  await publish("startup-state", { state: "starting", routes_ready: true, subsystems: { plans: "ready" } });
  await plans;
  expect(plansDone).toHaveBeenCalledOnce();
  expect(subscribe).toHaveBeenCalledOnce();
  expect(close).toHaveBeenCalledOnce();
});

it("cancels one area's wait without losing another area's subscription", async () => {
  let publish!: Parameters<TransportClient["subscribe"]>[1];
  const close = vi.fn();
  const loading = createStartupLoading((_path, callback) => { publish = callback; return close; });
  const controller = new AbortController();
  const cancelled = loading.wait("plugins", controller.signal);
  const retained = loading.wait("plans");
  controller.abort(new Error("fixture cancellation"));
  await expect(cancelled).rejects.toThrow("fixture cancellation");
  expect(close).not.toHaveBeenCalled();
  await publish("startup-state", { state: "ready" });
  await retained;
  expect(close).toHaveBeenCalledOnce();
});

it("waits for a typed starting read and never replays a mutation", async () => {
  const pending = () => Response.json({ error: { code: "starting", subsystem: "plans" } }, { status: 503 });
  let reads = 0;
  const fetch = vi.fn<typeof globalThis.fetch>().mockImplementation(async (input, init) => {
    if (String(input).endsWith("/api/startup/events")) {
      return new Response('event: startup-state\ndata: {"state":"ready"}\n\n', { headers: { "Content-Type": "text/event-stream" } });
    }
    if (init?.method === "POST") return pending();
    return ++reads === 1 ? pending() : Response.json({ plans: ["opaque"] });
  });
  const client = createTransportClient({ origin: "http://127.0.0.1:17786", token: "fixture-token", fetch });
  expect(await client.json("GET", "/api/plans")).toEqual({ plans: ["opaque"] });
  expect(reads).toBe(2);
  const before = fetch.mock.calls.length;
  await expect(client.json("POST", "/api/plans", { opaque: true })).rejects.toThrow("HTTP 503");
  expect(fetch.mock.calls.length).toBe(before + 1);
});
