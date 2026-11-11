import { reconcilePageSelection } from "../store/multiplexer/pageSelection";
import { fireResync, onResync } from "./resync";
import { createDelayedMessageFeed } from "../components/Terminal/delayedMessageFeed";
import { act, createElement } from "react";
import { createRoot } from "react-dom/client";
import { __setInvokeForTest, __setListenForTest } from "./bridge";
import { useChatSession } from "../components/Terminal/useChatSession";
import { chatBodyView } from "../components/Terminal/chatBodyVisibility";
import { ChatSetupState } from "../components/Terminal/ChatSetupState";
import { clearTranscriptWindows } from "../components/Terminal/transcriptStore";
import { afterEach, describe, expect, it, vi } from "vitest";
import { __resetStreamMuxesForTest, OutcomeUnknown, RetryableMuxError, withStreamMux } from "./streamMux";
import { MUX_REQUEST_TIMEOUT, readWhenMuxCapacityAvailable, type MuxRequestInit } from "./streamMuxRequestModel";
import type { TransportOptions } from "./transportClient";

interface Posted { id: string; session: string; path: string; method: string; init: RequestInit }

function fixture(onGatewayState?: (available: boolean) => void, origin = "http://mux.fixture") {
  const posted: Posted[] = [], deleted: string[] = [], sessions: string[] = [], credits: Array<{ session: string; received: number }> = [];
  let refuseMux = 0, failCredits = 0;
  const waiting: Array<(post: Posted) => void> = [];
  const cancellations: Array<(id: string) => void> = [];
  let cursor = 0, deleteCursor = 0;
  let controller!: ReadableStreamDefaultController<Uint8Array>;
  let reply: (post: Posted) => Promise<Response> = async () => Response.json({ accepted: true }, { status: 202 });
  const fetch: NonNullable<TransportOptions["fetch"]> = async (input, init = {}) => {
    const url = new URL(String(input));
    if (url.pathname === "/api/stream-mux") {
      sessions.push(url.searchParams.get("session")!);
      if (refuseMux) return new Response("unauthorized", { status: refuseMux });
      return new Response(new ReadableStream<Uint8Array>({ start(value) { controller = value; } }), { headers: { "Content-Type": "text/event-stream" } });
    }
    const parts = url.pathname.split("/");
    if (parts[4] === "credit") {
      credits.push({ session: parts[3], received: (JSON.parse(String(init.body)) as { received: number }).received });
      if (failCredits > 0) { failCredits--; return new Response("busy", { status: 503 }); }
      return new Response(null, { status: 204 });
    }
    if (init.method === "DELETE") {
      deleted.push(parts[5]); cancellations.shift()?.(parts[5]);
      return Response.json({ accepted: true });
    }
    const headers = new Headers(init.headers);
    const post = { id: parts[5], session: parts[3], path: headers.get("X-Domios-Mux-Path")!, method: headers.get("X-Domios-Mux-Method")!, init };
    posted.push(post); waiting.shift()?.(post);
    return reply(post);
  };
  const client = withStreamMux({ origin, token: "fixture-token", fetch, onGatewayState });
  const push = (value: unknown) => controller.enqueue(new TextEncoder().encode(`data: ${JSON.stringify(value)}\n\n`));
  return {
    client, posted, deleted, sessions, credits, push,
    refuse(status: number) { refuseMux = status; },
    failCredits(count: number) { failCredits = count; },
    head(post: Posted, status = 200, headers: [string, string][] = [["content-type", "application/json"]]) { push({ channel: post.id, open: true, status, headers }); },
    text(post: Posted, chunk: string) { push({ channel: post.id, chunk, enc: "utf8" }); },
    end(post: Posted) { push({ channel: post.id, end: { reason: "eof" } }); },
    drop() { controller.error(new Error("mux lost")); },
    reply(value: typeof reply) { reply = value; },
    next(): Promise<Posted> { const post = posted[cursor++]; return post ? Promise.resolve(post) : new Promise(resolve => waiting.push(resolve)); },
    cancelled(): Promise<string> { const id = deleted[deleteCursor++]; return id ? Promise.resolve(id) : new Promise(resolve => cancellations.push(resolve)); },
  };
}

