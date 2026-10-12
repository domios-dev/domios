// Run: npx tsx src/components/Surface/httpSseSurfaceTransport.test.ts

import { configureDaemonTransport } from "../../lib/daemonBootstrap";
import assert from "node:assert/strict";
import { createHttpSseSurfaceTransport, type FetchLike } from "./httpSseSurfaceTransport";
import { disconnectedNotice, MAX_IMMEDIATE_RECONNECTS, RECONNECTED_NOTICE } from "./surfaceHttpProtocol";
import type { StreamHandlers } from "./surfaceStreamTransport";

interface Waiter<T> {
  count: number;
  resolve(value: T[]): void;
}

class Recorder<T> {
  readonly items: T[] = [];
  private waiters: Waiter<T>[] = [];
  push(item: T) {
    this.items.push(item);
    this.waiters = this.waiters.filter((waiter) => {
      if (this.items.length < waiter.count) return true;
      waiter.resolve(this.items);
      return false;
    });
  }
  until(count: number): Promise<T[]> {
    if (this.items.length >= count) return Promise.resolve(this.items);
    return new Promise((resolve) => this.waiters.push({ count, resolve }));
  }
}

interface LiveBody {
  send(text: string): void;
  end(): void;
}

interface FetchCall {
  url: string;
  init: RequestInit;
}

type Reply = (call: FetchCall) => Promise<Response>;

function createFakeFetch() {
  const calls = new Recorder<FetchCall>();
  const replies: Reply[] = [];
  const fetch: FetchLike = (url, init) => {
    const call = { url, init };
    calls.push(call);
    const reply = replies.shift();
    if (!reply) return Promise.reject(new Error("no reply queued"));
    return reply(call);
  };
  const live = (): LiveBody => {
    let controller!: ReadableStreamDefaultController<Uint8Array>;
    const body = new ReadableStream<Uint8Array>({ start(next) { controller = next; } });
    const encoder = new TextEncoder();
    replies.push(async (call) => {
      call.init.signal?.addEventListener("abort", () => controller.error(new DOMException("aborted", "AbortError")));
      return new Response(body, { status: 200, headers: { "Content-Type": "text/event-stream" } });
    });
    return { send: (text) => controller.enqueue(encoder.encode(text)), end: () => controller.close() };
  };
  return {
    fetch,
    calls,
    live,
    status: (status: number, body: string) => replies.push(async () => new Response(body, { status })),
    networkError: (message: string) => replies.push(async () => { throw new TypeError(message); }),
  };
}

function recordHandlers() {
  const events = new Recorder<{ type: string; value: unknown }>();
  const handlers: StreamHandlers = {
    open: () => events.push({ type: "open", value: null }),
    state: (value) => events.push({ type: "state", value }),
    frame: (value) => events.push({ type: "frame", value }),
    notice: (value) => events.push({ type: "notice", value }),
  };
  return { events, handlers };
}

function signal() {
  let fire: () => void = () => undefined;
  let subscribed = 0;
  return {
    retrySignal: (onSignal: () => void) => {
      fire = onSignal;
      subscribed += 1;
      return () => { subscribed -= 1; fire = () => undefined; };
    },
    fire: () => fire(),
    subscribed: () => subscribed,
  };
}

const viewport = { w: 640, h: 480, dpr: 2 };
const base = "http://127.0.0.1:7686";
const tests: Array<[string, () => Promise<void>]> = [];
const test = (name: string, fn: () => Promise<void>) => tests.push([name, fn]);

test("streams with the auth header and dispatches events split across chunks", async () => {
  const fake = createFakeFetch();
  const body = fake.live();
  const { events, handlers } = recordHandlers();
  const transport = createHttpSseSurfaceTransport({ baseUrl: base, token: "secret", tabId: "t1", fetch: fake.fetch, retrySignal: signal().retrySignal });
  const disconnect = transport.connect(viewport, handlers);
  const [call] = await fake.calls.until(1);
  assert.equal(call.url, `${base}/api/surfaces/t1/stream?w=640&h=480&dpr=2`);
  assert.equal(new Headers(call.init.headers).get("Authorization"), "Bearer secret");
  await events.until(1);
  const metadata = { deviceWidth: 800, deviceHeight: 600 };
  body.send("event: state\ndata: {\"owner\":\"Je");
  body.send("v\"}\r\n\r\nevent: frame\ndata: ");
  body.send(`${JSON.stringify({ data: "QUJD", metadata })}\n\nevent: notice\ndata: {"text":"hi"}\n\n`);
  assert.deepEqual(await events.until(4), [
    { type: "open", value: null },
    { type: "state", value: { owner: "Jev" } },
    { type: "frame", value: { data: "QUJD", metadata } },
    { type: "notice", value: { text: "hi" } },
  ]);
  disconnect();
  assert.equal(call.init.signal?.aborted, true, "disconnect aborts the request");
});

