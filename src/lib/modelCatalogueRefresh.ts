import { invoke } from "./bridge";

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

export function refreshFailureMessage(reason: string | undefined): string {
  const cause = reason ? `: ${reason}` : "";
  return `The model list could not be refreshed${cause}. The current list stays in use.`;
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
