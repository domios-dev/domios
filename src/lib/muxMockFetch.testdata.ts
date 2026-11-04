import type { TransportOptions } from "./transportClient";
import { muxRequestPolicy } from "./streamMuxRequestModel";
import { __streamMuxesBusyForTest, __streamMuxesIdleForTest } from "./streamMux";

type FetchPort = NonNullable<TransportOptions["fetch"]>;

// Sessions outlive one fake, so a test can swap daemon fakes under an already open mux.
const sessions = new Map<string, ReadableStreamDefaultController<Uint8Array>>();

// Test fakes are loosely typed (string urls, Response-like results); the mux hands them plain hrefs.
// biome-ignore lint/suspicious/noExplicitAny: test fake signature
export type DaemonFake = (input: any, init?: any) => unknown;

interface ResponseLike { status?: number; ok?: boolean; headers?: HeadersInit; text?: () => Promise<string>; json?: () => Promise<unknown> }

async function asResponse(value: unknown): Promise<Response> {
  if (value instanceof Response) return value;
  const like = value as ResponseLike;
  // Response-like fakes answer success through json() and failures through text().
  const body = like.json && like.ok !== false ? JSON.stringify(await like.json()) : like.text ? await like.text() : JSON.stringify(await like.json?.());
  return new Response(body, { status: like.status ?? (like.ok === false ? 500 : 200), headers: like.headers ?? { "Content-Type": "application/json" } });
}

export function muxMockFetch(domain: DaemonFake): FetchPort {
  const jobs = new Map<string, { aborter: AbortController; detached: boolean; cancellable: boolean; reader?: ReadableStreamDefaultReader<Uint8Array> }>();
  const encoder = new TextEncoder();
  function emit(session: string, frame: unknown) {
    try { sessions.get(session)?.enqueue(encoder.encode(`data: ${JSON.stringify(frame)}\n\n`)); } catch {}
  }
  const carrier: FetchPort = async (input, init = {}) => {
    const url = new URL(String(input));
    if (url.pathname === "/api/stream-mux") {
      const session = url.searchParams.get("session")!;
      const body = new ReadableStream<Uint8Array>({
        start(controller) {
          sessions.set(session, controller);
          init.signal?.addEventListener("abort", () => { sessions.delete(session); try { controller.close(); } catch {} }, { once: true });
        },
        cancel() { sessions.delete(session); },
      });
      return new Response(body, { headers: { "Content-Type": "text/event-stream" } });
    }
    const parts = url.pathname.split("/");
    if (parts[2] !== "stream-mux") throw new Error("UI request bypassed the mux");
    const session = parts[3];
    if (init.method === "DELETE") {
      const job = jobs.get(parts[5]);
      if (job) { job.detached = true; if (job.cancellable) { job.aborter.abort(); void job.reader?.cancel().catch(() => {}); } }
      return Response.json({ accepted: true });
    }
    const headers = new Headers(init.headers);
    const stream = parts[4] === "channels";
    const channel = stream ? JSON.parse(String(init.body)) as { channel: string; path: string } : null;
    const id = channel?.channel ?? parts[5];
    const method = stream ? "GET" : headers.get("X-Domios-Mux-Method")!;
    const path = channel?.path ?? headers.get("X-Domios-Mux-Path")!;
    for (const key of [...headers.keys()]) if (key.startsWith("x-domios-mux-")) headers.delete(key);
    const job: { aborter: AbortController; detached: boolean; cancellable: boolean; reader?: ReadableStreamDefaultReader<Uint8Array> } = { aborter: new AbortController(), detached: false, cancellable: stream || muxRequestPolicy(method, path, init.body).cancellable };
    jobs.set(id, job);
    void (async () => {
      try {
        const response = await asResponse(await domain(new URL(path, url.origin).href, { ...init, method, headers, body: stream ? undefined : init.body, signal: job.aborter.signal }));
        if (job.detached) return;
        emit(session, { channel: id, open: true, status: response.status, headers: [...response.headers.entries()] });
        const reader = response.body?.getReader();
        job.reader = reader;
        if (reader) {
          try {
            for (;;) {
              const next = await reader.read(); if (next.done || job.detached) break;
              for (let start = 0; start < next.value.length; start += 8192) {
                emit(session, { channel: id, chunk: btoa(String.fromCharCode(...next.value.subarray(start, start + 8192))), enc: "base64" });
              }
            }
          } finally { await reader.cancel().catch(() => {}); reader.releaseLock(); }
        }
        if (!job.detached) emit(session, { channel: id, end: { reason: "eof" } });
      } catch (error) {
        if (!job.detached) emit(session, { channel: id, end: { reason: "error", message: String(error) } });
      } finally { jobs.delete(id); }
    })();
    return Response.json({ accepted: true }, { status: 202 });
  };
  // Tests that read the global fetch as their spy see the domain fake's mock state.
  return new Proxy(carrier, {
    get: (target, key) => (key in domain && !(key in Function.prototype) ? Reflect.get(domain, key) : Reflect.get(target, key)),
    has: (target, key) => key in domain || key in target,
  });
}

/**
 * Settles the UI against the fake daemon: waits until the carrier has no request in flight, then lets the
 * callers' promise continuations run (setImmediate follows the microtask queue), and repeats while they
 * issued follow-up requests. Every wait is a carrier acknowledgement, never a guessed number of turns.
 */
export async function settleDaemon(): Promise<void> {
  do {
    await __streamMuxesIdleForTest();
    await new Promise(resolve => setImmediate(resolve));
  } while (__streamMuxesBusyForTest());
}
