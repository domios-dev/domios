/**
 * Web-mirror runtime: local daemon host/token + mesh relay targeting.
 *
 * In WEB mode (no Tauri) the React app talks to the CodeTerm daemon that
 * served /web over plain HTTP, authenticated by that daemon's own token.
 * The browser ALWAYS points at this local daemon. To view a paired mesh
 * peer the picker sets a "relay target" (a peer instance_id); requests are
 * then wrapped and proxied through the local daemon's `/api/mesh/relay-to`,
 * which forwards over the signed mesh channel. The browser never re-points
 * at the peer and never holds a peer-scoped secret.
 */

export const IS_WEB =
  typeof window !== "undefined" && !("__TAURI_INTERNALS__" in window);

const TOKEN_PREFIX = "codeterm-web-token:";
const RELAY_PEER_KEY = "codeterm-web-relay-peer";
const RELAY_NAME_KEY = "codeterm-web-relay-peer-name";

/** Origin the SPA was served from — the local daemon. */
export function selfHost(): string {
  return `${window.location.protocol}//${window.location.host}`;
}

/** API host the browser talks to directly — always the local daemon. */
export function apiHost(): string {
  return selfHost();
}

export function tokenFor(host: string): string | null {
  return localStorage.getItem(TOKEN_PREFIX + host);
}

export function setTokenFor(host: string, token: string): void {
  localStorage.setItem(TOKEN_PREFIX + host, token);
}

/** Local-daemon token (authenticates the browser to its own daemon). */
export function currentToken(): string | null {
  return tokenFor(selfHost());
}

interface RelayTarget {
  id: string;
  name: string;
}

/** Active mesh peer the browser is viewing through the relay, or null (self). */
export function relayTarget(): RelayTarget | null {
  const id = sessionStorage.getItem(RELAY_PEER_KEY);
  if (!id) return null;
  return { id, name: sessionStorage.getItem(RELAY_NAME_KEY) ?? id };
}

export function setRelayTarget(id: string, name: string): void {
  sessionStorage.setItem(RELAY_PEER_KEY, id);
  sessionStorage.setItem(RELAY_NAME_KEY, name);
}

export function clearRelayTarget(): void {
  sessionStorage.removeItem(RELAY_PEER_KEY);
  sessionStorage.removeItem(RELAY_NAME_KEY);
}

/** Local-daemon address shape (the desktop app's `config.api`). */
export interface DaemonApiConfig {
  host?: string | null;
  port?: number | null;
  token?: string | null;
}

/**
 * Canonical daemon HTTP origin for plugin assets/iframes/fetch across every access
 * mode: web uses `apiHost()` (tracks window.location), desktop uses the local daemon.
 * Never reconstruct from the daemon's bind address (`0.0.0.0`).
 */
export function daemonHttpBase(api?: DaemonApiConfig): string {
  if (IS_WEB) return apiHost();
  const host = api?.host && api.host !== "0.0.0.0" ? api.host : "127.0.0.1";
  return `http://${host}:${api?.port ?? 7686}`;
}

/** Token to authenticate daemon HTTP calls in the current access mode. */
export function daemonAuthToken(api?: DaemonApiConfig): string {
  return (IS_WEB ? currentToken() : api?.token) ?? "";
}

export class WebApiError extends Error {
  constructor(
    public status: number,
    public path: string,
    body: string,
  ) {
    super(`${path} → HTTP ${status}: ${body.slice(0, 300)}`);
  }
}

/** Mesh relay envelope returned by `/api/mesh/relay-to`. */
interface RelayResponse {
  ok: boolean;
  status: number;
  data: unknown;
  error: string | null;
}

/** Authenticated fetch straight at the local daemon — never relayed. */
export async function localFetch(
  path: string,
  init: RequestInit = {},
): Promise<Response> {
  const headers = new Headers(init.headers);
  headers.set("Content-Type", "application/json");
  const token = currentToken();
  if (token) headers.set("Authorization", `Bearer ${token}`);
  return fetch(`${selfHost()}${path}`, { ...init, headers });
}

/** Local-daemon JSON call (bypasses any active relay target). */
export async function localJson<T>(
  method: string,
  path: string,
  body?: unknown,
): Promise<T> {
  const res = await localFetch(path, {
    method,
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const text = await res.text();
  if (!res.ok) throw new WebApiError(res.status, path, text);
  try {
    return JSON.parse(text) as T;
  } catch {
    return text as unknown as T;
  }
}

/**
 * Authenticated fetch. With no relay target it hits the local daemon
 * directly; when a mesh peer is the active target the request is wrapped as
 * `POST /api/mesh/relay-to` to the LOCAL daemon (existing local token), which
 * proxies it to the peer over the signed mesh channel and returns its
 * response — synthesized back into a `Response` so callers are unaffected.
 */
export async function webFetch(
  path: string,
  init: RequestInit = {},
): Promise<Response> {
  const target = relayTarget();
  if (!target) return localFetch(path, init);

  const method = (init.method ?? "GET").toUpperCase();
  const body =
    typeof init.body === "string" && init.body.length > 0
      ? JSON.parse(init.body)
      : null;
  const local = await localFetch("/api/mesh/relay-to", {
    method: "POST",
    body: JSON.stringify({ target_peer_id: target.id, method, path, body }),
  });
  // Framing-level failure (peer unpaired, daemon error): surface verbatim.
  if (!local.ok) return local;
  const relay = (await local.json()) as RelayResponse;
  const inner = relay.data == null ? "" : JSON.stringify(relay.data);
  return new Response(inner, {
    status: relay.status || (relay.ok ? 200 : 502),
    headers: { "Content-Type": "application/json" },
  });
}

/**
 * Upload a browser file/blob to the daemon host; returns the server-side
 * path the agent CLI can read. Content-Type stays unset so the browser
 * writes the multipart boundary itself.
 */
export async function webUpload(
  file: Blob,
  filename: string,
): Promise<{ path: string; size: number }> {
  const form = new FormData();
  form.append("file", file, filename);
  const headers = new Headers();
  const token = currentToken();
  if (token) headers.set("Authorization", `Bearer ${token}`);
  const res = await fetch(`${apiHost()}/api/upload`, {
    method: "POST",
    headers,
    body: form,
  });
  const text = await res.text();
  if (!res.ok) throw new WebApiError(res.status, "/api/upload", text);
  return JSON.parse(text) as { path: string; size: number };
}

export async function webJson<T>(
  method: string,
  path: string,
  body?: unknown,
): Promise<T> {
  const res = await webFetch(path, {
    method,
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const text = await res.text();
  if (!res.ok) throw new WebApiError(res.status, path, text);
  try {
    return JSON.parse(text) as T;
  } catch {
    return text as unknown as T;
  }
}
