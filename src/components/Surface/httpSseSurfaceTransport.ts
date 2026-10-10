import { GATEWAY_STATE_EVENT } from "../../lib/webApi";
import { createSseParser } from "./sseParser";
import type { StreamViewport } from "./streamModel";
import {
  callReply,
  INITIAL_RECONNECT,
  nextReconnect,
  reconnectOnSignal,
  streamOpened,
  streamOutcomeForStatus,
  surfaceCallUrl,
  surfaceStreamEvent,
  surfaceStreamUrl,
  type ReconnectState,
  type StreamOutcome,
} from "./surfaceHttpProtocol";
import type { Disconnect, StreamHandlers, SurfaceNotice, SurfaceStreamTransport } from "./surfaceStreamTransport";

export type FetchLike = (url: string, init: RequestInit) => Promise<Response>;
export type RetrySignal = (onSignal: () => void) => () => void;

export interface HttpSseSurfaceTransportOptions {
  baseUrl: string;
  token: string;
  tabId: string;
  fetch?: FetchLike;
  retrySignal?: RetrySignal;
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function authHeaders(token: string, extra: Record<string, string>): Headers {
  const headers = new Headers(extra);
  if (token) headers.set("Authorization", `Bearer ${token}`);
  return headers;
}

/** Events that suggest the daemon may be reachable again; a parked stream retries on them, never on a timer. */
export const browserRetrySignal: RetrySignal = (onSignal) => {
  if (typeof window === "undefined") return () => undefined;
  const onVisible = () => {
    if (document.visibilityState === "visible") onSignal();
  };
  const onGateway = (event: Event) => {
    if ((event as CustomEvent<{ available?: boolean }>).detail?.available) onSignal();
  };
  window.addEventListener("online", onSignal);
  window.addEventListener("focus", onSignal);
  window.addEventListener(GATEWAY_STATE_EVENT, onGateway);
  document.addEventListener("visibilitychange", onVisible);
  return () => {
    window.removeEventListener("online", onSignal);
    window.removeEventListener("focus", onSignal);
    window.removeEventListener(GATEWAY_STATE_EVENT, onGateway);
    document.removeEventListener("visibilitychange", onVisible);
  };
};

async function readStream(body: ReadableStream<Uint8Array>, handlers: StreamHandlers, isClosed: () => boolean): Promise<boolean> {
  const reader = body.getReader();
  const decoder = new TextDecoder();
  const parser = createSseParser();
  let delivered = false;
  try {
    for (;;) {
      const { done, value } = await reader.read();
      if (isClosed()) return delivered;
      const text = done ? decoder.decode() : decoder.decode(value, { stream: true });
      for (const message of parser.push(text)) {
        const event = surfaceStreamEvent(message);
        if (!event || isClosed()) continue;
        delivered = true;
        if (event.type === "state") handlers.state(event.state);
        else if (event.type === "frame") handlers.frame(event.frame);
        else handlers.notice(event.notice);
      }
      if (done) return delivered;
    }
  } finally {
    reader.releaseLock();
  }
}

class StreamSession {
  private controller: AbortController | null = null;
  private reconnect: ReconnectState = INITIAL_RECONNECT;
  private closed = false;
  private readonly stopSignal: () => void;

  constructor(
    private readonly options: Required<Omit<HttpSseSurfaceTransportOptions, "retrySignal">>,
    private readonly url: string,
    private readonly handlers: StreamHandlers,
    retrySignal: RetrySignal,
  ) {
    this.stopSignal = retrySignal(() => this.onSignal());
    void this.run();
  }

  close() {
    this.closed = true;
    this.stopSignal();
    this.controller?.abort();
    this.controller = null;
  }

  private notify(notice: SurfaceNotice | undefined) {
    if (notice && !this.closed) this.handlers.notice(notice);
  }

  private onSignal() {
    if (this.closed) return;
    const { retry, state } = reconnectOnSignal(this.reconnect);
    this.reconnect = state;
    if (retry) void this.run();
  }

  // Each drop is retried at once up to a small cap, then the session parks until a reachability event.
  private async run() {
    while (!this.closed) {
      const step = nextReconnect(this.reconnect, await this.attempt());
      if (this.closed) return;
      this.reconnect = step.state;
      this.notify(step.notice);
      if (step.action !== "retry") return;
    }
  }

  private async attempt(): Promise<StreamOutcome> {
    const controller = new AbortController();
    this.controller = controller;
    try {
      const response = await this.options.fetch(this.url, {
        method: "GET",
        headers: authHeaders(this.options.token, { Accept: "text/event-stream" }),
        cache: "no-store",
        signal: controller.signal,
      });
      if (!response.ok) return streamOutcomeForStatus(response.status, await response.text().catch(() => ""));
      if (!response.body) return { kind: "dropped", delivered: false, message: "the stream has no body" };
      const opened = streamOpened(this.reconnect);
      this.reconnect = opened.state;
      if (!this.closed) this.handlers.open();
      this.notify(opened.notice);
      const delivered = await readStream(response.body, this.handlers, () => this.closed);
      return { kind: "dropped", delivered, message: "the stream ended" };
    } catch (error) {
      return { kind: "dropped", delivered: false, message: errorMessage(error) };
    } finally {
      if (this.controller === controller) this.controller = null;
    }
  }
}

export function createHttpSseSurfaceTransport(options: HttpSseSurfaceTransportOptions): SurfaceStreamTransport {
  const resolved = {
    baseUrl: options.baseUrl,
    token: options.token,
    tabId: options.tabId,
    fetch: options.fetch ?? ((url: string, init: RequestInit) => fetch(url, init)),
  };
  const retrySignal = options.retrySignal ?? browserRetrySignal;
  return {
    connect(params: StreamViewport, handlers: StreamHandlers): Disconnect {
      const session = new StreamSession(resolved, surfaceStreamUrl(resolved.baseUrl, resolved.tabId, params), handlers, retrySignal);
      return () => session.close();
    },
    async invoke<T>(method: string, args: Record<string, unknown> = {}): Promise<T> {
      const response = await resolved.fetch(surfaceCallUrl(resolved.baseUrl, resolved.tabId), {
        method: "POST",
        headers: authHeaders(resolved.token, { "Content-Type": "application/json" }),
        body: JSON.stringify({ method, args }),
      });
      const reply = callReply<T>(response.status, await response.text());
      if (!reply.ok) throw new Error(reply.message);
      return reply.result;
    },
  };
}