afterEach(() => { __resetStreamMuxesForTest(); vi.useRealTimers(); });

describe("RPC responses over one mux", () => {
  it("registers before posting and delivers a complete response that arrived before its ack", async () => {
    const f = fixture(); let accept!: (response: Response) => void;
    f.reply(() => new Promise(resolve => { accept = resolve; }));
    const response = f.client.localJson("GET", "/api/fixture");
    const post = await f.next();
    f.head(post); f.text(post, '{"snow":"雪"}'); f.end(post);
    accept(Response.json({ accepted: true }, { status: 202 }));
    expect(await response).toEqual({ snow: "雪" });
    expect(f.sessions).toHaveLength(1);
    expect(new Headers(post.init.headers).get("Authorization")).toBe("Bearer fixture-token");
  });

  it("preserves status, headers, startup refusal bodies, and exact binary bytes", async () => {
    const f = fixture();
    const response = f.client.request!("/api/fixture"); const post = await f.next();
    f.head(post, 503, [["content-type", "application/json"], ["etag", "version"]]);
    f.text(post, '{"detail":"cold"}'); f.end(post);
    const received = await response;
    expect(received.status).toBe(503); expect(received.headers.get("etag")).toBe("version");
    expect(await received.json()).toEqual({ detail: "cold" });
    const binary = f.client.request!("/api/binary"); const next = await f.next();
    f.head(next, 200, [["content-type", "application/octet-stream"]]);
    f.push({ channel: next.id, chunk: "AP8=", enc: "base64" }); f.end(next);
    expect([...new Uint8Array(await (await binary).arrayBuffer())]).toEqual([0, 255]);
    expect(f.sessions).toHaveLength(1);
  });

  it("classifies an ack network failure as mutation uncertainty without any carrier replay", async () => {
    const f = fixture(); f.reply(async () => { throw new TypeError("ack lost"); });
    await expect(f.client.localJson("POST", "/api/tabs/t/chat-send", { client_token: "own" })).rejects.toBeInstanceOf(OutcomeUnknown);
    expect(f.posted).toHaveLength(1);
  });

  it.each(["GET", "POST"])("fails an acknowledged %s on session loss without reposting it", async method => {
    const f = fixture();
    const response = f.client.request!("/api/fixture", { method }); const post = await f.next();
    f.head(post); const received = await response;
    const body = received.text();
    const rejected = expect(body).rejects.toBeInstanceOf(method === "GET" ? RetryableMuxError : OutcomeUnknown);
    f.drop(); await rejected; expect(f.posted).toHaveLength(1);
  });

  it.each(["GET", "POST"])("cancels or detaches %s after its original POST settles", async method => {
    const f = fixture(), aborter = new AbortController();
    const response = f.client.request!("/api/fixture", { method, signal: aborter.signal }); const post = await f.next();
    f.head(post); const received = await response;
    const body = received.text();
    const rejected = method === "GET" ? expect(body).rejects.toMatchObject({ name: "AbortError" }) : expect(body).rejects.toBeInstanceOf(OutcomeUnknown);
    aborter.abort(); await rejected;
    expect(await f.cancelled()).toBe(post.id); expect(f.posted).toHaveLength(1);
    expect(new Headers(post.init.headers).get("X-Domios-Mux-Cancellable")).toBe(method === "GET" ? "1" : null);
  });

  it("reposts a proven 410 once after reopening, for exactly one mutation dispatch", async () => {
    const f = fixture(); let dispatched = 0;
    f.reply(async post => {
      if (f.posted.length === 1) return new Response("closed", { status: 410 });
      dispatched++; f.head(post); f.text(post, '{"ok":true}'); f.end(post);
      return Response.json({ accepted: true }, { status: 202 });
    });
    expect(await f.client.localJson("POST", "/api/tabs/t/chat-send", { client_token: "own" })).toEqual({ ok: true });
    expect(f.posted).toHaveLength(2); expect(dispatched).toBe(1); expect(f.sessions).toHaveLength(2);
    expect(f.posted[0].session).not.toBe(f.posted[1].session);
    expect(f.posted[0].init.body).toBe(f.posted[1].init.body);
  });

  it("keeps a non-stream deadline active after the response head until end", async () => {
    vi.useFakeTimers(); const f = fixture();
    const init: MuxRequestInit = { [MUX_REQUEST_TIMEOUT]: 10 };
    const response = f.client.request!("/api/fixture", init); const post = await f.next();
    f.head(post); const received = await response;
    const rejected = expect(received.text()).rejects.toMatchObject({ name: "TimeoutError" });
    await vi.advanceTimersByTimeAsync(10); await rejected;
    expect(await f.cancelled()).toBe(post.id);
  });
});

