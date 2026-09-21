/**
 * Web-mirror runtime: local daemon host/token + mesh relay targeting.
 *
 * In WEB mode (no Tauri) the React app talks to the CodeTerm daemon that
 * served the browser UI over plain HTTP, authenticated by that daemon's own token.
 * The browser ALWAYS points at this local daemon. To view a paired mesh
 * peer the picker sets a "relay target" (a peer instance_id); requests are
 * then wrapped and proxied through the local daemon's `/api/mesh/relay-to`,
 * which forwards over the signed mesh channel. The browser never re-points
 * at the peer and never holds a peer-scoped secret.
 */

import { RELAY_PROTOCOL_VERSION } from "./types";

export const IS_WEB =
  typeof window !== "undefined" && !("__TAURI_INTERNALS__" in window);

export const GATEWAY_STATE_EVENT = "codeterm-gateway-state";

function publishGatewayState(available: boolean): void {
  if (typeof window !== "undefined") {
    window.dispatchEvent(new window.CustomEvent(GATEWAY_STATE_EVENT, { detail: { available } }));
  }
}

const TOKEN_PREFIX = "codeterm-web-token:";
const RELAY_PEER_KEY = "codeterm-web-relay-peer";
const RELAY_NAME_KEY = "codeterm-web-relay-peer-name";

function relayKey(base: string): string {
  return `${base}:${selfHost()}`;
}

function migrateRelayFromSessionStorage(): void {
  if (typeof sessionStorage === "undefined" || typeof localStorage === "undefined") return;
  const legacyId = sessionStorage.getItem(RELAY_PEER_KEY);
  if (!legacyId) return;
  if (!localStorage.getItem(relayKey(RELAY_PEER_KEY))) {
    localStorage.setItem(relayKey(RELAY_PEER_KEY), legacyId);
    const legacyName = sessionStorage.getItem(RELAY_NAME_KEY);
    if (legacyName) localStorage.setItem(relayKey(RELAY_NAME_KEY), legacyName);
  }
  sessionStorage.removeItem(RELAY_PEER_KEY);
  sessionStorage.removeItem(RELAY_NAME_KEY);
}

if (typeof window !== "undefined") {
  migrateRelayFromSessionStorage();
}

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

let requestContextGeneration = 0;
const requestControllers = new Set<AbortController>();

/** Cancel every request bound to the current peer before changing relay target. */
export function invalidateWebRequestContext(): number {
  requestContextGeneration += 1;
  for (const controller of requestControllers) controller.abort();
  requestControllers.clear();
  return requestContextGeneration;
}

/** Active mesh peer the browser is viewing through the relay, or null (self). */
export function relayTarget(): RelayTarget | null {
  const id = localStorage.getItem(relayKey(RELAY_PEER_KEY));
  if (!id) return null;
  return { id, name: localStorage.getItem(relayKey(RELAY_NAME_KEY)) ?? id };
}

/** Sole rule for whether a peer can serve as a relay target. A stored target
 *  outlives pairing, presence and protocol, so adoption must re-check it. */
export function relayPeerUsable(
  peer: { online?: boolean; relay_protocol_version?: number } | undefined,
): boolean {
  return peer?.online === true && peer.relay_protocol_version === RELAY_PROTOCOL_VERSION;
}

export function usableRelayTarget(
  peers: Array<{ instance_id: string; instance_name: string; online?: boolean; relay_protocol_version?: number }>,
  instanceId: string,
): RelayTarget | null {
  const peer = peers.find((candidate) => candidate.instance_id === instanceId);
  return relayPeerUsable(peer) && peer
    ? { id: peer.instance_id, name: peer.instance_name }
    : null;
}

export function setRelayTarget(id: string, name: string): void {
  localStorage.setItem(relayKey(RELAY_PEER_KEY), id);
  localStorage.setItem(relayKey(RELAY_NAME_KEY), name);
}

export function clearRelayTarget(): void {
  localStorage.removeItem(relayKey(RELAY_PEER_KEY));
  localStorage.removeItem(relayKey(RELAY_NAME_KEY));
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

export function isPromptNotOnScreenResponse(value: unknown): boolean {
  return typeof value === "object"
    && value !== null
    && "ok" in value
    && value.ok === false
    && "reason" in value
    && value.reason === "prompt_not_on_screen";
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
  try {
    const response = await fetch(`${selfHost()}${path}`, { ...init, headers });
    publishGatewayState(true);
    return response;
  } catch (error) {
    if (!(error instanceof DOMException && error.name === "AbortError")) {
      publishGatewayState(false);
    }
    throw error;
  }
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
  const generation = requestContextGeneration;
  const target = relayTarget();
  const controller = new AbortController();
  requestControllers.add(controller);
  const externalSignal = init.signal;
  const abortFromExternal = () => controller.abort(externalSignal?.reason);
  if (externalSignal?.aborted) abortFromExternal();
  else externalSignal?.addEventListener("abort", abortFromExternal, { once: true });

  try {
    const requestInit = { ...init, signal: controller.signal };
    if (!target) {
      const response = await localFetch(path, requestInit);
      if (generation !== requestContextGeneration) throw new DOMException("Peer context changed", "AbortError");
      return response;
    }

    const method = (init.method ?? "GET").toUpperCase();
    const body =
      typeof init.body === "string" && init.body.length > 0
        ? JSON.parse(init.body)
        : null;
    const local = await localFetch("/api/mesh/relay-to", {
      method: "POST",
      body: JSON.stringify({ target_peer_id: target.id, method, path, body }),
      signal: controller.signal,
    });
    if (generation !== requestContextGeneration) throw new DOMException("Peer context changed", "AbortError");
    if (!local.ok) return local;
    const relay = (await local.json()) as RelayResponse;
    if (generation !== requestContextGeneration) throw new DOMException("Peer context changed", "AbortError");
    const inner = relay.data == null ? "" : JSON.stringify(relay.data);
    return new Response(inner, {
      status: relay.status || (relay.ok ? 200 : 502),
      headers: { "Content-Type": "application/json" },
    });
  } finally {
    requestControllers.delete(controller);
    externalSignal?.removeEventListener("abort", abortFromExternal);
  }
}

/** Upload a browser blob to a daemon path the agent CLI can read. */
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
  if (!res.ok) {
    if (res.status === 409 && path.endsWith("/prompt/respond")) {
      try {
        const value: unknown = JSON.parse(text);
        if (isPromptNotOnScreenResponse(value)) return value as T;
      } catch {}
    }
    throw new WebApiError(res.status, path, text);
  }
  try {
    return JSON.parse(text) as T;
  } catch {
    return text as unknown as T;
  }
}
