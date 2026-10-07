import { invoke } from "./bridge";
import type { ProviderCliStatus } from "./providerCli";
import type { ProviderDescriptor } from "./types";

export async function probeProviderInstalled(provider: ProviderDescriptor): Promise<boolean> {
  if (provider.transport === "chat_backend") return true;
  try {
    const status = await invoke<ProviderCliStatus>("provider_cli_status", { providerId: provider.id });
    return !status.required || status.installed !== false;
  } catch {
    return false;
  }
}

export async function probeProviderInstallMap(
  providers: ProviderDescriptor[],
): Promise<Record<string, boolean>> {
  const entries = await Promise.all(
    providers.map(async (provider) => [provider.id, await probeProviderInstalled(provider)] as const),
  );
  return Object.fromEntries(entries);
}
