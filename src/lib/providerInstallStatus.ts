import { invoke } from "./bridge";
import type { ProviderDescriptor } from "./types";

export async function probeProviderInstalled(provider: ProviderDescriptor): Promise<boolean> {
  if (!provider.cli_binary) return true;
  try {
    return await invoke<boolean>("check_cli_installed", { binary: provider.cli_binary });
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
