import { invoke } from "./bridge";

export interface ModelCatalogueRefreshApi {
  refresh(providerId: string): Promise<boolean>;
}

/** The desktop command answers a bool; the web route answers `{ refreshed }`. */
export function refreshedFromResponse(value: unknown): boolean {
  if (typeof value === "boolean") return value;
  return typeof value === "object" && value !== null && (value as { refreshed?: unknown }).refreshed === true;
}

const defaultApi: ModelCatalogueRefreshApi = {
  refresh: async (providerId) => refreshedFromResponse(await invoke<unknown>("refresh_provider_models", { providerId })),
};

let apiOverride: ModelCatalogueRefreshApi | null = null;

export function getModelCatalogueRefreshApi(): ModelCatalogueRefreshApi {
  return apiOverride ?? defaultApi;
}

export function __setModelCatalogueRefreshApiForTest(api: ModelCatalogueRefreshApi | null): void {
  apiOverride = api;
}