test("a dropped stream reconnects with the same size and fires open again", async () => {
  const fake = createFakeFetch();
  const first = fake.live();
  fake.live();
  const { events, handlers } = recordHandlers();
  const transport = createHttpSseSurfaceTransport({ baseUrl: base, token: "", tabId: "t1", fetch: fake.fetch, retrySignal: signal().retrySignal });
  const disconnect = transport.connect(viewport, handlers);
  await events.until(1);
  first.send("event: state\ndata: {\"owner\":\"Jev\"}\n\n");
  await events.until(2);
  first.end();
  const calls = await fake.calls.until(2);
  assert.equal(calls[1].url, calls[0].url, "the reconnect reuses the connect params");
  assert.equal(new Headers(calls[0].init.headers).has("Authorization"), false, "no token sends no header");
  const opened = await events.until(3);
  assert.deepEqual(opened.map((event) => event.type), ["open", "state", "open"], "each connection fires open so the size is re-sent");
  disconnect();
});

test("a refused stream stops with a notice and never retries", async () => {
  const fake = createFakeFetch();
  fake.status(409, "{\"ok\":false,\"error\":{\"code\":\"not_a_stream_surface\",\"message\":\"Tab t1 is not a stream surface\"}}");
  const retry = signal();
  const { events, handlers } = recordHandlers();
  const disconnect = createHttpSseSurfaceTransport({ baseUrl: base, token: "", tabId: "t1", fetch: fake.fetch, retrySignal: retry.retrySignal }).connect(viewport, handlers);
  const [notice] = await events.until(1);
  assert.deepEqual(notice, { type: "notice", value: { text: "Live view unavailable: Tab t1 is not a stream surface", tone: "error" } });
  retry.fire();
  assert.equal(fake.calls.items.length, 1, "a reachability signal does not revive a refused stream");
  disconnect();
  assert.equal(retry.subscribed(), 0, "disconnect drops the signal subscription");
});

test("network failures retry up to the cap, park with a notice and resume on a signal", async () => {
  const fake = createFakeFetch();
  for (let index = 0; index <= MAX_IMMEDIATE_RECONNECTS; index += 1) fake.networkError("Failed to fetch");
  const retry = signal();
  const { events, handlers } = recordHandlers();
  const disconnect = createHttpSseSurfaceTransport({ baseUrl: base, token: "", tabId: "t1", fetch: fake.fetch, retrySignal: retry.retrySignal }).connect(viewport, handlers);
  const [parked] = await events.until(1);
  assert.deepEqual(parked, { type: "notice", value: disconnectedNotice("Failed to fetch") });
  assert.equal(fake.calls.items.length, MAX_IMMEDIATE_RECONNECTS + 1, "one attempt plus the capped retries");
  fake.live();
  retry.fire();
  const resumed = await events.until(3);
  assert.deepEqual(resumed.slice(1), [{ type: "open", value: null }, { type: "notice", value: RECONNECTED_NOTICE }]);
  disconnect();
});

test("disconnect silences an in-flight stream", async () => {
  const fake = createFakeFetch();
  const body = fake.live();
  const { events, handlers } = recordHandlers();
  const disconnect = createHttpSseSurfaceTransport({ baseUrl: base, token: "", tabId: "t1", fetch: fake.fetch, retrySignal: signal().retrySignal }).connect(viewport, handlers);
  await events.until(1);
  disconnect();
  assert.throws(() => body.send("event: state\ndata: {}\n\n"), "the aborted body accepts no more data");
  assert.equal(fake.calls.items.length, 1, "a closed session never reconnects");
  assert.deepEqual(events.items.map((event) => event.type), ["open"]);
});

test("invoke posts method and args and unwraps the result", async () => {
  const fake = createFakeFetch();
  fake.status(200, "{\"result\":{\"current\":\"Work\",\"profiles\":[\"Work\"]}}");
  const transport = createHttpSseSurfaceTransport({ baseUrl: base, token: "secret", tabId: "t1", fetch: fake.fetch, retrySignal: signal().retrySignal });
  const result = await transport.invoke("profiles.use", { name: "Work" });
  assert.deepEqual(result, { current: "Work", profiles: ["Work"] });
  const [call] = fake.calls.items;
  assert.equal(call.url, `${base}/api/surfaces/t1/call`);
  assert.equal(call.init.method, "POST");
  assert.equal(new Headers(call.init.headers).get("Authorization"), "Bearer secret");
  assert.deepEqual(JSON.parse(String(call.init.body)), { method: "profiles.use", args: { name: "Work" } });
});

