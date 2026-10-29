interface Flight {
  tail: Promise<void>;
  holders: number;
}

function record(value: unknown): Record<string, unknown> | null {
  return value !== null && typeof value === "object" && !Array.isArray(value) ? value as Record<string, unknown> : null;
}

function parsedBody(body: RequestInit["body"]): unknown {
  if (typeof body === "string") { try { return JSON.parse(body); } catch {} }
  return undefined;
}

export function startupMutationKey(path: string, init: RequestInit, peerId?: string, local = false): string | null {
  let method = (init.method ?? "GET").toUpperCase();
  let destination = local || !peerId ? "local" : `peer:${peerId}`;
  let body: unknown;
  const relayed = path === "/api/mesh/relay-to";
  if (relayed) {
    const relay = record(parsedBody(init.body));
    if (!relay || typeof relay.target_peer_id !== "string" || typeof relay.path !== "string" || typeof relay.method !== "string") return null;
    destination = `peer:${relay.target_peer_id}`;
    path = relay.path;
    method = relay.method.toUpperCase();
    body = relay.body;
  }
  const route = path.split("?")[0];
  if (["GET", "HEAD"].includes(method) || /\/(?:stop|close|interrupt)$/.test(route)) return null;
  if (/\/(?:pty\/[^/]+|tabs\/[^/]+\/pty)\/write$/.test(route)) {
    const data = record(relayed ? body : parsedBody(init.body))?.data;
    if (typeof data === "string" && (data.includes("\x03") || data === "\x1b")) return null;
  }
  const target = route.match(/^\/api\/(?:tabs|pty|workspaces)\/[^/]+/)?.[0] ?? route;
  return `${destination}:${target}`;
}

export function createStartupMutations() {
  const flights = new Map<string, Flight>();
  return {
    run(key: string | null, start: (hold: () => Promise<void>) => Promise<Response>, signal?: AbortSignal | null): Promise<Response> {
      if (key === null) return start(() => Promise.resolve());
      let flight = flights.get(key);
      if (!flight) {
        flight = { tail: Promise.resolve(), holders: 0 };
        flights.set(key, flight);
      }
      const owned = flight;
      const previous = owned.tail;
      let holding = false;
      const hold = () => {
        if (!holding && !signal?.aborted) { holding = true; owned.holders++; }
        return previous;
      };
      const releaseHolder = () => { if (holding) { holding = false; owned.holders--; } };
      const begin = async () => {
        if (signal?.aborted) throw signal.reason ?? new DOMException("Request cancelled", "AbortError");
        return start(hold);
      };
      // Ordinary calls run concurrently; only a proven startup refusal installs a barrier.
      const result = owned.holders > 0 ? previous.then(begin) : begin();
      const settled = result.then(releaseHolder, releaseHolder);
      const tail = Promise.all([previous, settled]).then(() => {});
      owned.tail = tail;
      void tail.then(() => { if (owned.tail === tail) flights.delete(key); });
      if (!signal) return result;
      return new Promise<Response>((resolve, reject) => {
        const abort = () => { releaseHolder(); reject(signal.reason ?? new DOMException("Request cancelled", "AbortError")); };
        const release = () => signal.removeEventListener("abort", abort);
        signal.addEventListener("abort", abort, { once: true });
        result.then(value => { release(); resolve(value); }, error => { release(); reject(error); });
        if (signal.aborted) { release(); abort(); }
      });
    },
  };
}
