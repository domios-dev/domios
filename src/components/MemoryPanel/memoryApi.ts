import type { ApiConfig } from "../../lib/types";
import type { GraphFullResponse, MemItemFull } from "./types";
import type {
  DeltaResponse,
  GraphSnapshot,
  LabelRow,
  SnapshotEdge,
} from "./snapshotTypes";
import { IS_WEB, apiHost, currentToken } from "../../lib/webApi";

export class SnapshotUnavailableError extends Error {}

export function baseUrl(api: ApiConfig): string {
  if (IS_WEB) return apiHost();
  const host = api.host === "0.0.0.0" ? "127.0.0.1" : api.host;
  return `http://${host}:${api.port}`;
}

export function authHeaders(api: ApiConfig): HeadersInit {
  const token = IS_WEB ? currentToken() : api.token;
  return token ? { Authorization: `Bearer ${token}` } : {};
}

export function jsonHeaders(api: ApiConfig): HeadersInit {
  return {
    "Content-Type": "application/json",
    ...authHeaders(api),
  };
}

export function isTauriEnv(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

export async function fetchGraphFull(
  api: ApiConfig,
  signal?: AbortSignal,
): Promise<GraphFullResponse> {
  const res = await fetch(`${baseUrl(api)}/api/mem/graph/full`, {
    headers: authHeaders(api),
    signal,
  });
  if (!res.ok) throw new Error(`graph/full failed: ${res.status} ${await res.text()}`);
  return res.json();
}

export async function fetchSnapshot(
  api: ApiConfig,
  signal?: AbortSignal,
): Promise<GraphSnapshot> {
  const res = await fetch(`${baseUrl(api)}/api/mem/graph/snapshot`, {
    headers: authHeaders(api),
    signal,
  });
  if (res.status === 404) throw new SnapshotUnavailableError("snapshot endpoint missing");
  if (!res.ok) throw new Error(`snapshot failed: ${res.status} ${await res.text()}`);
  return res.json();
}

export async function fetchSnapshotDelta(
  api: ApiConfig,
  sinceVersion: number,
  sinceSeq: number,
  signal?: AbortSignal,
): Promise<DeltaResponse> {
  const url = `${baseUrl(api)}/api/mem/graph/snapshot?since_version=${sinceVersion}&since_seq=${sinceSeq}`;
  const res = await fetch(url, { headers: authHeaders(api), signal });
  if (res.status === 404) throw new SnapshotUnavailableError("snapshot endpoint missing");
  if (!res.ok) throw new Error(`snapshot delta failed: ${res.status} ${await res.text()}`);
  return res.json();
}

export async function fetchLabels(
  api: ApiConfig,
  ids: string[],
  signal?: AbortSignal,
): Promise<LabelRow[]> {
  if (ids.length === 0) return [];
  const qs = encodeURIComponent(ids.join(","));
  const res = await fetch(`${baseUrl(api)}/api/mem/graph/labels?ids=${qs}`, {
    headers: authHeaders(api),
    signal,
  });
  if (!res.ok) throw new Error(`labels failed: ${res.status}`);
  return res.json();
}

export async function fetchGraphEdges(
  api: ApiConfig,
  ids: string[],
  signal?: AbortSignal,
): Promise<SnapshotEdge[]> {
  if (ids.length === 0) return [];
  const qs = encodeURIComponent(ids.join(","));
  const res = await fetch(`${baseUrl(api)}/api/mem/graph/edges?ids=${qs}`, {
    headers: authHeaders(api),
    signal,
  });
  if (!res.ok) throw new Error(`edges failed: ${res.status}`);
  return res.json();
}

export async function setPinnedPosition(
  api: ApiConfig,
  id: string,
  x: number,
  y: number,
): Promise<void> {
  const res = await fetch(
    `${baseUrl(api)}/api/mem/pinned-positions/${encodeURIComponent(id)}`,
    {
      method: "PATCH",
      headers: { ...authHeaders(api), "Content-Type": "application/json" },
      body: JSON.stringify({ x, y }),
    },
  );
  if (!res.ok) throw new Error(`pin failed: ${res.status}`);
}

export async function rebuildLayout(api: ApiConfig): Promise<{ started: boolean }> {
  const res = await fetch(`${baseUrl(api)}/api/mem/graph/layout/rebuild`, {
    method: "POST",
    headers: authHeaders(api),
  });
  if (res.status === 409) return { started: false };
  if (!res.ok) throw new Error(`rebuild failed: ${res.status}`);
  return res.json();
}

export async function fetchItemFull(api: ApiConfig, id: string): Promise<MemItemFull> {
  const res = await fetch(`${baseUrl(api)}/api/mem/items/${encodeURIComponent(id)}`, {
    headers: authHeaders(api),
  });
  if (!res.ok) throw new Error(`get item failed: ${res.status} ${await res.text()}`);
  return res.json();
}

export interface ItemPatch {
  title?: string;
  summary?: string;
  area?: string;
  tags?: string[];
  body?: string;
  importance?: number;
}

export async function patchItem(
  api: ApiConfig,
  id: string,
  patch: ItemPatch,
): Promise<{ updated: string }> {
  const res = await fetch(`${baseUrl(api)}/api/mem/items/${encodeURIComponent(id)}`, {
    method: "PATCH",
    headers: { ...authHeaders(api), "Content-Type": "application/json" },
    body: JSON.stringify(patch),
  });
  if (!res.ok) throw new Error(`patch item failed: ${res.status} ${await res.text()}`);
  return res.json();
}

export async function deleteItem(api: ApiConfig, id: string): Promise<{ deleted: boolean }> {
  const res = await fetch(`${baseUrl(api)}/api/mem/items/${encodeURIComponent(id)}`, {
    method: "DELETE",
    headers: authHeaders(api),
  });
  if (!res.ok) throw new Error(`delete item failed: ${res.status} ${await res.text()}`);
  return res.json();
}

export interface MemStats {
  total_items: number;
  total_vectors: number;
  total_relations: number;
  total_entities: number;
  by_kind: Record<string, number>;
}

export async function fetchWorkerLog(api: ApiConfig, workerId: string): Promise<string> {
  const res = await fetch(`${baseUrl(api)}/api/mem/workers/${encodeURIComponent(workerId)}/log`, {
    headers: authHeaders(api),
  });
  if (!res.ok) {
    if (res.status === 404) return "(no log available)";
    throw new Error(`worker log failed: ${res.status}`);
  }
  return res.text();
}

export async function fetchStats(api: ApiConfig): Promise<MemStats> {
  const res = await fetch(`${baseUrl(api)}/api/mem/stats`, {
    headers: authHeaders(api),
  });
  if (!res.ok) throw new Error(`stats failed: ${res.status}`);
  return res.json();
}
