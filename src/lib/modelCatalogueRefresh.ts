import { invoke } from "./bridge";
import type { Notification } from "./notificationTypes";

export interface ModelCatalogueRefreshOutcome {
  refreshed: boolean;
  reason?: string;
}

export interface ModelCatalogueRefreshApi {
  refresh(providerId: string): Promise<ModelCatalogueRefreshOutcome>;
}

/** The desktop command answers a bool; the web route answers `{ refreshed, reason }`. */
export function refreshOutcomeFromResponse(value: unknown): ModelCatalogueRefreshOutcome {
  if (typeof value === "boolean") return { refreshed: value };
  if (typeof value !== "object" || value === null) return { refreshed: false };
  const { refreshed, reason } = value as { refreshed?: unknown; reason?: unknown };
  return {
    refreshed: refreshed === true,
    ...(typeof reason === "string" && reason.trim() ? { reason: reason.trim() } : {}),
  };
}

/** A one-line local toast; the daemon log keeps the full reason, Copy hands it over. */
export function refreshFailureToast(
  providerId: string,
  reason: string | undefined,
  copy: (text: string) => void | Promise<void>,
): Omit<Notification, "timestamp" | "read"> {
  return {
    id: `model-catalogue-refresh:${providerId}`,
    tab_id: null,
    workspace_id: null,
    title: "Couldn't refresh models",
    body: null,
    severity: "Warning",
    level: "verbose",
    ...(reason ? { local_actions: [{ label: "Copy", run: () => copy(reason) }] } : {}),
  };
}

const defaultApi: ModelCatalogueRefreshApi = {
  refresh: async (providerId) => refreshOutcomeFromResponse(await invoke<unknown>("refresh_provider_models", { providerId })),
};

let apiOverride: ModelCatalogueRefreshApi | null = null;

export function getModelCatalogueRefreshApi(): ModelCatalogueRefreshApi {
  return apiOverride ?? defaultApi;
}

export function __setModelCatalogueRefreshApiForTest(api: ModelCatalogueRefreshApi | null): void {
  apiOverride = api;
}
