import { uid } from "./uid";

const streamClientId = uid();

export type Unsubscribe = () => void;
export type TokenSource = string | (() => string | null);

export interface TransportOptions {
  origin: string;
  token: TokenSource;
  fetch?: typeof fetch;
  relayPeerId?: string;
  headers?: HeadersInit;
  onGatewayState?: (available: boolean) => void;
  onStreamError?: (error: unknown) => void;
}

export interface SseMessage {
  event: string;
  data: string;
  id?: string;
}

export type HttpMethod = "GET" | "POST" | "PUT" | "PATCH" | "DELETE";
export interface StreamEnd {
  reason: "eof" | "http" | "error" | "abort" | "capacity";
  bytes?: number;
  status?: number;
  error?: unknown;
}

export interface TransportClient {
  request?(path: string, init?: RequestInit, local?: boolean): Promise<Response>;
  json<T>(method: HttpMethod, path: string, body?: unknown, opts?: { signal?: AbortSignal; timeoutMs?: number; callerTabId?: string }): Promise<T>;
  localJson<T>(method: string, path: string, body?: unknown, opts?: { signal?: AbortSignal; callerTabId?: string }): Promise<T>;
  upload<T>(path: string, form: FormData, opts?: { signal?: AbortSignal }): Promise<T>;
  subscribe(stream: string, onEvent: (name: string, data: unknown) => void, opts?: { signal?: AbortSignal; onEnd?: (end: StreamEnd) => void; raw?: boolean; onActivity?: () => void; onReady?: () => void }): Unsubscribe;
  currentToken(): string | null;
  origin(): string;
}

export class TransportError extends Error {
  constructor(public status: number, public path: string, public body: string) {
    super(`${path} → HTTP ${status}: ${body.slice(0, 300)}`);
    this.name = "WebApiError";
  }
}

export { TransportError as WebApiError };

export class InvalidJsonResponseError extends TransportError {
  constructor(status: number, path: string, body: string, public contentType: string | null) {
    super(status, path, body);
    this.name = "InvalidJsonResponseError";
    this.message = `${path} → invalid daemon JSON response (${contentType ?? "missing content type"})`;
  }
}

export function decodeTransportResponse<T>(status: number, path: string, text: string, contentType: string | null, promptResponse = true): T {
  let value: unknown;
  let parsed = false;
  try { value = JSON.parse(text); parsed = true; } catch {}
  const promptMissing = promptResponse && status === 409 && path.endsWith("/prompt/respond")
    && typeof value === "object" && value !== null
    && "ok" in value && value.ok === false
    && "reason" in value && value.reason === "prompt_not_on_screen";
  if ((status < 200 || status >= 300) && !promptMissing) throw new TransportError(status, path, text);
  if (contentType?.split(";")[0].trim().toLowerCase() !== "application/json" || !parsed) {
    throw new InvalidJsonResponseError(status, path, text, contentType);
  }
  return value as T;
}

export const MAX_SSE_FRAME_BYTES = 2 * 1024 * 1024;
const MAX_JSON_DEPTH = 32;
const MAX_JSON_STRUCTURAL_TOKENS = 256 * 1024;

class SseCapacityError extends Error {
  constructor(public bytes: number) { super(`SSE capacity exceeded (${bytes} bytes)`); }
}

function guardJsonStructure(value: string, bytes: number): void {
  let depth = 0, tokens = 0;
  let quoted = false, escaped = false;
  for (const character of value) {
    if (quoted) {
      if (escaped) escaped = false;
      else if (character === "\\") escaped = true;
      else if (character === '"') quoted = false;
      continue;
    }
    if (character === '"') quoted = true;
    else if ("{}[],:".includes(character)) {
      tokens++;
      if (character === "{" || character === "[") depth++;
      else if (character === "}" || character === "]") depth--;
      if (depth > MAX_JSON_DEPTH || tokens > MAX_JSON_STRUCTURAL_TOKENS) throw new SseCapacityError(bytes);
    }
  }
}

class SseAccumulator {
  private buffer = new Uint8Array(4096);
  private bytes = 0;
  private tail = 0;