describe("mux bulk credit and credential refusal", () => {
  it("credits received bulk bytes coarsely and cumulatively, including frames for requests it no longer tracks", async () => {
    const f = fixture();
    const response = f.client.request!("/api/fixture"); const post = await f.next();
    f.head(post, 200, [["content-type", "application/octet-stream"]]);
    const cost = 64 * 1024;
    for (let i = 0; i < 6; i++) f.push({ channel: post.id, chunk: "AA==", enc: "base64", credit: cost });
    for (let i = 0; i < 2; i++) f.push({ channel: "a-cancelled-request", chunk: "AA==", enc: "base64", credit: cost });
    f.push({ channel: post.id, chunk: "AA==", enc: "base64" });
    f.end(post);
    await (await response).arrayBuffer();
    await vi.waitFor(() => expect(f.credits.map(c => c.received)).toEqual([4 * cost, 8 * cost]));
    expect(new Set(f.credits.map(c => c.session))).toEqual(new Set([f.sessions[0]]));
  });

  it("re-posts a refused credit without waiting for another frame, so a closed window cannot wedge", async () => {
    const f = fixture();
    f.failCredits(1);
    const response = f.client.request!("/api/fixture"); const post = await f.next();
    f.head(post, 200, [["content-type", "application/octet-stream"]]);
    const cost = 64 * 1024;
    for (let i = 0; i < 4; i++) f.push({ channel: post.id, chunk: "AA==", enc: "base64", credit: cost });
    await vi.waitFor(() => expect(f.credits.map(c => c.received)).toEqual([4 * cost, 4 * cost]));
    f.end(post);
    await (await response).arrayBuffer();
  });

  it("fails waiting requests with the 401 instead of reconnecting forever", async () => {
    const f = fixture();
    f.refuse(401);
    await expect(f.client.localJson("GET", "/api/workspaces")).rejects.toMatchObject({ name: "WebApiError", status: 401 });
    expect(f.sessions).toHaveLength(1);
    expect(f.posted).toHaveLength(0);
  });
});


it("attaches a declared large snapshot received before acknowledgement and repeats overflow resync safely", async () => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const f = fixture();
  let accept!: (response: Response) => void;
  f.reply(post => post.path.endsWith("/chat-raw/start") ? new Promise(resolve => { accept = resolve; })
    : Promise.resolve(Response.json({ accepted: true }, { status: 202 })));
  __setListenForTest(async () => () => {});
  __setInvokeForTest((async (command: string, args: Record<string, unknown>) => command === "chat_start"
    ? f.client.localJson("POST", "/api/tabs/large/chat-raw/start", args) : []) as never);
  clearTranscriptWindows();
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  let latest: ReturnType<typeof useChatSession> | undefined;
  let failed = false;
  function Probe() {
    latest = useChatSession({ tabId: "large", agentSessionId: "own", sessionBindingNonce: "nonce", active: true });
    failed ||= latest.attachmentState === "failed";
    const view = chatBodyView({ snapshotReady: latest.snapshotReady, hasTranscript: latest.allMessages.length > 0,
      chatLoading: latest.loading, attachmentError: latest.attachmentState === "failed" ? latest.error : null,
      unavailable: latest.transcriptUnavailable, bootView: { kind: "none" } });
    return createElement(ChatSetupState, { tab: null, providerName: "Fixture", view, onRetry: latest.retry });
  }
  try {
    await act(async () => root.render(createElement(Probe)));
    const content = "x".repeat(820_000);
    for (let round = 0; round < 2; round++) {
      const post = await f.next();
      const body = JSON.stringify({ source_session_id: "own", session_binding_nonce: "nonce", subscription_id: JSON.parse(String(post.init.body)).subscriptionId,
        messages: [{ id: "answer", type: "assistant", content }], text: "", start_offset: 0, offset: content.length, pending: false });
      f.head(post, 200, [["content-type", "application/json"], ["content-length", String(new TextEncoder().encode(body).byteLength)]]);
      for (let index = 0; index < body.length; index += 16 * 1024) f.text(post, body.slice(index, index + 16 * 1024));
      f.end(post);
      const barrier = f.client.localJson("GET", "/api/fixture/barrier");
      const marker = await f.next();
      f.head(marker); f.text(marker, "{}"); f.end(marker);
      await act(async () => { await barrier; accept(Response.json({ accepted: true }, { status: 202 })); });
      expect(latest?.attachmentState).toBe("attached");
      expect(latest?.allMessages[0]?.content.length).toBe(820_000);
      expect(failed).toBe(false);
      expect(host.querySelector('[aria-label="Chat connection failed"]')).toBeNull();
      if (round === 0) {
        const { fireResync } = await import("./resync");
        await act(async () => fireResync("overflow"));
      }
    }
  } finally {
    await act(async () => root.unmount()); host.remove();
    __setInvokeForTest(null); __setListenForTest(null); clearTranscriptWindows();
  }
});

