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

it("waits for a typed starting read but rejects a mutation without non-execution proof", async () => {
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

function deferred() {
  let resolve!: () => void;
  const promise = new Promise<void>(done => { resolve = done; });
  return { promise, resolve };
}

it.each(["POST", "PUT", "PATCH", "DELETE"] as const)("holds a refused %s until readiness and executes it once", async method => {
  const subscribed = deferred();
  const ready = deferred();
  let attempts = 0;
  let executions = 0;
  const fetch = vi.fn<typeof globalThis.fetch>().mockImplementation(async (input) => {
    if (String(input).endsWith("/api/startup/events")) {
      subscribed.resolve();
      await ready.promise;
      return new Response('event: startup-state\ndata: {"state":"ready"}\n\nevent: startup-state\ndata: {"state":"ready"}\n\n', {
        headers: { "Content-Type": "text/event-stream" },
      });
    }
    if (++attempts === 1) return Response.json({
      error: { code: "starting", subsystem: "service", request_executed: false },
    }, { status: 503 });
    executions++;
    return Response.json({ accepted: true });
  });
  const client = createTransportClient({ origin: "http://127.0.0.1:17786", token: "fixture-token", fetch });
  const result = client.json(method, "/api/tabs/opaque/wake", { opaque: true });
  await subscribed.promise;
  expect(attempts).toBe(1);
  expect(executions).toBe(0);
  ready.resolve();
  expect(await result).toEqual({ accepted: true });
  expect(attempts).toBe(2);
  expect(executions).toBe(1);
  const mutations = fetch.mock.calls.filter(([, init]) => init?.method === method);
  expect(mutations).toHaveLength(2);
  expect(mutations[0][1]?.body).toBe(mutations[1][1]?.body);
});

it("accepts a current ready snapshot when readiness preceded the refusal response", async () => {
  let attempts = 0;
  const fetch = vi.fn<typeof globalThis.fetch>().mockImplementation(async input => {
    if (String(input).endsWith("/api/startup/events")) return new Response(
      'event: startup-state\ndata: {"state":"ready"}\n\n',
      { headers: { "Content-Type": "text/event-stream" } },
    );
    return ++attempts === 1
      ? Response.json({ error: { code: "starting", subsystem: "service", request_executed: false } }, { status: 503 })
      : Response.json({ accepted: true });
  });
  const client = createTransportClient({ origin: "http://127.0.0.1:17786", token: "fixture-token", fetch });
  expect(await client.json("POST", "/api/tabs/opaque/wake")).toEqual({ accepted: true });
  expect(attempts).toBe(2);
});

it.each([
  { code: "unavailable", subsystem: "service", request_executed: false },
  { code: "starting", subsystem: "service", request_executed: true },
  { code: "starting", subsystem: "service" },
])("does not reissue other service refusals: %j", async error => {
  const fetch = vi.fn<typeof globalThis.fetch>().mockImplementation(async () => Response.json({ error }, { status: 503 }));
  const client = createTransportClient({ origin: "http://127.0.0.1:17786", token: "fixture-token", fetch });
  await expect(client.json("POST", "/api/tabs/opaque/wake")).rejects.toThrow("HTTP 503");
  expect(fetch).toHaveBeenCalledOnce();
});

it("does not reissue a mutation after transport uncertainty", async () => {
  const fetch = vi.fn<typeof globalThis.fetch>().mockRejectedValue(new Error("opaque transport uncertainty"));
  const client = createTransportClient({ origin: "http://127.0.0.1:17786", token: "fixture-token", fetch });
  await expect(client.json("POST", "/api/tabs/opaque/wake")).rejects.toThrow("opaque transport uncertainty");
  expect(fetch).toHaveBeenCalledOnce();
});

it.each([500, 502, 504])("does not reissue a mutation after HTTP %i", async status => {
  const fetch = vi.fn<typeof globalThis.fetch>().mockImplementation(async () => Response.json({
    error: { code: "starting", subsystem: "service", request_executed: false },
  }, { status }));
  const client = createTransportClient({ origin: "http://127.0.0.1:17786", token: "fixture-token", fetch });
  await expect(client.json("POST", "/api/tabs/opaque/wake")).rejects.toThrow(`HTTP ${status}`);
  expect(fetch).toHaveBeenCalledOnce();
});

it("does not loop if the route still refuses after readiness", async () => {
  let attempts = 0;
  const fetch = vi.fn<typeof globalThis.fetch>().mockImplementation(async input => {
    if (String(input).endsWith("/api/startup/events")) return new Response(
      'event: startup-state\ndata: {"state":"ready"}\n\n',
      { headers: { "Content-Type": "text/event-stream" } },
    );
    attempts++;
    return Response.json({ error: { code: "starting", subsystem: "service", request_executed: false } }, { status: 503 });
  });
  const client = createTransportClient({ origin: "http://127.0.0.1:17786", token: "fixture-token", fetch });
  await expect(client.json("POST", "/api/tabs/opaque/wake")).rejects.toThrow("HTTP 503");
  expect(attempts).toBe(2);
});

it("never reissues a streamed mutation even with non-execution proof", async () => {
  const fetch = vi.fn<typeof globalThis.fetch>().mockImplementation(async () => Response.json({
    error: { code: "starting", subsystem: "service", request_executed: false },
  }, { status: 503 }));
  const client = createTransportClient({ origin: "http://127.0.0.1:17786", token: "fixture-token", fetch });
  const body = new ReadableStream<Uint8Array>({ start(controller) { controller.close(); } });
  const response = await client.request!("/api/tabs/opaque/chat-send", { method: "POST", body });
  expect(response.status).toBe(503);
  expect(fetch).toHaveBeenCalledOnce();
  expect(fetch.mock.calls[0][1]?.body).toBe(body);
});

it("never reissues a mutation cancelled while waiting for startup readiness", async () => {
  const fetch = vi.fn<typeof globalThis.fetch>().mockImplementation(async () => Response.json({
    error: { code: "starting", subsystem: "service", request_executed: false },
  }, { status: 503 }));
  const client = createTransportClient({ origin: "http://127.0.0.1:17786", token: "fixture-token", fetch });
  const subscribed = deferred();
  let publish!: Parameters<TransportClient["subscribe"]>[1];
  const close = vi.fn();
  client.subscribe = vi.fn<TransportClient["subscribe"]>((_path, callback) => {
    publish = callback;
    subscribed.resolve();
    return close;
  });
  const controller = new AbortController();
  const pending = client.json("POST", "/api/tabs/opaque/chat-send", { opaque: true }, { signal: controller.signal });
  const rejected = expect(pending).rejects.toThrow("fixture cancellation");
  await subscribed.promise;
  controller.abort(new Error("fixture cancellation"));
  await rejected;
  await publish("startup-state", { state: "ready" });
  expect(fetch).toHaveBeenCalledOnce();
  expect(close).toHaveBeenCalledOnce();
});

it("serializes held and newly arriving chat sends to one tab in their original order", async () => {
  const subscribed = deferred();
  const ready = deferred();
  const firstAccepted = deferred();
  const finishFirst = deferred();
  const requests: string[] = [];
  const executions: string[] = [];
  const fetch = vi.fn<typeof globalThis.fetch>().mockImplementation(async (input, init) => {
    if (String(input).endsWith("/api/startup/events")) {
      subscribed.resolve();
      await ready.promise;
      return new Response('event: startup-state\ndata: {"state":"ready"}\n\n', {
        headers: { "Content-Type": "text/event-stream" },
      });
    }
    const { text } = JSON.parse(String(init?.body)) as { text: string };
    requests.push(text);
    if (requests.length === 1) return Response.json({
      error: { code: "starting", subsystem: "service", request_executed: false },
    }, { status: 503 });
    executions.push(text);
    if (text === "first") {
      firstAccepted.resolve();
      await finishFirst.promise;
    }
    return Response.json({ accepted: true });
  });
  const client = createTransportClient({ origin: "http://127.0.0.1:17786", token: "fixture-token", fetch });
  const first = client.json("POST", "/api/tabs/opaque/chat-send", { text: "first" });
  await subscribed.promise;
  const second = client.localJson("POST", "/api/tabs/opaque/chat-send", { text: "second" });
  expect(requests).toEqual(["first"]);
  ready.resolve();
  await firstAccepted.promise;
  const third = client.json("POST", "/api/tabs/opaque/chat-send", { text: "third" });
  expect(executions).toEqual(["first"]);
  finishFirst.resolve();
  await Promise.all([first, second, third]);
  expect(requests).toEqual(["first", "first", "second", "third"]);
  expect(executions).toEqual(["first", "second", "third"]);
});

it("cancels a queued mutation without letting later requests overtake a startup-held head", async () => {
  const subscribed = deferred();
  const ready = deferred();
  const requests: string[] = [];
  const fetch = vi.fn<typeof globalThis.fetch>().mockImplementation(async (input, init) => {
    if (String(input).endsWith("/api/startup/events")) {
      subscribed.resolve();
      await ready.promise;
      return new Response('event: startup-state\ndata: {"state":"ready"}\n\n', { headers: { "Content-Type": "text/event-stream" } });
    }
    const { text } = JSON.parse(String(init?.body)) as { text: string };
    requests.push(text);
    return requests.length === 1
      ? Response.json({ error: { code: "starting", subsystem: "service", request_executed: false } }, { status: 503 })
      : Response.json({ accepted: true });
  });
  const client = createTransportClient({ origin: "http://127.0.0.1:17786", token: "fixture-token", fetch });
  const first = client.json("POST", "/api/tabs/opaque/chat-send", { text: "first" });
  await subscribed.promise;
  const controller = new AbortController();
  const second = client.json("POST", "/api/tabs/opaque/chat-send", { text: "second" }, { signal: controller.signal });
  const rejected = expect(second).rejects.toThrow("fixture queued cancellation");
  const third = client.json("POST", "/api/tabs/opaque/chat-send", { text: "third" });
  controller.abort(new Error("fixture queued cancellation"));
  await rejected;
  expect(requests).toEqual(["first"]);
  ready.resolve();
  await Promise.all([first, third]);
  expect(requests).toEqual(["first", "first", "third"]);
});

it("runs ordinary same-tab mutations concurrently and never delays Stop behind a slow send", async () => {
  const started = deferred();
  const finishFirst = deferred();
  const secondArrived = deferred();
  const stopArrived = deferred();
  const fetch = vi.fn<typeof globalThis.fetch>().mockImplementation(async (input, init) => {
    if (String(input).endsWith("/stop")) { stopArrived.resolve(); return Response.json({ stopped: true }); }
    const { text } = JSON.parse(String(init?.body)) as { text: string };
    if (text === "first") { started.resolve(); await finishFirst.promise; }
    else secondArrived.resolve();
    return Response.json({ accepted: true });
  });
  const client = createTransportClient({ origin: "http://127.0.0.1:17786", token: "fixture-token", fetch });
  const first = client.json("POST", "/api/tabs/opaque/chat-send", { text: "first" });
  await started.promise;
  const second = client.json("POST", "/api/tabs/opaque/chat-send", { text: "second" });
  const stop = client.json("POST", "/api/tabs/opaque/stop");
  await Promise.all([secondArrived.promise, stopArrived.promise]);
  expect(await second).toEqual({ accepted: true });
  expect(await stop).toEqual({ stopped: true });
  finishFirst.resolve();
  await first;
});

it.each([
  { held: "/api/tabs/opaque/chat-send", control: "/api/tabs/opaque/stop", body: {} },
  { held: "/api/tabs/opaque/chat-send", control: "/api/tabs/opaque/close", body: {} },
  { held: "/api/pty/opaque/write", control: "/api/pty/opaque/write", body: { data: "\x03", raw: true } },
  { held: "/api/pty/opaque/write", control: "/api/pty/opaque/write", body: { data: "\x1b", raw: true } },
])("lets control $control bypass a startup-held target", async ({ held, control, body }) => {
  const subscribed = deferred();
  const ready = deferred();
  let requests = 0;
  const fetch = vi.fn<typeof globalThis.fetch>().mockImplementation(async input => {
    if (String(input).endsWith("/api/startup/events")) {
      subscribed.resolve();
      await ready.promise;
      return new Response('event: startup-state\ndata: {"state":"ready"}\n\n', { headers: { "Content-Type": "text/event-stream" } });
    }
    return ++requests === 1
      ? Response.json({ error: { code: "starting", subsystem: "service", request_executed: false } }, { status: 503 })
      : Response.json({ accepted: true });
  });
  const client = createTransportClient({ origin: "http://127.0.0.1:17786", token: "fixture-token", fetch });
  const pending = client.json("POST", held, { data: "opaque", text: "opaque" });
  await subscribed.promise;
  expect(await client.json("POST", control, body)).toEqual({ accepted: true });
  expect(requests).toBe(2);
  ready.resolve();
  await pending;
});

it("does not let a startup-held relay to one peer block another peer", async () => {
  const subscribed = deferred();
  const ready = deferred();
  const requests: string[] = [];
  const fetch = vi.fn<typeof globalThis.fetch>().mockImplementation(async (input, init) => {
    if (String(input).endsWith("/api/startup/events")) {
      subscribed.resolve();
      await ready.promise;
      return new Response('event: startup-state\ndata: {"state":"ready"}\n\n', { headers: { "Content-Type": "text/event-stream" } });
    }
    const relay = JSON.parse(String(init?.body)) as { target_peer_id: string };
    requests.push(relay.target_peer_id);
    return requests.length === 1
      ? Response.json({ error: { code: "starting", subsystem: "mesh", request_executed: false } }, { status: 503 })
      : Response.json({ ok: true });
  });
  const client = createTransportClient({ origin: "http://127.0.0.1:17786", token: "fixture-token", fetch });
  const first = client.json("POST", "/api/mesh/relay-to", { target_peer_id: "first", method: "POST", path: "/api/tabs/opaque/chat-send", body: {} });
  await subscribed.promise;
  expect(await client.json("POST", "/api/mesh/relay-to", { target_peer_id: "second", method: "POST", path: "/api/tabs/opaque/chat-send", body: {} })).toEqual({ ok: true });
  expect(requests).toEqual(["first", "second"]);
  ready.resolve();
  await first;
  expect(requests).toEqual(["first", "second", "first"]);
});

it.each([
  ["terminal", "/api/output/stream"],
  ["chat", "/api/events/stream?events=chat-msg"],
])("connects the %s stream after startup without reporting a connection error", async (_pane, path) => {
  const subscribed = deferred();
  const ready = deferred();
  const delivered = deferred();
  let attempts = 0;
  const fetch = vi.fn<typeof globalThis.fetch>().mockImplementation(async input => {
    if (String(input).endsWith("/api/startup/events")) {
      subscribed.resolve(); await ready.promise;
      return new Response('event: startup-state\ndata: {"state":"ready"}\n\n', { headers: { "Content-Type": "text/event-stream" } });
    }
    if (++attempts === 1) return Response.json({ error: { code: "starting", subsystem: "service" } }, { status: 503 });
    return new Response(new ReadableStream<Uint8Array>({ start(controller) {
      controller.enqueue(new TextEncoder().encode('event: fixture\ndata: {"connected":true}\n\n'));
    } }), { headers: { "Content-Type": "text/event-stream" } });
  });
  const client = createTransportClient({ origin: "http://127.0.0.1:17786", token: "fixture-token", fetch });
  const connected = vi.fn();
  const starting = vi.fn();
  const ended = vi.fn();
  const receive = vi.fn(() => { delivered.resolve(); });
  const dispose = client.subscribe(path, receive, { requireEventStream: true, onStarting: starting, onReady: connected, onEnd: ended });
  try {
    await subscribed.promise;
    expect(connected).not.toHaveBeenCalled();
    expect(starting).toHaveBeenCalledOnce();
    expect(ended).not.toHaveBeenCalled();
    ready.resolve();
    await delivered.promise;
    expect(connected).toHaveBeenCalledOnce();
    expect(ended).not.toHaveBeenCalled();
    expect(receive).toHaveBeenCalledWith("fixture", { connected: true });
    expect(attempts).toBe(2);
  } finally { ready.resolve(); dispose(); }
});
