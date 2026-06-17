/**
 * Web-mirror runtime: API host + per-peer token management.
 *
 * In WEB mode (no Tauri) the React app talks to a CodeTerm daemon over
 * plain HTTP. The default target is the daemon that served /web (same
 * origin). The peer picker can flip the target to a paired peer by asking
 * the current daemon to mint the target's web token over the signed mesh
 * channel, then storing that token per host.
 */

export const IS_WEB =
  typeof window !== "undefined" && !("__TAURI_INTERNALS__" in window);

const HOST_KEY = "codeterm-web-host";
const HOST_NAME_KEY = "codeterm-web-host-name";
const TOKEN_PREFIX = "codeterm-web-token:";

/** Origin the SPA was served from — the "self" peer. */
export function selfHost(): string {
  return `${window.location.protocol}//${window.location.host}`;
}

/** Current API target (self unless the peer picker switched). */
export function apiHost(): string {
  return sessionStorage.getItem(HOST_KEY) ?? selfHost();
}

export function setApiHost(host: string | null, name?: string): void {
  if (host === null || host === selfHost()) {
    sessionStorage.removeItem(HOST_KEY);
    sessionStorage.removeItem(HOST_NAME_KEY);
  } else {
    sessionStorage.setItem(HOST_KEY, host);
    // The friendly name is only knowable at switch time: the target daemon
    // doesn't list ITSELF in /api/mesh/peers, so post-reload lookups by
    // host would fall back to the raw IP.
    if (name) {
      sessionStorage.setItem(HOST_NAME_KEY, name);
    } else {
      sessionStorage.removeItem(HOST_NAME_KEY);
    }
  }
}

/** Friendly name of the switched-to host, captured at switch time. */
export function apiHostName(): string | null {
  return sessionStorage.getItem(HOST_NAME_KEY);
}

export function tokenFor(host: string): string | null {
  return localStorage.getItem(TOKEN_PREFIX + host);
}

export function setTokenFor(host: string, token: string): void {
  localStorage.setItem(TOKEN_PREFIX + host, token);
}

export function clearTokenFor(host: string): void {
  localStorage.removeItem(TOKEN_PREFIX + host);
}

export function currentToken(): string | null {
  return tokenFor(apiHost());
}

/** Local-daemon address shape (the desktop app's `config.api`). */
export interface DaemonApiConfig {
  host?: string | null;
  port?: number | null;
  token?: string | null;
}

/**
 * Canonical HTTP origin of the daemon to reach for plugin assets, iframes, and
 * any direct fetch — the single source of truth across every access mode.
 *
 * Web: the origin the SPA is actually talking to (`apiHost()`), which already
 * tracks window.location for localhost / LAN-or-Tailscale IP / Tailscale serve
 * (https, no port) and the peer picker. Desktop (Tauri): the local daemon, since
 * the app shell runs off a Vite/`tauri://` origin and only the daemon serves the
 * plugin routes. Never reconstruct from the daemon's bind address (`0.0.0.0`).
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

/** Authenticated fetch against the current API target. */
export async function webFetch(
  path: string,
  init: RequestInit = {},
): Promise<Response> {
  const headers = new Headers(init.headers);
  headers.set("Content-Type", "application/json");
  const token = currentToken();
  if (token) headers.set("Authorization", `Bearer ${token}`);
  return fetch(`${apiHost()}${path}`, { ...init, headers });
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
