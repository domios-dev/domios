export const REST_READ_TIMEOUT_MS = 30_000;
// Above the daemon's 120s mesh relay budget, so the server's delivery classification always answers first.
export const REST_WRITE_TIMEOUT_MS = 130_000;

const LONG_POLL_PATH = /^\/api\/(?:pty\/[^/]+\/read|tabs\/[^/]+\/pty\/read)$/;

export function isLongPollRequest(path: string): boolean {
  const url = new URL(path, "http://daemon");
  return url.searchParams.get("wait") === "true" || LONG_POLL_PATH.test(url.pathname);
}

/** Bound for one non-stream REST call; `null` means unbounded. An explicit caller value always wins. */
export function restRequestTimeoutMs(method: string, path: string, explicit?: number | null): number | null {
  if (explicit !== undefined) return explicit;
  if (isLongPollRequest(path)) return null;
  const verb = method.toUpperCase();
  return verb === "GET" || verb === "HEAD" ? REST_READ_TIMEOUT_MS : REST_WRITE_TIMEOUT_MS;
}

export function restRequestSignal(method: string, path: string, opts?: { signal?: AbortSignal; timeoutMs?: number | null }): AbortSignal | undefined {
  const timeoutMs = restRequestTimeoutMs(method, path, opts?.timeoutMs);
  const deadline = timeoutMs === null ? undefined : AbortSignal.timeout(timeoutMs);
  return opts?.signal && deadline ? AbortSignal.any([opts.signal, deadline]) : opts?.signal ?? deadline;
}