test("invoke rejects with the reply error", async () => {
  const fake = createFakeFetch();
  fake.status(200, "{\"error\":\"no such element\"}");
  fake.status(409, "{\"ok\":false,\"error\":{\"code\":\"not_a_stream_surface\",\"message\":\"not a stream surface\"}}");
  const transport = createHttpSseSurfaceTransport({ baseUrl: base, token: "", tabId: "t1", fetch: fake.fetch, retrySignal: signal().retrySignal });
  await assert.rejects(transport.invoke("grab.select", { x: 1, y: 2 }), /no such element/);
  await assert.rejects(transport.invoke("grab.cancel"), /not a stream surface/);
  assert.deepEqual(JSON.parse(String(fake.calls.items[1].init.body)), { method: "grab.cancel", args: {} }, "args default to an empty object");
});

for (const peerId of [undefined, "eight"]) {
  test(`default transport delivers page frames and controls on ${peerId ?? "direct daemon"}`, async () => {
    const fake = createFakeFetch();
    const body = fake.live();
    const originalFetch = globalThis.fetch;
    globalThis.fetch = ((url, init) => fake.fetch(String(url), init ?? {})) as typeof fetch;
    configureDaemonTransport({ origin: base, token: `local-${peerId ?? "direct"}` });
    let disconnect: (() => void) | undefined;
    try {
      const { events, handlers } = recordHandlers();
      const transport = createHttpSseSurfaceTransport({ baseUrl: base, token: "unused", tabId: "88a2dfa7", peerId, retrySignal: signal().retrySignal });
      disconnect = transport.connect(viewport, handlers);
      const [stream] = await fake.calls.until(1);
      const url = new URL(stream.url);
      if (peerId) {
        assert.equal(url.pathname, "/api/mesh/relay-stream");
        assert.equal(url.searchParams.get("peer"), peerId);
        assert.equal(url.searchParams.get("path"), "/api/surfaces/88a2dfa7/stream?w=640&h=480&dpr=2");
      } else assert.equal(url.pathname, "/api/surfaces/88a2dfa7/stream");
      assert.equal(new Headers(stream.init.headers).get("Authorization"), `Bearer local-${peerId ?? "direct"}`);
      body.send('event: state\ndata: {"owner":"Prism","url":"https://page.test/"}\n\nevent: frame\ndata: {"data":"QUJD","metadata":{"deviceWidth":640,"deviceHeight":480}}\n\n');
      assert.deepEqual((await events.until(3)).map(event => event.type), ["open", "state", "frame"]);
      const result = { url: "https://next.test/" };
      globalThis.fetch = (async (url, init) => {
        const path = new URL(String(url)).pathname;
        if (peerId) {
          assert.equal(path, "/api/mesh/relay-to");
          assert.deepEqual(JSON.parse(String(init?.body)), {
            target_peer_id: peerId, method: "POST", path: "/api/surfaces/88a2dfa7/call",
            body: { method: "goto", args: { url: result.url } },
          });
        } else {
          assert.equal(path, "/api/surfaces/88a2dfa7/call");
          assert.deepEqual(JSON.parse(String(init?.body)), { method: "goto", args: { url: result.url } });
        }
        return new Response(JSON.stringify(peerId ? { ok: true, status: 200, data: { result } } : { result }), { headers: { "Content-Type": "application/json" } });
      }) as typeof fetch;
      assert.deepEqual(await transport.invoke("goto", { url: result.url }), result);
      body.send(`event: state\ndata: ${JSON.stringify({ owner: "Prism", url: result.url })}\n\n`);
      assert.deepEqual((await events.until(4))[3], { type: "state", value: { owner: "Prism", url: result.url } });
    } finally {
      disconnect?.();
      globalThis.fetch = originalFetch;
    }
  });
}

async function main() {
  let failed = 0;
  for (const [name, fn] of tests) {
    try {
      await fn();
      console.log(`  ✓ ${name}`);
    } catch (error) {
      failed += 1;
      console.error(`  ✗ ${name}: ${error instanceof Error ? error.stack ?? error.message : error}`);
    }
  }
  if (failed > 0) process.exit(1);
  console.log(`
✓ all ${tests.length} httpSseSurfaceTransport tests green`);
}

void main();
