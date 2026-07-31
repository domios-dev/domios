import type { ProviderDescriptor } from "../../lib/types";

export type WorkerSelection = {
  provider: ProviderDescriptor | undefined;
  providerLabel: string;
  defaultModelLabel: string;
  modelOptions: ProviderDescriptor["available_models"];
  modelCompatible: boolean;
};

function modelLabel(provider: ProviderDescriptor | undefined): string {
  if (!provider?.default_model) return "default";
  return provider.available_models.find((model) => model.id === provider.default_model)?.display_name
    ?? provider.default_model;
}

export function resolveWorkerSelection(
  providers: ProviderDescriptor[],
  selectedProvider: string,
  defaultProvider: string,
  selectedModel: string,
): WorkerSelection {
  const provider = providers.find((item) => item.id === (selectedProvider || defaultProvider));
  const modelOptions = provider?.available_models ?? [];
  return {
    provider,
    providerLabel: provider?.display_name ?? (selectedProvider || defaultProvider || "No provider"),
    defaultModelLabel: modelLabel(provider),
    modelOptions,
    modelCompatible: !selectedModel || modelOptions.some((model) => model.id === selectedModel),
  };
}
