import { useCallback, useEffect } from "react";
import { invoke } from "../lib/bridge";
import { useContributions } from "./useContributions";
import { attachModelCatalogueChangedRefresh } from "./useAgentProviders";

export interface ModelPresetChoice {
  provider: string;
  name: string;
  model: string;
}

async function fetchPresets(): Promise<ModelPresetChoice[]> {
  try {
    const result = await invoke<{ provider: string; presets: Array<{ name: string; model: string }> }>("model_provider_manage", { request: { action: "preset_list" } });
    return result.presets.map((preset) => ({ provider: result.provider, name: preset.name, model: preset.model }));
  } catch (error) {
    if (String(error) === "model provider operation failed (unsupported)") return [];
    throw error;
  }
}

export function useModelPresets() {
  const fetcher = useCallback(fetchPresets, []);
  const { items: presets, loading, error, refetch } = useContributions("agent_provider", fetcher);
  useEffect(() => attachModelCatalogueChangedRefresh(refetch), [refetch]);
  return { presets, loading, error };
}
