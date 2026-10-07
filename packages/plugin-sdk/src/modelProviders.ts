export interface ModelProviderDefinition {
  id: string;
  kind: "openai-compatible" | "anthropic";
  baseUrl: string;
  models: string[];
  keyEnv?: string;
  name?: string;
  secretName?: string;
}

export interface ModelProviderConfig {
  readModelPresets(config: unknown): ModelPreset[];
  applyModelPresets(config: unknown, presets: ModelPreset[]): { config: object; changed: boolean; conflicts: string[] };
  modelPresetLaunchArgs(name: string): string[];
  modelProviderConfigPath(): string | null;
  readModelProviders(config: unknown): ModelProviderDefinition[];
  applyModelProviders(config: unknown, providers: ModelProviderDefinition[]): {
    config: object;
    changed: boolean;
    conflicts: string[];
  };
  modelProviderKeyEnvName(id: string): string;
}

export interface ModelPreset {
  name: string;
  model: string;
  prompt: string;
  params?: { temperature?: number; top_p?: number; steps?: number };
}
