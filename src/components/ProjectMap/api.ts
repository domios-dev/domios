import type { ApiConfig } from "../../lib/types";
import type { ProjectMapPayload } from "./types";

export function baseUrl(api: ApiConfig): string {
  const host = api.host === "0.0.0.0" ? "127.0.0.1" : api.host;
  return `http://${host}:${api.port}`;
}

export function authHeaders(api: ApiConfig): HeadersInit {
  return api.token ? { Authorization: `Bearer ${api.token}` } : {};
}

export async function fetchProjectMapGraph(
  api: ApiConfig,
  workspaceId: string,
  signal?: AbortSignal,
): Promise<ProjectMapPayload> {
  const url = `${baseUrl(api)}/api/map/graph?workspace=${encodeURIComponent(workspaceId)}`;
  const res = await fetch(url, { headers: authHeaders(api), signal });
  if (!res.ok) {
    throw new Error(`map/graph failed: ${res.status} ${await res.text()}`);
  }
  return res.json();
}

export async function fetchProjectMapFocus(
  api: ApiConfig,
  workspaceId: string,
  nodeId: number,
  kHops: number,
  signal?: AbortSignal,
): Promise<ProjectMapPayload> {
  const url =
    `${baseUrl(api)}/api/map/graph?workspace=${encodeURIComponent(workspaceId)}` +
    `&node=${nodeId}&k_hops=${kHops}`;
  const res = await fetch(url, { headers: authHeaders(api), signal });
  if (!res.ok) {
    throw new Error(`map/graph focus failed: ${res.status} ${await res.text()}`);
  }
  return res.json();
}
