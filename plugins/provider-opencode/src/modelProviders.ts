import type { ModelProviderConfig, ModelProviderDefinition } from "@codeterm/plugin-sdk";
import { applyDomiosProviders, providerKeyEnvName } from "./opencodeConfig";
import { opencodeConfigPath } from "./helpers";
import { applyDomiosPresets, readDomiosPresets } from "./opencodePresets";

function record(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

export function readModelProviders(config: unknown): ModelProviderDefinition[] {
  if (!record(config) || (config.provider !== undefined && !record(config.provider))) {
    throw new Error("Invalid provider config");
  }
  return Object.entries(config.provider ?? {}).flatMap(([id, entry]) => {
    if (!record(entry) || entry["x-domios"] !== true) return [];
    if (!record(entry.options) || typeof entry.options.baseURL !== "string") {
      throw new Error("Invalid owned provider");
    }
    const kind = entry.npm === "@ai-sdk/anthropic" ? "anthropic"
      : entry.npm === "@ai-sdk/openai-compatible" ? "openai-compatible" : null;
    if (!kind) throw new Error("Unsupported owned provider");
    const apiKey = entry.options.apiKey;
    const env = typeof apiKey === "string" ? /^\{env:([A-Z][A-Z0-9_]*)\}$/.exec(apiKey)?.[1] : undefined;
    if (apiKey !== undefined && !env) throw new Error("Owned provider requires an env reference");
    const secret = entry["x-domios-secret"];
    if (secret !== undefined && (typeof secret !== "string" || secret.length === 0)) {
      throw new Error("Invalid owned secret reference");
    }
    return [{
      id, kind, baseUrl: entry.options.baseURL,
      models: record(entry.models) ? Object.keys(entry.models) : [],
      ...(typeof entry.name === "string" ? { name: entry.name } : {}),
      ...(env ? { keyEnv: env } : {}),
      ...(typeof secret === "string" ? { secretName: secret } : {}),
    }];
  });
}

export const modelProviderExports: ModelProviderConfig = {
  readModelPresets: readDomiosPresets,
  applyModelPresets: applyDomiosPresets,
  modelPresetLaunchArgs: (name) => ["--agent", name],
  modelProviderConfigPath: opencodeConfigPath,
  readModelProviders,
  applyModelProviders: applyDomiosProviders,
  modelProviderKeyEnvName: providerKeyEnvName,
};
