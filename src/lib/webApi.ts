import { browserUploadBody } from "./browserUploadBody";
import { baseDaemonTransport, daemonTransportForPeer, DAEMON_TOKEN_PREFIX } from "./daemonBootstrap";
import { decodeTransportResponse, type TransportClient } from "./transportClient";
export { TransportError as WebApiError } from "./transportClient";

import { RELAY_PROTOCOL_VERSION } from "./types";

export const IS_WEB =
  typeof window !== "undefined" && !("__TAURI_INTERNALS__" in window);

export const GATEWAY_STATE_EVENT = "codeterm-gateway-state";

const RELAY_PEER_KEY = "codeterm-web-relay-peer";
const RELAY_NAME_KEY = "codeterm-web-relay-peer-name";

function relayKey(base: string): string {
  return `${base}:${typeof window === "undefined" ? "" : (window.location.origin ?? `${window.location.protocol}//${window.location.host}`)}`;
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
  return baseDaemonTransport().origin();
}

/** API host the browser talks to directly — always the local daemon. */
export function apiHost(): string {
  return selfHost();
}

export function tokenFor(host: string): string | null {
  return localStorage.getItem(DAEMON_TOKEN_PREFIX + host);
}

export function setTokenFor(host: string, token: string): void {
  localStorage.setItem(DAEMON_TOKEN_PREFIX + host, token);
}

/** Local-daemon token (authenticates the browser to its own daemon). */
export function currentToken(): string | null {
  return baseDaemonTransport().currentToken();
}

interface RelayTarget {
  id: string;
  name: string;
}

let requestContextGeneration = 0;
const requestControllers = new Set<AbortController>();

export function webRequestContext(): number {
  return requestContextGeneration;
}

export function assertWebRequestContext(generation: number): void {
  if (generation !== requestContextGeneration) throw new DOMException("Peer context changed", "AbortError");
}

/** Cancel every request bound to the current peer before changing relay target. */
export function invalidateWebRequestContext(): number {
  requestContextGeneration += 1;
  for (const controller of requestControllers) controller.abort();
  requestControllers.clear();
  return requestContextGeneration;
}

/** Active mesh peer the browser is viewing through the relay, or null (self). */
export function relayTarget(): RelayTarget | null {
  if (typeof localStorage === "undefined") return null;
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
export function daemonHttpBase(_api?: DaemonApiConfig): string {
  return baseDaemonTransport().origin();
}

export function daemonAuthToken(_api?: DaemonApiConfig): string {
  return baseDaemonTransport().currentToken() ?? "";
}

export function daemonFetch(_api: DaemonApiConfig, path: string, init: RequestInit = {}): Promise<Response> {
  return webFetch(path, init);
}

export function isPromptNotOnScreenResponse(value: unknown): boolean {
  return typeof value === "object"
    && value !== null
    && "ok" in value
    && value.ok === false
    && "reason" in value
    && value.reason === "prompt_not_on_screen";
}

export function localFetch(path: string, init: RequestInit = {}): Promise<Response> {
  return baseDaemonTransport().request!(path, init, true);
}

/** Local-daemon JSON call (bypasses any active relay target). */
export async function localJson<T>(
  method: string,
  path: string,
  body?: unknown,
): Promise<T> {
  return baseDaemonTransport().localJson<T>(method, path, body);
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
    const response = await daemonTransportForPeer(target?.id).request!(path, requestInit);
    assertWebRequestContext(generation);
    return response;
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
  const form = await browserUploadBody(file, filename);
  const controller = new AbortController();
  const interrupt = () => controller.abort();
  const visibilityChanged = () => { if (document.visibilityState === "hidden") interrupt(); };
  window.addEventListener("pagehide", interrupt);
  document.addEventListener("visibilitychange", visibilityChanged);
  try {
    return await baseDaemonTransport().upload("/api/attachments/upload", form, { signal: controller.signal });
  } finally {
    window.removeEventListener("pagehide", interrupt);
    document.removeEventListener("visibilitychange", visibilityChanged);
  }
}

export async function webJson<T>(
  method: string,
  path: string,
  body?: unknown,
  opts?: Parameters<TransportClient["json"]>[3],
): Promise<T> {
  const generation = webRequestContext();
  const deadline = opts?.timeoutMs === undefined ? undefined : AbortSignal.timeout(opts.timeoutMs);
  const signal = opts?.signal && deadline ? AbortSignal.any([opts.signal, deadline]) : opts?.signal ?? deadline;
  const res = await webFetch(path, {
    signal,
    headers: opts?.callerTabId ? { "X-CodeTerm-Tab-Id": opts.callerTabId } : undefined,
    method,
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const text = await res.text();
  assertWebRequestContext(generation);
  return decodeTransportResponse<T>(res.status, path, text, res.headers.get("content-type"));
}