  push(chunk: Uint8Array, deliver: (frame: string) => void): void {
    for (const byte of chunk) {
      if (this.bytes === this.buffer.length) {
        const grown = new Uint8Array(Math.min(this.buffer.length * 2, MAX_SSE_FRAME_BYTES + 4));
        grown.set(this.buffer);
        this.buffer = grown;
      }
      this.buffer[this.bytes++] = byte;
      this.tail = ((this.tail << 8) | byte) >>> 0;
      const lastThree = this.tail & 0xffffff;
      const separator = this.tail === 0x0d0a0d0a ? 4
        : lastThree === 0x0a0d0a || lastThree === 0x0d0a0a ? 3
        : (this.tail & 0xffff) === 0x0a0a ? 2 : 0;
      if (separator) {
        const frame = new TextDecoder().decode(this.buffer.subarray(0, this.bytes - separator));
        this.bytes = 0;
        this.tail = 0;
        deliver(frame);
      } else {
        const suffix = lastThree === 0x0d0a0d ? 3
          : (this.tail & 0xffff) === 0x0d0a || (this.tail & 0xffff) === 0x0a0d ? 2
          : byte === 13 || byte === 10 ? 1 : 0;
        if (this.bytes - suffix > MAX_SSE_FRAME_BYTES) throw new SseCapacityError(this.bytes - suffix);
      }
    }
  }

  finish(): void {
    if (this.bytes > MAX_SSE_FRAME_BYTES) throw new SseCapacityError(this.bytes);
  }
}

export function parseSseFrame(frame: string): SseMessage | null {
  const bytes = new TextEncoder().encode(frame).length;
  if (bytes > MAX_SSE_FRAME_BYTES) throw new SseCapacityError(bytes);
  const data: string[] = [];
  let event = "message";
  let id: string | undefined;
  for (const line of frame.split(/\r?\n/)) {
    if (line.startsWith(":")) continue;
    const colon = line.indexOf(":");
    const field = colon < 0 ? line : line.slice(0, colon);
    const value = colon < 0 ? "" : line.slice(colon + 1).replace(/^ /, "");
    if (field === "data") data.push(value);
    if (field === "event") event = value;
    if (field === "id" && !value.includes("\0")) id = value;
  }
  if (!data.length) return null;
  const payload = data.join("\n");
  guardJsonStructure(payload, bytes);
  return { event, data: payload, ...(id === undefined ? {} : { id }) };
}

