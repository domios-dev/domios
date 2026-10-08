import { invoke } from "../../../lib/bridge";
import { daemonFetch, type DaemonApiConfig } from "../../../lib/webApi";
import type { AddRequest, DiscoverRequest, ModelEndpoint, UpdateRequest } from "./modelEndpointsModel";

export interface ModelEndpointsApi {
  list(): Promise<ModelEndpoint[]>;
  add(request: AddRequest): Promise<void>;
  update(request: UpdateRequest): Promise<void>;
  remove(id: string): Promise<void>;
  test(id: string): Promise<string[]>;
  discover(request: DiscoverRequest): Promise<string[]>;
  savedSecrets(): Promise<string[]>;
}

export function modelEndpointsApi(providerId: string, api?: DaemonApiConfig): ModelEndpointsApi {
  const call = <T>(request: unknown) => invoke<T>("model_provider_manage", { request, providerId });
  return {
    async list() { return (await call<{ providers: ModelEndpoint[] }>({ action: "list" })).providers; },
    async add(request) { await call(request); },
    async update(request) { await call(request); },
    async remove(name) { await call({ action: "remove", name }); },
    async test(name) { return (await call<{ models: string[] }>({ action: "test", name })).models; },
    async discover(request) { return (await call<{ models: string[] }>(request)).models; },
    async savedSecrets() {
      const response = await daemonFetch(api ?? {}, "/api/secrets/items?limit=500");
      if (!response.ok) return [];
      const body = (await response.json()) as { items?: { name?: unknown }[] };
      return (body.items ?? []).flatMap((item) => (typeof item.name === "string" ? [item.name] : []));
    },
  };
}
