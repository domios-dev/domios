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
  modelProviderConfigPath(): string | null;
  readModelProviders(config: unknown): ModelProviderDefinition[];
  applyModelProviders(config: unknown, providers: ModelProviderDefinition[]): {
    config: object;
    changed: boolean;
    conflicts: string[];
  };
  modelProviderKeyEnvName(id: string): string;
}