it("classifies chat_start acknowledgement loss as a retryable read, including a relayed read", async () => {
  const f = fixture(); f.reply(async () => { throw new TypeError("ack lost"); });
  await expect(f.client.localJson("POST", "/api/tabs/t/chat-raw/start", {})).rejects.toBeInstanceOf(RetryableMuxError);
  await expect(f.client.localJson("POST", "/api/mesh/relay-to", { method: "POST", path: "/api/tabs/t/chat-raw/start", body: {} })).rejects.toBeInstanceOf(RetryableMuxError);
});
it("reports the overflowing request's bytes and limit instead of the aggregate session total", async () => {
  const f = fixture();
  const other = f.client.request!("/api/paused"); const paused = await f.next();
  f.head(paused); const unread = await other;
  for (let index = 0; index < 4; index++) f.text(paused, "p".repeat(64 * 1024));
  const response = f.client.request!("/api/fixture"); const post = await f.next();
  f.head(post); const received = await response;
  for (let index = 0; index < 9; index++) f.text(post, "x".repeat(64 * 1024));
  const barrier = f.client.localJson("GET", "/api/fixture/barrier"); const marker = await f.next();
  f.head(marker); f.text(marker, "{}"); f.end(marker); await barrier;
  await expect(received.text()).rejects.toThrow(/per-request.*589824.*524288/);
  await unread.body?.cancel();
});
it("keeps the aggregate buffer bounded even when a response advertises a larger body", async () => {
  const f = fixture();
  const response = f.client.request!("/api/fixture"); const post = await f.next();
  f.head(post, 200, [["content-type", "application/json"], ["content-length", String(8 * 1024 * 1024)]]);
  const received = await response;
  for (let index = 0; index < 65; index++) f.text(post, "x".repeat(64 * 1024));
  const barrier = f.client.localJson("GET", "/api/fixture/barrier"); const marker = await f.next();
  f.head(marker); f.text(marker, "{}"); f.end(marker); await barrier;
  await expect(received.text()).rejects.toThrow(/session.*4259840.*4194304/);
});


it("coalesces the two composer feeds across a wake burst instead of filling all 64 RPC slots", async () => {
  const f = fixture();
  f.reply(async post => {
    if (post.path.endsWith("/stop")) { f.head(post); f.text(post, '{"ok":true}'); f.end(post); }
    return Response.json({ accepted: true }, { status: 202 });
  });
  let loads = 0;
  const feeds = Array.from({ length: 16 }, (_, index) => createDelayedMessageFeed(`tab-${index >> 1}`, {
    load: (tab, signal?: AbortSignal) => { loads++; return f.client.localJson("GET", `/api/tabs/${tab}/${index % 2 ? "system-heartbeats" : "delayed-messages"}`, undefined, { signal }); },
    publish: () => {}, subscribe: async () => () => {},
  }));
  try {
    for (let index = 0; index < 16; index++) await f.next();
    for (let event = 0; event < 3; event++) for (const feed of feeds) void feed.wake();
    expect(loads).toBe(16);
    await expect(f.client.localJson("POST", "/api/tabs/active/stop")).resolves.toEqual({ ok: true });
  } finally { for (const feed of feeds) feed.dispose(); }
});

