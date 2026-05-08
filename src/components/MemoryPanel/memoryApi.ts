import type { ApiConfig } from "../../lib/types";
import type { GraphFullResponse, MemItemFull } from "./types";

export function baseUrl(api: ApiConfig): string {
  const host = api.host === "0.0.0.0" ? "127.0.0.1" : api.host;
  return `http://${host}:${api.port}`;
}

export function authHeaders(api: ApiConfig): HeadersInit {
  return api.token ? { Authorization: `Bearer ${api.token}` } : {};
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
