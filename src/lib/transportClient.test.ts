import { describe, expect, it, vi } from "vitest";
import { createTransportClient, decodeTransportResponse, parseSseFrame, TransportError, InvalidJsonResponseError } from "./transportClient";

const origin = "http://127.0.0.1:7686";
function clientWith(response: Response) {
  const fetch = vi.fn<typeof globalThis.fetch>().mockResolvedValue(response);
  return { client: createTransportClient({ origin, token: () => "secret", fetch }), fetch };
}

describe("transportClient", () => {
  it("requires an explicit HTTP origin and confines credentials to it", async () => {
    expect(() => createTransportClient({ origin: "tauri://localhost", token: () => "x" })).toThrow();
    const { client, fetch } = clientWith(Response.json({}));
    await expect(client.json("GET", "//foreign/api")).rejects.toThrow();
    await expect(client.json("GET", "/\\foreign/api")).rejects.toThrow();
    expect(fetch).not.toHaveBeenCalled();
    expect(client.origin()).toBe(origin);
    expect(client.currentToken()).toBe("secret");
  });
  it("uses bearer HTTP from an explicit origin and preserves envelopes", async () => {
    const { client, fetch } = clientWith(Response.json({ ok: true, result: { id: "tab" } }));
    expect(await client.json("POST", "/api/tabs", { nonce: "binding" })).toEqual({ ok: true, result: { id: "tab" } });
    const [url, init] = fetch.mock.calls[0];
    expect(url).toBe(`${origin}/api/tabs`);
    expect(new Headers(init?.headers).get("Authorization")).toBe("Bearer secret");
    expect(init?.credentials).toBe("omit");
    expect(init?.body).toBe('{"nonce":"binding"}');
  });
  it("sends caller identity as a routing header and refreshes tokens", async () => {
    let token = "first";
    const fetch = vi.fn<typeof globalThis.fetch>().mockImplementation(async () => Response.json({}));
    const client = createTransportClient({ origin, token: () => token, fetch });
    await client.json("GET", "/api/tabs", undefined, { callerTabId: "mesh:peer/tab" });
    token = "second";
    await client.localJson("GET", "/api/config", undefined, { callerTabId: "tab" });
    expect(new Headers(fetch.mock.calls[0][1]?.headers).get("X-CodeTerm-Tab-Id")).toBe("mesh:peer/tab");
    expect(new Headers(fetch.mock.calls[1][1]?.headers).get("Authorization")).toBe("Bearer second");
  });
  it("preserves bare JSON results", () => {
    expect(decodeTransportResponse(200, "/api/value", '[1,2]', "application/json; charset=utf-8")).toEqual([1, 2]);
  });
  it("rejects a static SPA response before config.font.size can be read", async () => {
    const html = '<!doctype html><div id="root"></div>';
    for (const [body, contentType] of [[html, "text/html"], [html, "application/json"], ['{"font":{"size":14}}', "text/plain"], ["{}", null]]) {
      expect(() => decodeTransportResponse(200, "/api/config", body!, contentType!)).toThrow(InvalidJsonResponseError);
    }
    const { client } = clientWith(new Response(html, { headers: { "content-type": "text/html" } }));
    await expect(client.json("GET", "/api/config")).rejects.toMatchObject({
      name: "InvalidJsonResponseError", status: 200, path: "/api/config", body: html, contentType: "text/html",
    });
    const local = clientWith(new Response(html, { headers: { "content-type": "text/html" } })).client;
    await expect(local.localJson("GET", "/api/config")).rejects.toBeInstanceOf(InvalidJsonResponseError);
  });
  it("retains error status, path and exact reason body", () => {
    try { decodeTransportResponse(403, "/api/teams", '{"reason":"caller_denied"}', "application/json"); }
    catch (error) {
      expect(error).toBeInstanceOf(TransportError);
      expect(error).toMatchObject({ status: 403, path: "/api/teams", body: '{"reason":"caller_denied"}' });
      return;
    }
    throw new Error("Expected authorization refusal");
  });
  it("returns only the existing prompt-not-on-screen 409 exception", async () => {
    const body = '{"ok":false,"reason":"prompt_not_on_screen"}';
    expect(decodeTransportResponse(409, "/api/tabs/t/prompt/respond", body, "application/json")).toEqual(JSON.parse(body));
    expect(() => decodeTransportResponse(409, "/api/other", body, "application/json")).toThrow(TransportError);
    const { client } = clientWith(new Response(body, { status: 409, headers: { "content-type": "application/json" } }));
    await expect(client.localJson("POST", "/api/tabs/t/prompt/respond")).rejects.toThrow(TransportError);
  });
  it("relays peer requests with the local token and exact inner status", async () => {
    const fetch = vi.fn<typeof globalThis.fetch>().mockResolvedValue(Response.json({ ok: false, status: 404, data: { reason: "missing" }, error: "missing" }));
    const client = createTransportClient({ origin, token: () => "local", relayPeerId: "peer", fetch });
    await expect(client.json("GET", "/api/tabs/remote")).rejects.toMatchObject({ status: 404, body: '{"reason":"missing"}' });
    expect(JSON.parse(String(fetch.mock.calls[0][1]?.body))).toEqual({ target_peer_id: "peer", method: "GET", path: "/api/tabs/remote", body: null });
    expect(new Headers(fetch.mock.calls[0][1]?.headers).get("Authorization")).toBe("Bearer local");
  });
  it("uploads bytes locally without assigning JSON multipart headers", async () => {
    const { client, fetch } = clientWith(Response.json({ path: "/saved", size: 3 }));
    const form = new FormData();
    form.append("file", new Blob(["abc"]), "a.txt");
    expect(await client.upload("/api/attachments/upload", form)).toEqual({ path: "/saved", size: 3 });
    expect(fetch.mock.calls[0][1]?.body).toBe(form);
    expect(new Headers(fetch.mock.calls[0][1]?.headers).has("Content-Type")).toBe(false);
  });
  it("fails closed when no bearer token exists", async () => {
    const fetch = vi.fn<typeof globalThis.fetch>();
    const client = createTransportClient({ origin, token: () => null, fetch });
    await expect(client.json("GET", "/api/config")).rejects.toThrow("Daemon bearer token required");
    expect(fetch).not.toHaveBeenCalled();
  });
  it("signals readiness for an accepted quiet stream before any event arrives", async () => {
    let connected!: () => void;
    const ready = new Promise<void>((resolve) => { connected = resolve; });
    const { client } = clientWith(new Response(new ReadableStream<Uint8Array>()));
    const handler = vi.fn();
    const stop = client.subscribe("/api/events/stream", handler, { onReady: connected });
    try {
      await ready;
      expect(handler).not.toHaveBeenCalled();
    } finally { stop(); }
  });
  it("reads fragmented CRLF SSE and unsubscribe cancels the reader", async () => {
    let controller!: ReadableStreamDefaultController<Uint8Array>;
    let delivered!: () => void;
    const event = new Promise<void>((resolve) => { delivered = resolve; });
    const cancel = vi.fn();
    const stream = new ReadableStream<Uint8Array>({ start(c) { controller = c; }, cancel });
    const { client, fetch } = clientWith(new Response(stream));
    const handler = vi.fn(() => delivered());
    const signal = new AbortController();
    const unsubscribe = client.subscribe("/api/events/stream?since=boot%3A1", handler, { signal: signal.signal });
    const encoder = new TextEncoder();
    controller.enqueue(encoder.encode(': heartbeat\r\n\r\ndata: {"event_type":"chat","data":'));
    controller.enqueue(encoder.encode('{"nonce":"exact"}}\r\n\r\n'));
    await event;
    expect(handler).toHaveBeenCalledExactlyOnceWith("chat", { nonce: "exact" });
    unsubscribe();
    expect((fetch.mock.calls[0][1]?.signal as AbortSignal).aborted).toBe(true);
    expect(cancel).toHaveBeenCalledOnce();
  });
  it("identifies all streams from a page with the same client header", async () => {
    const requests: string[] = [];
    let resolve!: () => void;
    const accepted = new Promise<void>((done) => { resolve = done; });
    const fetch = vi.fn<typeof globalThis.fetch>(async (_url, init) => {
      requests.push(new Headers(init?.headers).get("X-CodeTerm-Client-Id")!);
      if (requests.length === 2) resolve();
      return new Response(new ReadableStream());
    });
    const first = createTransportClient({ origin, token: "token", fetch });
    const second = createTransportClient({ origin, token: "token", fetch });
    const stops = [first.subscribe("/api/events/stream", () => {}), second.subscribe("/api/output/stream", () => {})];
    await accepted;
    expect(requests[0]).toMatch(/^[a-f0-9-]{36}$/);
    expect(requests[1]).toBe(requests[0]);
    stops.forEach((stop) => stop());
  });
  it("parses named multiline events and ignores comments", () => {
    expect(parseSseFrame(":keepalive")).toBeNull();
    expect(parseSseFrame("event: output\nid: b:2\ndata: first\ndata: second")).toEqual({ event: "output", id: "b:2", data: "first\nsecond" });
  });
});


