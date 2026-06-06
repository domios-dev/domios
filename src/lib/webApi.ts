/**
 * Web-mirror runtime: API host + per-peer token management.
 *
 * In WEB mode (no Tauri) the React app talks to a CodeTerm daemon over
 * plain HTTP. The default target is the daemon that served /web (same
 * origin). The peer picker can flip the target to any paired peer's own
 * address — peers can't verify each other's browser sessions (mesh auth
 * is Ed25519-signed or the peer's own api token, and browsers can't
 * sign), so each peer gets its OWN token, stored per host.
 */

export const IS_WEB =
  typeof window !== "undefined" && !("__TAURI_INTERNALS__" in window);

const HOST_KEY = "codeterm-web-host";
const TOKEN_PREFIX = "codeterm-web-token:";

/** Origin the SPA was served from — the "self" peer. */
export function selfHost(): string {
  return `${window.location.protocol}//${window.location.host}`;
}

/** Current API target (self unless the peer picker switched). */
export function apiHost(): string {
  return sessionStorage.getItem(HOST_KEY) ?? selfHost();
}

export function setApiHost(host: string | null): void {
  if (host === null || host === selfHost()) {
    sessionStorage.removeItem(HOST_KEY);
  } else {
    sessionStorage.setItem(HOST_KEY, host);
  }
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