it("reserves admission for foreground chat_start and user actions under a background-read flood", async () => {
  const f = fixture();
  f.reply(async post => {
    if (post.method !== "GET") { f.head(post); f.text(post, '{"ok":true}'); f.end(post); }
    return Response.json({ accepted: true }, { status: 202 });
  });
  const reads = Array.from({ length: 64 }, (_, index) => f.client.localJson("GET", `/api/tabs/t-${index}/delayed-messages`).catch(error => error));
  await f.next();
  for (const [method, path] of [["POST", "chat-raw/start"], ["POST", "stop"], ["POST", "chat-send"], ["POST", "prompt/respond"], ["POST", "approve"], ["DELETE", "close"]]) {
    await expect(f.client.localJson(method, `/api/tabs/active/${path}`, {})).resolves.toEqual({ ok: true });
  }
  expect(reads).toHaveLength(64);
});

it("keeps a carrier POST slot for Stop before background acknowledgements return", async () => {
  const f = fixture();
  const accepts: Array<(response: Response) => void> = [];
  f.reply(post => {
    if (post.path.endsWith("/stop")) { f.head(post); f.text(post, '{}'); f.end(post); return Promise.resolve(Response.json({ accepted: true }, { status: 202 })); }
    return new Promise(resolve => accepts.push(resolve));
  });
  for (let index = 0; index < 4; index++) void f.client.localJson("GET", `/api/read-${index}`).catch(() => {});
  try {
    for (let index = 0; index < 3; index++) await f.next();
    const stop = f.client.localJson("POST", "/api/tabs/active/stop").catch(error => error);
    expect((await f.next()).path).toBe("/api/tabs/active/stop");
    expect(await stop).toEqual({});
  } finally { for (const accept of accepts) accept(Response.json({ accepted: true }, { status: 202 })); }
});

it("queues an action at the 64-request wire limit without replaying any posted mutation", async () => {
  const f = fixture();
  f.reply(async post => {
    if (post.path.endsWith("/stop")) { f.head(post); f.text(post, '{"ok":true}'); f.end(post); }
    return Response.json({ accepted: true }, { status: 202 });
  });
  for (let index = 0; index < 64; index++) void f.client.localJson("POST", `/api/tabs/t-${index}/send`, { token: index }).catch(() => {});
  let first!: Posted;
  for (let index = 0; index < 64; index++) { const post = await f.next(); if (index === 0) first = post; }
  const stop = f.client.localJson("POST", "/api/tabs/active/stop").catch(error => error);
  f.head(first); f.text(first, '{}'); f.end(first);
  expect(await stop).toEqual({ ok: true });
  expect(f.posted.filter(post => post.path.endsWith("/stop"))).toHaveLength(1);
  expect(f.posted.filter(post => post.path.endsWith("/send"))).toHaveLength(64);
});

it("does not declare the healthy browser gateway unavailable for a local admission refusal", async () => {
  const gateway = vi.fn(); const f = fixture(gateway);
  for (let index = 0; index < 64; index++) void f.client.localJson("GET", `/api/read-${index}`).catch(() => {});
  await f.next();
  await expect(f.client.localJson("GET", "/api/one-more-read")).rejects.toBeInstanceOf(RetryableMuxError);
  expect(gateway.mock.calls.some(([available]) => available === false)).toBe(false);
});

