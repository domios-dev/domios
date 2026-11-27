import { localFetch } from "../webApi";
import type {
  CloudBootstrapSnapshot,
  CloudCreatePhases,
  PrepareProviderSignInResponse,
} from "./types";

async function jsonRequest<T>(
  method: string,
  path: string,
  body?: unknown,
  signal?: AbortSignal,
): Promise<T> {
  const response = await localFetch(path, {
    method,
    signal,
    ...(body !== undefined
      ? { headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) }
      : {}),
  });
  const text = await response.text();
  if (!response.ok) {
    throw new Error(text.trim() || `Request failed (${response.status})`);
  }
  return text ? (JSON.parse(text) as T) : ({} as T);
}

export function getCloudBootstrapStatus(
  machineId: string,
  signal?: AbortSignal,
): Promise<CloudBootstrapSnapshot> {
  return jsonRequest<CloudBootstrapSnapshot>(
    "GET",
    `/api/mesh/cloud/${encodeURIComponent(machineId)}/bootstrap`,
    undefined,
    signal,
  );
}

export function prepareCloudProviderSignIn(
  machineId: string,
  params: { provider_id?: string; profile_id?: string } = {},
  signal?: AbortSignal,
): Promise<PrepareProviderSignInResponse> {
  return jsonRequest<PrepareProviderSignInResponse>(
    "POST",
    `/api/mesh/cloud/${encodeURIComponent(machineId)}/provider-signin/prepare`,
    params,
    signal,
  );
}

export function recordCloudProviderSignInTiming(
  machineId: string,
  params: { started_at_ms?: number; completed_at_ms?: number },
  signal?: AbortSignal,
): Promise<{ machine_id: string; create_phases: CloudCreatePhases }> {
  return jsonRequest<{ machine_id: string; create_phases: CloudCreatePhases }>(
    "POST",
    `/api/mesh/cloud/${encodeURIComponent(machineId)}/provider-signin/timing`,
    params,
    signal,
  );
}
