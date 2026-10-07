import { invoke } from "../../lib/bridge";

export interface ModelProviderEntry {
  id: string;
  kind: "openai-compatible" | "anthropic";
  baseUrl: string;
  models: string[];
  name?: string;
  secretName?: string;
}

export interface ModelProviderForm {
  name: string;
  kind: ModelProviderEntry["kind"];
  baseUrl: string;
  sourceSecret: string;
  models: string;
}

export function modelProviderAddRequest(form: ModelProviderForm) {
  return {
    action: "add_from_secret",
    source_secret: form.sourceSecret.trim(),
    provider: {
      id: form.name.trim(),
      kind: form.kind,
      baseUrl: form.baseUrl.trim(),
      models: [...new Set(form.models.split(/[\s,]+/).filter(Boolean))],
    },
  };
}

export interface ModelProviderApi {
  list(): Promise<ModelProviderEntry[]>;
  add(form: ModelProviderForm): Promise<void>;
  remove(name: string): Promise<void>;
  test(name: string): Promise<string[]>;
}

const call = <T>(request: unknown) => invoke<T>("model_provider_manage", { request });

export const modelProviderApi: ModelProviderApi = {
  async list() { return (await call<{ providers: ModelProviderEntry[] }>({ action: "list" })).providers; },
  async add(form) { await call(modelProviderAddRequest(form)); },
  async remove(name) { await call({ action: "remove", name }); },
  async test(name) { return (await call<{ models: string[] }>({ action: "test", name })).models; },
};
