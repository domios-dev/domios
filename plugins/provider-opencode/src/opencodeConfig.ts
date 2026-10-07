/**
 * Pure config writer for Domios-owned providers in OpenCode's config format.
 * No host calls, no IO — all functions are pure.
 */

export type DomiosProviderKind = "openai" | "openai-compatible" | "anthropic" | "lmstudio";

export interface DomiosProvider {
  id: string;
  kind: DomiosProviderKind;
  baseUrl: string;
  keyEnv?: string;
  models?: string[];
  name?: string;
  secretName?: string;
}

const OWNERSHIP_MARKER = "x-domios";

interface OpenCodeProviderEntry {
  npm: string;
  name?: string;
  options?: {
    baseURL: string;
    apiKey?: string;
  };
  models?: Record<string, Record<string, never>>;
  [OWNERSHIP_MARKER]?: boolean;
}

function isDomiosOwned(entry: unknown): entry is OpenCodeProviderEntry {
  return (
    typeof entry === "object" &&
    entry !== null &&
    (entry as Record<string, unknown>)[OWNERSHIP_MARKER] === true
  );
}

function buildProviderEntry(provider: DomiosProvider): OpenCodeProviderEntry {
  const npm =
    provider.kind === "anthropic" ? "@ai-sdk/anthropic" : "@ai-sdk/openai-compatible";

  const entry: OpenCodeProviderEntry = {
    npm,
    [OWNERSHIP_MARKER]: true,
  };

  if (provider.secretName !== undefined) {
    Object.assign(entry, { "x-domios-secret": provider.secretName });
  }

  if (provider.name !== undefined) {
    entry.name = provider.name;
  }

  entry.options = { baseURL: provider.baseUrl };
  if (provider.keyEnv !== undefined) {
    entry.options.apiKey = `{env:${provider.keyEnv}}`;
  }

  if (provider.models && provider.models.length > 0) {
    const models: Record<string, Record<string, never>> = {};
    for (const m of provider.models) {
      models[m] = {};
    }
    entry.models = models;
  }

  return entry;
}

function deepEqual(a: unknown, b: unknown): boolean {
  if (a === b) return true;
  if (typeof a !== typeof b) return false;
  if (a === null || b === null) return false;
  if (typeof a !== "object") return false;

  const aObj = a as Record<string, unknown>;
  const bObj = b as Record<string, unknown>;
  const aKeys = Object.keys(aObj);
  const bKeys = Object.keys(bObj);
  if (aKeys.length !== bKeys.length) return false;
  for (const key of aKeys) {
    if (!bKeys.includes(key)) return false;
    if (!deepEqual(aObj[key], bObj[key])) return false;
  }
  return true;
}

export function applyDomiosProviders(
  config: unknown,
  providers: DomiosProvider[],
): { config: object; changed: boolean; conflicts: string[] } {
  const base: Record<string, unknown> =
    typeof config === "object" && config !== null
      ? { ...(config as Record<string, unknown>) }
      : {};

  const existingProviderMap: Record<string, unknown> =
    typeof base.provider === "object" && base.provider !== null
      ? { ...(base.provider as Record<string, unknown>) }
      : {};

  const wantedIds = new Set(providers.map((p) => p.id));
  for (const [key, value] of Object.entries(existingProviderMap)) {
    if (isDomiosOwned(value) && !wantedIds.has(key)) {
      delete existingProviderMap[key];
    }
  }

  const conflicts: string[] = [];
  for (const provider of providers) {
    const existing = existingProviderMap[provider.id];
    if (existing !== undefined && !isDomiosOwned(existing)) {
      conflicts.push(provider.id);
      continue;
    }
    existingProviderMap[provider.id] = buildProviderEntry(provider);
  }

  const result: Record<string, unknown> = { ...base };
  if (Object.keys(existingProviderMap).length > 0) {
    result.provider = existingProviderMap;
  } else {
    delete result.provider;
  }

  const changed = !deepEqual(config, result);
  return { config: result, changed, conflicts };
}

export function providerKeyEnvName(id: string): string {
  // Convert id to UPPER_SNAKE_CASE
  const snake = id
    .replace(/([a-z0-9])([A-Z])/g, "$1_$2")
    .replace(/[^a-zA-Z0-9]+/g, "_")
    .toUpperCase();
  return `DOMIOS_PROVIDER_${snake}_KEY`;
}
