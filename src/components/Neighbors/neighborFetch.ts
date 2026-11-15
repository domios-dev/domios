import { localFetch, relayTarget, webFetch } from "../../lib/webApi";

/** Remote views read the selected installation; local owner actions retain their existing authority. */
export function neighborFetch(path: string, init: RequestInit = {}): Promise<Response> {
  if (!relayTarget()) return localFetch(path, init);
  const target = path === "/api/account/neighbors" ? "/api/neighbors/list"
    : path === "/api/account/neighbors/runtime" ? "/api/neighbors/runtime"
    : path === "/api/account/neighbors/files" ? "/api/neighbors/files" : null;
  if (!target) return Promise.reject(new Error("Open this machine directly to change its Neighbors settings."));
  return webFetch(target, init);
}