it.each([200, 503])("reissues an opted-in, never-dispatched read once on released capacity; dispatched HTTP %s settles", async (status) => {
  const f = fixture();
  const attempts = vi.fn(() => f.client.localJson("GET", "/api/mesh/peers"));
  f.reply(async post => {
    if (post.path === "/api/mesh/peers") {
      f.head(post, status); f.text(post, '{"paired":[]}'); f.end(post);
    }
    return Response.json({ accepted: true }, { status: 202 });
  });
  for (let index = 0; index < 48; index++) void f.client.localJson("GET", `/api/read-${index}`).catch(() => {});
  const first = await f.next();
  const result = readWhenMuxCapacityAvailable(attempts, new AbortController().signal).catch(error => error);
  expect(attempts).toHaveBeenCalledTimes(1);
  expect(f.posted.some(post => post.path === "/api/mesh/peers")).toBe(false);
  f.head(first); f.text(first, '{}'); f.end(first);
  const settled = await result;
  if (status === 200) expect(settled).toEqual({paired:[]});
  else expect(settled).toMatchObject({status:503});
  expect(attempts).toHaveBeenCalledTimes(2);
  expect(f.posted.filter(post => post.path === "/api/mesh/peers")).toHaveLength(1);
});

it("expires an undispatched queued action as safe to retry without later sending it", async () => {
  vi.useFakeTimers(); const f = fixture();
  f.reply(async post => {
    if (post.path.endsWith("/witness")) { f.head(post); f.text(post, '{}'); f.end(post); }
    return Response.json({ accepted: true }, { status: 202 });
  });
  for (let index = 0; index < 64; index++) void f.client.localJson("POST", `/api/tabs/t-${index}/send`, {}).catch(() => {});
  let first!: Posted;
  for (let index = 0; index < 64; index++) { const post = await f.next(); if (!index) first = post; }
  const expired = f.client.localJson("POST", "/api/tabs/active/stop", {}, { timeoutMs: 10 }).catch(error => error);
  await vi.advanceTimersByTimeAsync(10);
  expect(await expired).toMatchObject({ code: "client_capacity", dispatched: false });
  f.head(first); f.text(first, '{}'); f.end(first);
  await expect(f.client.localJson("POST", "/api/witness", {})).resolves.toEqual({});
  expect(f.posted.some(post => post.path.endsWith("/stop"))).toBe(false);
});

it("keeps a passive browser single-flight while another browser switches shared selection and causes resync", async () => {
  // Two isolated browser transports reach one shared selection model through separate origin aliases.
  const a = fixture(undefined, "http://client-a.fixture"), b = fixture(undefined, "http://client-b.fixture");
  const workspaces = [{ id: "w", tabs: ["own", "other"] }];
  let selection = { active_workspace_id: "w", focused_tab_id: "own", activeTabIds: { w: "own" } };
  let passive = selection;
  const server = { select(tab: string) {
    selection = { ...selection, focused_tab_id: tab, activeTabIds: { w: tab } };
    passive = reconcilePageSelection(passive, selection, workspaces, true) as typeof passive;
  } };
  a.reply(async post => {
    server.select(JSON.parse(String(post.init.body)).tab);
    a.head(post); a.text(post, '{}'); a.end(post);
    return Response.json({ accepted: true }, { status: 202 });
  });
  let pending = 0, maximum = 0, loads = 0;
  const feeds = ["delayed-messages", "system-heartbeats"].map(path => createDelayedMessageFeed("own", {
    load: async (tab, signal) => {
      loads++; pending++; maximum = Math.max(maximum, pending);
      try { return await b.client.localJson("GET", `/api/tabs/${tab}/${path}`, undefined, { signal }); }
      finally { pending--; }
    }, publish: () => {}, subscribe: async () => () => {},
  }));
  const stopResync = onResync(() => { for (const feed of feeds) void feed.wake(); });
  try {
    const first = [await b.next(), await b.next()];
    for (let index = 0; index < 32; index++) {
      await a.client.localJson("POST", "/api/shared-selection", { tab: index % 2 ? "own" : "other" });
      fireResync(index % 2 ? "overflow" : "wake");
      for (const feed of feeds) void feed.wake();
    }
    expect(passive.focused_tab_id).toBe("own");
    expect(b.posted).toHaveLength(2); expect(loads).toBe(2); expect(maximum).toBe(2);
    for (const post of first) { b.head(post); b.text(post, '[]'); b.end(post); }
    await b.next(); await b.next();
    expect(loads).toBe(4); expect(pending).toBe(2); expect(maximum).toBe(2);
  } finally { stopResync(); for (const feed of feeds) feed.dispose(); }
});
