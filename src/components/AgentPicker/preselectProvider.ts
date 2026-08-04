import type { ProviderDescriptor } from "../../lib/types";
import { PREFERRED_GENERAL_PROVIDER_ID } from "./constants";

export function generalAgentProviders(
  providers: ProviderDescriptor[],
): ProviderDescriptor[] {
  return providers.filter((provider) => provider.transport === "pty");
}

export function providerRequiresCli(provider: ProviderDescriptor): boolean {
  return Boolean(provider.cli_binary);
}

export function isProviderInstalled(
  provider: ProviderDescriptor,
  installedByBinary: Record<string, boolean>,
): boolean {
  if (!providerRequiresCli(provider)) return true;
  const binary = provider.cli_binary;
  if (!binary) return true;
  return installedByBinary[binary] === true;
}

export function hasAnyInstalledProvider(
  providers: ProviderDescriptor[],
  installedByBinary: Record<string, boolean>,
): boolean {
  return providers.some((p) => isProviderInstalled(p, installedByBinary));
}

/** Claude when installed, else first installed in descriptor order, else null. */
export function preselectProviderId(
  providers: ProviderDescriptor[],
  installedByBinary: Record<string, boolean>,
): string | null {
  const preferred = providers.find((p) => p.id === PREFERRED_GENERAL_PROVIDER_ID);
  if (preferred && isProviderInstalled(preferred, installedByBinary)) {
    return preferred.id;
  }
  const firstInstalled = providers.find((p) => isProviderInstalled(p, installedByBinary));
  return firstInstalled?.id ?? null;
}
