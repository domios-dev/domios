/**
 * Minimal browser surface so the real web-mirror modules (`src/lib/webEvents`,
 * `src/lib/webApi`, `src/store/multiplexer/reducer`) can run under Node against
 * a live dev daemon. No browser, no Playwright: the modules under test are the
 * ones the browser loads, only the host objects are shimmed.
 *
 * `IS_WEB` is evaluated at module load, so install() must run before any
 * dynamic import of those modules.
 */

class MemoryStorage implements Storage {
  private values = new Map<string, string>();
  get length(): number {
    return this.values.size;
  }
  clear(): void {
    this.values.clear();
  }
  getItem(key: string): string | null {
    return this.values.get(key) ?? null;
  }
  key(index: number): string | null {
    return [...this.values.keys()][index] ?? null;
  }
  removeItem(key: string): void {
    this.values.delete(key);
  }
  setItem(key: string, value: string): void {
    this.values.set(key, value);
  }
}

export interface WebEnvOptions {
  origin: string;
  token: string;
  relayPeer?: { id: string; name: string } | null;
}

/** Simulates the transport outage that parks the mirror's cursor. Only the
 *  event stream is gated; ordinary API calls keep working, which is what a
 *  dropped SSE path actually looks like. */
export interface FetchGate {
  park(): void;
  release(): void;
  parked(): boolean;
  attempts(): number;
  /** Resolves when an event-stream response returns headers. The daemon
   *  subscribes to the bus before it answers, so headers are a sound
   *  readiness signal - not a delay. */
  onOpen(fn: () => void): void;
  opens(): number;
}

export interface WebEnv {
  muxRefreshes: () => number;
  onMuxRefresh: (fn: () => void) => void;
  bootChanges: () => number;
  gate: FetchGate;
}

export function installWebEnv(options: WebEnvOptions): WebEnv {
  const listeners = new Map<string, Set<EventListener>>();
  const localStorage = new MemoryStorage();
  const sessionStorage = new MemoryStorage();
  let muxRefreshes = 0;
  let bootChanges = 0;
  const muxRefreshHandlers = new Set<() => void>();

  const url = new URL(options.origin);
  const win = {
    localStorage,
    sessionStorage,
    location: { protocol: url.protocol, host: url.host, origin: options.origin },
    addEventListener(type: string, listener: EventListener) {
      const set = listeners.get(type) ?? new Set<EventListener>();
      set.add(listener);
      listeners.set(type, set);
    },
    removeEventListener(type: string, listener: EventListener) {
      listeners.get(type)?.delete(listener);
    },
    dispatchEvent(event: Event) {
      if (event.type === "codeterm-refresh-mux-state") {
        muxRefreshes++;
        for (const fn of [...muxRefreshHandlers]) fn();
      }
      if (event.type === "codeterm-web-boot-changed") bootChanges++;
      for (const listener of listeners.get(event.type) ?? []) listener(event);
      return true;
    },
    CustomEvent,
  };

  globalThis.window = win as unknown as Window & typeof globalThis;
  globalThis.localStorage = localStorage;
  globalThis.sessionStorage = sessionStorage;

  localStorage.setItem(`codeterm-web-token:${options.origin}`, options.token);
  if (options.relayPeer) {
    localStorage.setItem(`codeterm-web-relay-peer:${options.origin}`, options.relayPeer.id);
    localStorage.setItem(`codeterm-web-relay-peer-name:${options.origin}`, options.relayPeer.name);
  }

  let parked = false;
  let attempts = 0;
  let opens = 0;
  const openHandlers = new Set<() => void>();
  // Live event-stream connections, so park() can actually cut the path. Gating
  // fetch alone only blocks the NEXT connect; an open SSE keeps delivering.
  const live = new Set<{ cut: () => void }>();
  const realFetch = globalThis.fetch;

  /** Pass the SSE body through a stream this harness can sever on demand. */
  function severable(response: Response): Response {
    const upstream = response.body!.getReader();
    let cut = false;
    const handle = {
      cut: () => {
        cut = true;
        void upstream.cancel().catch(() => {});
      },
    };
    live.add(handle);
    const body = new ReadableStream<Uint8Array>({
      async pull(controller) {
        if (cut) {
          live.delete(handle);
          controller.close();
          return;
        }
        try {
          const { done, value } = await upstream.read();
          if (done || cut || !value) {
            live.delete(handle);
            controller.close();
            return;
          }
          controller.enqueue(value);
        } catch {
          live.delete(handle);
          controller.close();
        }
      },
      cancel() {
        handle.cut();
        live.delete(handle);
      },
    });
    return new Response(body, { status: response.status, headers: response.headers });
  }

  globalThis.fetch = ((input: RequestInfo | URL, init?: RequestInit) => {
    const url = typeof input === "string" ? input : input.toString();
    const isStream = url.includes("/api/events/stream") || url.includes("/api/mesh/relay-stream");
    if (!isStream) return realFetch(input, init);
    attempts++;
    if (parked) return Promise.reject(new TypeError("gated: event stream parked"));
    return realFetch(input, init).then((response) => {
      if (!response.ok || !response.body) return response;
      opens++;
      const proxied = severable(response);
      for (const fn of [...openHandlers]) fn();
      return proxied;
    });
  }) as typeof fetch;

  return {
    muxRefreshes: () => muxRefreshes,
    onMuxRefresh: (fn) => void muxRefreshHandlers.add(fn),
    bootChanges: () => bootChanges,
    gate: {
      park: () => {
        parked = true;
        for (const handle of [...live]) handle.cut();
      },
      release: () => { parked = false; },
      parked: () => parked,
      attempts: () => attempts,
      onOpen: (fn) => void openHandlers.add(fn),
      opens: () => opens,
    },
  };
}