export function createTransportClient(options: TransportOptions): TransportClient {
  const url = new URL(options.origin);
  if (!["http:", "https:"].includes(url.protocol) || url.username || url.password
    || url.pathname !== "/" || url.search || url.hash) throw new Error("Explicit daemon HTTP origin required");
  const origin = url.origin;
  const fetcher: typeof fetch = (input, init) => (options.fetch ?? globalThis.fetch)(input, init);
  const resolvePath = (path: string) => {
    if (!path.startsWith("/") || path.startsWith("//") || path.includes("\\")) {
      throw new Error("Daemon path must be origin-relative");
    }
    return `${origin}${path}`;
  };
  const localFetch = async (path: string, init: RequestInit = {}) => {
    const target = resolvePath(path);
    const headers = new Headers(options.headers);
    new Headers(init.headers).forEach((value, key) => headers.set(key, value));
    const token = typeof options.token === "function" ? options.token() : options.token;
    if (!token) throw new Error("Daemon bearer token required");
    headers.set("Authorization", `Bearer ${token}`);
    headers.set("X-CodeTerm-Client-Id", streamClientId);
    if (!(init.body instanceof FormData) && !headers.has("Content-Type")) headers.set("Content-Type", "application/json");
    try {
      const response = await fetcher(target, { ...init, credentials: "omit", headers });
      options.onGatewayState?.(true);
      return response;
    } catch (error) {
      if (!init.signal?.aborted) options.onGatewayState?.(false);
      throw error;
    }
  };
  const tracked = async (path: string, init: RequestInit = {}, local = false) => {
    resolvePath(path);
    let response: Response;
    if (local || !options.relayPeerId) response = await localFetch(path, { ...init, signal: init.signal });
    else {
      const body = typeof init.body === "string" && init.body.length ? JSON.parse(init.body) : null;
      response = await localFetch("/api/mesh/relay-to", {
        method: "POST", signal: init.signal, headers: init.headers,
        body: JSON.stringify({ target_peer_id: options.relayPeerId, method: (init.method ?? "GET").toUpperCase(), path, body }),
      });
      if (response.ok) {
        const relay = decodeTransportResponse<{ ok: boolean; status: number; data: unknown; error?: string }>(
          response.status, "/api/mesh/relay-to", await response.text(), response.headers.get("content-type"), false,
        );
        response = new Response(relay.data == null ? relay.error ?? "" : JSON.stringify(relay.data), {
          status: relay.status || (relay.ok ? 200 : 502), headers: { "Content-Type": "application/json" },
        });
      }
    }
    return response;
  };

  const json = async <T>(method: string, path: string, body: unknown, init: RequestInit = {}, local = false) => {
    const response = await tracked(path, { ...init, method, body: body === undefined ? undefined : JSON.stringify(body) }, local);
    const text = await response.text();
    return decodeTransportResponse<T>(response.status, path, text, response.headers.get("content-type"), !local);
  };
  return {
    request: tracked,
    origin: () => origin,
    currentToken: () => typeof options.token === "function" ? options.token() : options.token,
    json: (method, path, body, opts) => {
      const deadline = opts?.timeoutMs === undefined ? undefined : AbortSignal.timeout(opts.timeoutMs);
      const signal = opts?.signal && deadline ? AbortSignal.any([opts.signal, deadline]) : opts?.signal ?? deadline;
      return json(method, path, body, { signal, headers: opts?.callerTabId ? { "X-CodeTerm-Tab-Id": opts.callerTabId } : undefined });
    },
    localJson: (method, path, body, opts) => json(method, path, body, { signal: opts?.signal, headers: opts?.callerTabId ? { "X-CodeTerm-Tab-Id": opts.callerTabId } : undefined }, true),
    async upload<T>(path: string, form: FormData, opts?: { signal?: AbortSignal }): Promise<T> {
      const response = await tracked(path, { method: "POST", body: form, signal: opts?.signal }, true);
      const text = await response.text();
      return decodeTransportResponse<T>(response.status, path, text, response.headers.get("content-type"));
    },
    subscribe(path, handler, init = {}) {
      resolvePath(path);
      const controller = new AbortController();
      let cleaned = false;
      const abort = () => controller.abort(init.signal?.reason);
      if (init.signal?.aborted) abort();
      else init.signal?.addEventListener("abort", abort, { once: true });
      const done = (async () => {
        let reader: ReadableStreamDefaultReader<Uint8Array> | undefined;
        let end: StreamEnd = { reason: "eof" };
        const cancelReader = () => { void reader?.cancel().catch(() => {}); };
        controller.signal.addEventListener("abort", cancelReader, { once: true });
        try {
          const streamPath = options.relayPeerId
            ? `/api/mesh/relay-stream?${new URLSearchParams({ peer: options.relayPeerId, path })}` : path;
          const response = await localFetch(streamPath, { headers: { Accept: "text/event-stream", "X-CodeTerm-Client-Id": streamClientId }, signal: controller.signal });
          if (!response.ok) throw new TransportError(response.status, path, await response.text());
          if (!response.body) throw new Error("Daemon SSE response has no body");
          if (!controller.signal.aborted) init.onReady?.();
          reader = response.body.getReader();
          if (controller.signal.aborted) await reader.cancel();
          const accumulator = new SseAccumulator();
          while (!controller.signal.aborted) {
            const chunk = await reader.read();
            if (chunk.done || controller.signal.aborted) break;
            init.onActivity?.();
            accumulator.push(chunk.value, (frame) => {
              const message = parseSseFrame(frame);
              if (message && !controller.signal.aborted) {
                let data: unknown = message.data;
                try { data = JSON.parse(message.data); } catch {}
                if (!init.raw && typeof data === "object" && data !== null && "event_type" in data && typeof data.event_type === "string" && "data" in data) {
                  handler(data.event_type, data.data);
                } else handler(message.event, data);
              }
            });
          }
          accumulator.finish();
        } catch (error) {
          if (controller.signal.aborted) end = { reason: "abort", error: controller.signal.reason };
          else {
            end = error instanceof SseCapacityError ? { reason: "capacity", bytes: error.bytes }
              : error instanceof TransportError
              ? { reason: "http", status: error.status, error }
              : { reason: "error", error };
            if (error instanceof SseCapacityError && !init.onEnd) {
              if (options.onStreamError) options.onStreamError(error);
              else console.warn("Daemon SSE subscription exceeded capacity", error);
            }
            if (!init.onEnd && !(error instanceof SseCapacityError)) throw error;
          }
        } finally {
          await reader?.cancel().catch(() => {});
          reader?.releaseLock();
          controller.signal.removeEventListener("abort", cancelReader);
          init.signal?.removeEventListener("abort", abort);
          const finalEnd = controller.signal.aborted ? { reason: "abort" as const, error: controller.signal.reason } : end;
          controller.abort();
          if (!cleaned) init.onEnd?.(finalEnd);
        }
      })();
      void done.catch((error: unknown) => {
        if (options.onStreamError) options.onStreamError(error);
        else console.warn("Daemon SSE subscription failed", error);
      });
      return () => { cleaned = true; controller.abort(); };
    },
  };
}
