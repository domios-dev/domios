import { DEV_BUILD } from "./devBuild";
import { frontendBootDiagnostic } from "./frontendBootDiagnostic";

export interface RequestFailure {
  method: string;
  path: string;
  pageOrigin: string;
  targetOrigin: string;
  kind: "network" | "network-or-cors" | "opaque" | "http" | "aborted";
  status?: number;
}

export function requestFailure(
  input: RequestInfo | URL, init: RequestInit | undefined, pageUrl: string,
  response?: Pick<Response, "type" | "ok" | "status">,
): RequestFailure | undefined {
  const url = new URL(input instanceof Request ? input.url : String(input), pageUrl);
  if (url.pathname === "/api/frontend/diagnostic" || response?.ok) return undefined;
  const pageOrigin = new URL(pageUrl).origin;
  return {
    method: (init?.method ?? (input instanceof Request ? input.method : "GET")).toUpperCase(),
    path: url.pathname,
    pageOrigin,
    targetOrigin: url.origin,
    kind: response ? response.type === "opaque" || response.type === "opaqueredirect" ? "opaque" : "http"
      : init?.signal?.aborted ? "aborted" : pageOrigin === url.origin ? "network" : "network-or-cors",
    ...(response && response.status > 0 ? { status: response.status } : {}),
  };
}

export function instrumentFetch(
  fetcher: typeof fetch, pageUrl: () => string, report: (failure: RequestFailure) => void,
): typeof fetch {
  return async (input, init) => {
    let response: Response;
    try {
      response = await fetcher(input, init);
    } catch (error) {
      const failure = requestFailure(input, init, pageUrl());
      if (failure) report(failure);
      throw error;
    }
    const failure = requestFailure(input, init, pageUrl(), response);
    if (failure) report(failure);
    return response;
  };
}

export function installRequestDiagnostics(): void {
  if (!DEV_BUILD || typeof window === "undefined") return;
  window.fetch = instrumentFetch(window.fetch.bind(window), () => window.location.href,
    (request) => frontendBootDiagnostic("request-failed", { request }));
}