describe("SSE lifecycle", () => {
  it.each([200, 400])("reports the end once with status %s", async (status) => {
    const { client } = clientWith(status === 200 ? new Response(new ReadableStream({ start(c) { c.close(); } })) : new Response("", { status }));
    let resolve!: (end: unknown) => void;
    const ended = new Promise((done) => { resolve = done; });
    const onEnd = vi.fn(resolve);
    client.subscribe("/api/events/stream", () => {}, { onEnd });
    expect(await ended).toMatchObject(status === 200 ? { reason: "eof" } : { reason: "http", status: 400 });
    expect(onEnd).toHaveBeenCalledOnce();
  });
  it("reports a network failure with the original error", async () => {
    const error = new Error("offline");
    const client = createTransportClient({ origin, token: () => "token", fetch: vi.fn().mockRejectedValue(error) });
    let resolve!: (end: unknown) => void;
    const ended = new Promise((done) => { resolve = done; });
    const onEnd = vi.fn(resolve);
    client.subscribe("/api/events/stream", () => {}, { onEnd });
    expect(await ended).toEqual({ reason: "error", error });
    expect(onEnd).toHaveBeenCalledOnce();
  });
  it("does not deliver events or an end after caller cleanup", async () => {
    let release!: (response: Response) => void;
    let canceled!: () => void;
    const cancelled = new Promise<void>((resolve) => { canceled = resolve; });
    const fetch = vi.fn<typeof globalThis.fetch>(() => new Promise((resolve) => { release = resolve; }));
    const client = createTransportClient({ origin, token: () => "token", fetch });
    const onEnd = vi.fn();
    const onEvent = vi.fn();
    const cleanup = client.subscribe("/api/events/stream", onEvent, { onEnd });
    cleanup();
    release(new Response(new ReadableStream({ cancel() { canceled(); } })));
    await cancelled;
    expect(onEnd).not.toHaveBeenCalled();
    expect(onEvent).not.toHaveBeenCalled();
  });
  it("reports external abort to its owning generation", async () => {
    const { client } = clientWith(new Response(new ReadableStream()));
    const abort = new AbortController();
    let resolve!: (end: unknown) => void;
    const ended = new Promise((done) => { resolve = done; });
    client.subscribe("/api/events/stream", () => {}, { signal: abort.signal, onEnd: resolve });
    abort.abort("superseded");
    expect(await ended).toEqual({ reason: "abort", error: "superseded" });
  });
});


describe("SSE raw envelope and activity options", () => {
  it("retains cursor metadata and counts heartbeat activity", async () => {
    let controller!: ReadableStreamDefaultController<Uint8Array>;
    let received!: () => void;
    const delivered = new Promise<void>((resolve) => { received = resolve; });
    const { client, fetch } = clientWith(new Response(new ReadableStream({ start(c) { controller = c; } })));
    const envelope = { event_type: "tab", data: { id: "tab" }, boot: "boot", seq: 7 };
    const handler = vi.fn(() => received());
    const activity = vi.fn();
    const stop = client.subscribe("/api/events/stream", handler, { raw: true, onActivity: activity });
    controller.enqueue(new TextEncoder().encode(": heartbeat\n\n"));
    controller.enqueue(new TextEncoder().encode(`data: ${JSON.stringify(envelope)}\n\n`));
    await delivered;
    expect(handler).toHaveBeenCalledExactlyOnceWith("message", envelope);
    expect(activity).toHaveBeenCalledTimes(2);
    expect(new Headers(fetch.mock.calls[0][1]?.headers).get("Authorization")).toBe("Bearer secret");
    stop();
  });
});
