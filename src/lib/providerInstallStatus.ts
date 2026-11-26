import { invoke } from "./bridge";
import type { ProviderCliStatus } from "./providerCli";
import type { ProviderDescriptor } from "./types";

export async function probeProviderInstalled(
  provider: ProviderDescriptor,
  options?: { refresh?: boolean },
): Promise<boolean> {
  if (provider.transport === "chat_backend" && !provider.agent_runtime && !provider.cli_binary) {
    return true;
  }
  if (provider.agent_runtime?.launchProfiles != null) {
    return true;
  }
  try {
    const args = options?.refresh ? { providerId: provider.id, refresh: true } : { providerId: provider.id };
    const status = await invoke<ProviderCliStatus>("provider_cli_status", args);
    return !status.required || status.installed !== false;
  } catch {
    return false;
  }
}

export async function probeProviderInstallMap(
  providers: ProviderDescriptor[],
  options?: { refresh?: boolean },
): Promise<Record<string, boolean>> {
  const entries = await Promise.all(
    providers.map(async (provider) => [provider.id, await probeProviderInstalled(provider, options)] as const),
  );
  return Object.fromEntries(entries);
}
