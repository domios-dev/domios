import { afterEach, describe, expect, it, vi } from "vitest";
import { __resetStreamMuxesForTest, OutcomeUnknown, RetryableMuxError, withStreamMux } from "./streamMux";
import { MUX_REQUEST_TIMEOUT, type MuxRequestInit } from "./streamMuxRequestModel";
import type { TransportOptions } from "./transportClient";

interface Posted { id: string; session: string; path: string; method: string; init: RequestInit }

function fixture() {
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
  const client = withStreamMux({ origin: "http://mux.fixture", token: "fixture-token", fetch });
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
