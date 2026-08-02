import { invoke, listen } from "./bridge";
import type { AppConfig, ProviderConfig } from "./types";


let cached: AppConfig | null = null;
let loadPromise: Promise<AppConfig> | null = null;
let listening = false;

function ensureListener(): void {
  if (listening) return;
  listening = true;
  void listen<AppConfig>("config-changed", (e) => { cached = e.payload; })
    .catch(() => { listening = false; });
}

export function loadConfig(): Promise<AppConfig> {
  ensureListener();
  if (cached) return Promise.resolve(cached);
  if (loadPromise) return loadPromise;
  loadPromise = invoke<AppConfig>("get_config")
    .then((cfg) => { cached = cfg; return cfg; })
    .finally(() => { loadPromise = null; });
  return loadPromise;
}

export function getCachedConfig(): AppConfig | null {
  ensureListener();
  return cached;
}

export async function setUiSettings(patch: Record<string, unknown>): Promise<void> {
  const config = await invoke<AppConfig>("get_config");
  const next: AppConfig = { ...config, ui: { ...config.ui, ...patch } };
  await invoke("update_config", { config: next });
  cached = next;
}

export async function setAgentSettings(patch: Partial<AppConfig["agent"]>): Promise<void> {
  const config = await invoke<AppConfig>("get_config");
  const next: AppConfig = { ...config, agent: { ...config.agent, ...patch } };
  await invoke("update_config", { config: next });
  cached = next;
}

export async function setProviderSettings(
  providerId: string,
  patch: Partial<ProviderConfig> | Record<string, unknown>,
): Promise<void> {
  const config = await invoke<AppConfig>("get_config");
  const agents = { ...(config.agents ?? {}) };
  agents[providerId] = { ...(agents[providerId] ?? {}), ...patch };
  const next: AppConfig = { ...config, agents };
  await invoke("update_config", { config: next });
  cached = next;
}
