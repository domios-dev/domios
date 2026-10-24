import type { ProviderDescriptor } from "../../lib/types";
import { isProviderSelectable, providerAvailability } from "../../lib/providerAvailability";
import { PREFERRED_GENERAL_PROVIDER_ID } from "./constants";

export function generalAgentProviders(
  providers: ProviderDescriptor[],
): ProviderDescriptor[] {
  return providers.filter((provider) =>
    (provider.transport === "pty" ||
      (provider.transport === "chat_backend" && provider.agent_runtime != null)) &&
    (provider.general_agent_offer_when_absent === true || providerAvailability(provider) === "available"),
  );
}

export function providerRequiresCli(provider: ProviderDescriptor): boolean {
  return Boolean(provider.cli_binary);
}

export { isProviderSelectable, providerAvailability };

export function hasAnySelectableProvider(providers: ProviderDescriptor[]): boolean {
  return providers.some(isProviderSelectable);
}

/** Claude when it can launch, else the first that can, else null. */
export function preselectProviderId(providers: ProviderDescriptor[]): string | null {
  const preferred = providers.find((p) => p.id === PREFERRED_GENERAL_PROVIDER_ID);
  if (preferred && isProviderSelectable(preferred)) return preferred.id;
  return providers.find(isProviderSelectable)?.id ?? null;
}
