import { createTransportClient, type TokenSource, type TransportClient } from "./transportClient";

export interface DaemonBootstrap { origin: string; token: TokenSource }
export interface ReloadCredentials { apiOrigin: string; token: string }
export const FRONTEND_BOOT_KEY = "codeterm.frontend.boot";
export const DAEMON_TOKEN_PREFIX = "codeterm-web-token:";
declare global { interface Window { __CODETERM_DAEMON__?: DaemonBootstrap } }

export function validateDaemonOrigin(origin: string, pageOrigin?: string, native = false): string {
  const url = new URL(origin);
  if (!["http:", "https:"].includes(url.protocol) || url.username || url.password
    || url.pathname !== "/" || url.search || url.hash) throw new Error("Explicit daemon HTTP origin required");
  const loopback = ["127.0.0.1", "localhost", "[::1]"].includes(url.hostname);
  if (native && !loopback) throw new Error("Native daemon origin must be loopback");
  if (pageOrigin && /^https?:/.test(pageOrigin) && pageOrigin !== url.origin
    && !(native && ["http://tauri.localhost", "https://tauri.localhost"].includes(pageOrigin))) {
    throw new Error("Daemon credentials belong to another origin");
  }
  return url.origin;
}

export function readReloadCredentials(storage: Pick<Storage, "getItem">): ReloadCredentials | undefined {
  const value = storage.getItem(FRONTEND_BOOT_KEY);
  if (!value) return undefined;
  const parsed: unknown = JSON.parse(value);
  if (!parsed || typeof parsed !== "object" || !("apiOrigin" in parsed) || typeof parsed.apiOrigin !== "string"
    || !("token" in parsed) || typeof parsed.token !== "string" || !parsed.token) {
    throw new Error("Invalid frontend reload credentials");
  }
  return { apiOrigin: parsed.apiOrigin, token: parsed.token };
}

let bootstrap: DaemonBootstrap | undefined;
let base: TransportClient | undefined;
const peers = new Map<string, TransportClient>();
const listeners = new Set<(client: TransportClient) => void>();

function gateway(available: boolean): void {
  if (typeof window !== "undefined") {
    const EventConstructor = window.CustomEvent ?? CustomEvent;
    window.dispatchEvent(new EventConstructor("codeterm-gateway-state", { detail: { available } }));
  }
}

export function configureDaemonTransport(input: DaemonBootstrap | ReloadCredentials): TransportClient {
  const origin = validateDaemonOrigin("origin" in input ? input.origin : input.apiOrigin,
    typeof window === "undefined" ? undefined : window.location.origin,
    typeof window !== "undefined" && "__TAURI_INTERNALS__" in window);
  if (typeof input.token === "string" && !input.token) throw new Error("Daemon bearer token required");
  bootstrap = { origin, token: input.token };
  base = createTransportClient({ ...bootstrap, onGatewayState: gateway });
  peers.clear();
  for (const listener of listeners) listener(base);
  return base;
}

export function initializeDaemonTransport(): TransportClient | undefined {
  if (base) return base;
  if (typeof window === "undefined") return undefined;
  const injected = window.__CODETERM_DAEMON__;
  if (injected) {
    delete window.__CODETERM_DAEMON__;
    return configureDaemonTransport(injected);
  }
  const restored = window.sessionStorage ? readReloadCredentials(window.sessionStorage) : typeof sessionStorage === "undefined" ? undefined : readReloadCredentials(sessionStorage);
  if (restored) return configureDaemonTransport(restored);
  if ("__TAURI_INTERNALS__" in window) return undefined;
  return configureDaemonTransport({
    origin: window.location.origin ?? `${window.location.protocol}//${window.location.host}`,
    token: () => (window.localStorage ?? localStorage).getItem(DAEMON_TOKEN_PREFIX + (window.location.origin ?? `${window.location.protocol}//${window.location.host}`))
  });
}

export function baseDaemonTransport(): TransportClient {
  const client = base ?? initializeDaemonTransport();
  if (!client) throw new Error("Explicit daemon bootstrap required; native RPC fallback is unavailable");
  return client;
}

export function daemonTransportForPeer(peerId?: string): TransportClient {
  const client = baseDaemonTransport();
  if (!peerId) return client;
  if (!peers.has(peerId)) peers.set(peerId, createTransportClient({ ...bootstrap!, relayPeerId: peerId, onGatewayState: gateway }));
  return peers.get(peerId)!;
}

export function onDaemonTransport(listener: (client: TransportClient) => void): () => void {
  listeners.add(listener);
  if (base) listener(base);
  return () => { listeners.delete(listener); };
}
