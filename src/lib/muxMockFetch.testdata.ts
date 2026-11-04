import type { TransportOptions } from "./transportClient";
import { muxRequestPolicy } from "./streamMuxRequestModel";

type FetchPort = NonNullable<TransportOptions["fetch"]>;

// Sessions outlive one fake, so a test can swap daemon fakes under an already open mux.
const sessions = new Map<string, ReadableStreamDefaultController<Uint8Array>>();

export function muxMockFetch(domain: FetchPort): FetchPort {
  const jobs = new Map<string, { aborter: AbortController; detached: boolean; cancellable: boolean }>();
  const encoder = new TextEncoder();
  function emit(session: string, frame: unknown) {
    try { sessions.get(session)?.enqueue(encoder.encode(`data: ${JSON.stringify(frame)}\n\n`)); } catch {}
  }
  return async (input, init = {}) => {
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
      if (job) { job.detached = true; if (job.cancellable) job.aborter.abort(); }
      return Response.json({ accepted: true });
    }
    const headers = new Headers(init.headers);
    const stream = parts[4] === "channels";
    const channel = stream ? JSON.parse(String(init.body)) as { channel: string; path: string } : null;
    const id = channel?.channel ?? parts[5];
    const method = stream ? "GET" : headers.get("X-Domios-Mux-Method")!;
    const path = channel?.path ?? headers.get("X-Domios-Mux-Path")!;
    for (const key of [...headers.keys()]) if (key.startsWith("x-domios-mux-")) headers.delete(key);
    const job = { aborter: new AbortController(), detached: false, cancellable: stream || muxRequestPolicy(method, path, init.body).cancellable };
    jobs.set(id, job);
    void (async () => {
      try {
        const response = await domain(new URL(path, url.origin).href, { ...init, method, headers, body: stream ? undefined : init.body, signal: job.aborter.signal });
        if (job.detached) return;
        emit(session, { channel: id, open: true, status: response.status, headers: [...response.headers.entries()] });
        const reader = response.body?.getReader();
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
}

