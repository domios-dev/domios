import type {OptionalWireFields} from "./wireViews";
export type ModelProviderDefinition = OptionalWireFields<import("./gen/ModelProviderDefinition").ModelProviderDefinition, "keyEnv" | "name" | "secretName">;

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

export type ModelPreset = OptionalWireFields<import("./gen/ModelPreset").ModelPreset, "params">;
